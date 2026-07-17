# JInk — logic notes for agents

Tauri 2 + React quick-input popup. Global hotkey → popup at cursor → type/AI-enhance/translate/voice → inject into whatever app had focus.

## Update rule

**Whenever you add, change, or remove app logic (not pure styling), update the relevant section below in the same commit/PR.** Keep entries short — what it does + why, not full code walkthrough. If a section grows stale (doesn't match code), fix it before adding new ones.

---

## Text injection (`src-tauri/src/inject.rs`)

- **Linux**: `xdotool type --clearmodifiers --delay 4 -- <text>` — types chars directly, no clipboard touch. X11-only, no Wayland support (no ydotool/wtype fallback yet). Avoids Ctrl+V vs Ctrl+Shift+V ambiguity that embedded terminals (host window class) can't be detected for.
- **macOS/Windows**: clipboard set (`arboard`) + simulated paste (`enigo`: Cmd+V on macOS, Ctrl+V on Windows), then restores previous clipboard after a delay.

Keep this split — don't switch Linux to clipboard+paste (terminal-detection bug blocks it) or macOS to xdotool-style typing (no xdotool on macOS; enigo paste is faster for long text and unicode-safe).

TODO: Wayland fallback (ydotool/wtype) not implemented.

## Popup show/hide & focus (`src-tauri/src/lib.rs`)

- Global hotkey (default `Alt+Space`, KWin: may collide with KRunner) toggles popup visibility.
- `show_popup`: position at cursor, show, `set_focus()`. Then re-asserts focus 3x over 150ms off-thread — KWin's focus-stealing prevention only honors `set_focus()` on first map, not re-show, so on Linux `force_focus_x11()` (`xdotool windowactivate` by name) is also fired as fallback each retry.
- `position_at_cursor`: places popup 12px right / 16px below cursor, clamped to the monitor under the cursor (falls back to `win.center()` if cursor position unavailable).
- `inject_text` command: hides popup first (returns focus to target app), then calls `inject::inject_text` on a blocking thread.

## Click-outside-to-hide vs helper apps (`lib.rs` + `Popup.tsx`)

- Optional (`settings.hideOnBlur`): popup hides when it loses OS focus.
- Exception: if the newly-focused window is a known "helper app" (clipboard manager or file manager — see `HELPER_APPS` list in `lib.rs`), popup drops `always_on_top` instead of hiding, so e.g. picking from CopyQ or a file manager doesn't dismiss the popup.
- Helper detection: Linux via `xdotool getactivewindow getwindowclassname`; macOS via `osascript` (frontmost process name); other OSes → always `false`.
- Popup's own native file dialog also suppresses blur-hide via `suppressBlurRef` (dialog itself would otherwise steal focus and trigger hide).

## AI streaming (`src-tauri/src/ai.rs` + `src/lib/ai.ts`)

- OpenAI-compatible `/chat/completions` with `stream: true`, `temperature: 0.3`.
- Rust side parses SSE (`data: {...}` lines, terminated by `data: [DONE]`), emits Tauri events per request: `ai-token` (delta), `ai-done`, `ai-error`. Events carry `request_id` so the frontend can ignore stale streams (e.g. user cancelled and started a new one).
- Frontend (`runAiStream`) registers listeners keyed by a fresh `crypto.randomUUID()` request id, invokes `ai_stream`, returns a `cleanup()` fn to detach listeners (called on done/error, or manually to cancel — `Popup.tsx` calls it again to abort mid-stream when the enhance/translate button is clicked while busy).
- Two prompt builders: `enhancePrompt` (grammar/clarity fix, same language) and `translatePrompt` (vi↔en, driven by `settings.translateFrom/To`). Both instruct the model to return only the result text, no wrapping.
- `Popup.tsx` streams tokens directly into the textarea (`setText(acc)` per token), saves pre-stream text as `undoText` for one-shot undo.

## Voice transcription (`src/lib/voice.ts`)

- Browser `MediaRecorder` (webm) records mic audio until `stop()`.
- Sent to OpenAI-compatible `/audio/transcriptions` (hardcoded `model: "whisper-1"`) as multipart form; result text appended to the textarea (with a leading space if non-empty).
- No Rust involvement — pure browser API + fetch.

## Attachments (`Popup.tsx`)

- Two paths: native file dialog (`pickFiles`, via `@tauri-apps/plugin-dialog`) yields absolute filesystem paths; paste (`onPaste`) yields in-memory `File` objects (images only, previewed as data URLs, **no filesystem path** — kind is inferred from MIME type not extension).
- On `insert()`, only attachments with a `path` (i.e., picked via dialog, not pasted) are appended to the injected text as newline-separated paths — pasted-image attachments without a path are silently dropped from injection (visual-only in the popup).

## Settings persistence (`src/lib/settings.ts`)

- `@tauri-apps/plugin-store`, single JSON file `settings.json`, single key `"settings"`, merged over `DEFAULT_SETTINGS` on load (so new fields get defaults without a migration step).
- Settings window (`open_settings` command in `lib.rs`) is a separate webview (`index.html#/settings`), singleton — reopening focuses existing window instead of creating a new one. Popup and Settings run in separate windows/processes-of-webview, so Popup re-reads settings from disk on every `popup-shown` event rather than sharing in-memory state.
- Saving settings also re-registers the global hotkey (`set_hotkey` command: unregister-all then register the new accelerator).

## System tray (`lib.rs`)

- Always-running tray icon (app stays alive after windows close — `ExitRequested` is intercepted with `api.prevent_exit()`). Menu: Open input / Settings / Quit.
