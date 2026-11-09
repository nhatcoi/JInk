# JInk — logic notes for agents

100% Pure Native Rust + GPUI quick-input popup. Global hotkey → popup at cursor → type/AI-enhance/translate → inject into whatever app had focus.

## Update rule

**Whenever you add, change, or remove app logic (not pure styling), update the relevant section below in the same commit/PR.** Keep entries short — what it does + why, not full code walkthrough. If a section grows stale (doesn't match code), fix it before adding new ones.

---

## Architecture (GPUI Native Rust)

- UI framework: **GPUI 0.2.2** (GPU-accelerated via Metal on macOS, Vulkan on Linux).
- Footprint: ~20MB RAM idle, 4.3MB release binary. Zero Webview runtime overhead.
- Styling: Gemini-style neutral dark pill (rounded-28, round send button, chip actions) via `src/theme.rs` (Catppuccin accents kept for settings/status).

## Text input (`src/text_input.rs`)

- Custom GPUI `TextInput` entity implementing `EntityInputHandler`: macOS IME (Telex/VNI, dead keys, emoji palette) works via marked text (`replace_and_mark_text_in_range`).
- Multi-line, wrapped at `WRAP_WIDTH` (516px), scrolls after `MAX_HEIGHT`. Text shaped with `shape_text` in `request_layout`; offsets <-> points via `WrappedLine::position_for_index` / `closest_index_for_position`.
- Keys are GPUI actions bound in context `JInkInput` (`bind_keys`): selection (Shift+arrows), word (Alt/Ctrl+arrows), line (Cmd+arrows/Home/End), doc (Cmd+Up/Down), delete word/line, Cmd+A/C/X/V, undo/redo (Cmd+Z / Cmd+Shift+Z, typing coalesced 900ms, one step per IME composition). Mouse: click, drag, double-click word, triple-click line, wheel scroll.
- Events to `JInkPopup`: `Submit` (Enter -> inject; Shift/Alt+Enter = newline), `UndoEmpty` (falls back to AI undo), `Changed`.
- AI streaming writes via `set_text_silent` / `append_silent` (no undo history; `undo_text` in popup restores the pre-AI text).

## Attachments, selection, click-outside (`src/popup.rs`)

- Ctrl+V with image-only clipboard (`TextInputEvent::PasteImage`) → saved as temp PNG via `arboard`; `^O`/attach button (`prompt_for_paths`) and drag-drop add files. Images insert a `[#ImageN]` token and ride inline on inject + go to Explain as multimodal; non-image files are appended as path lines on inject. Chips row shows attachments (× removes, token too).
- Linux: with `prefill_selection` setting on (off by default, toggle in settings), PRIMARY selection prefills the popup on open (`selected_text` in `main.rs`).
- Linux: with `selection_context` setting on (default), PRIMARY selection is captured on open as `JInkPopup.context` (chip with ×, never injected); `ai::enhance_prompt`/`explain_prompt` append it (≤4000 chars) as background context. Ignored when `prefill_selection` is on (selection goes into the input instead).
- Click-outside: 120ms poll of `window.is_window_active()` (gpui 0.2.2 has no activation observer); closes on active→inactive when `hide_on_blur` (toggle in settings), skipped while our file dialog is open.
- Drag: header (JINK wordmark/status) `mouse_down` → `window.start_window_move()`; `moving_since` pauses click-outside because the WM drops activation during a move.
- Linux window: `WindowKind::Normal` + `WindowDecorations::Client` (gpui `PopUp` = X11 NOTIFICATION type, KWin never keeps keyboard focus on it; Normal gets a KWin titlebar unless client decorations). KWin ignores the requested position, so `place_x11_popup` (xdotool/wmctrl) moves it to the chosen origin, pins above/skip-taskbar and focuses it after open.
- Linux: opaque window + opaque `pill_bg` (no reliable X11 blur → unreadable over terminals). Ctrl+Shift+T = reverse translate.
- Not ported from main yet: voice input, image/clipboard-helper exemption for blur.

## Text injection (`src/inject.rs`)

- Text is split on `[#ImageN]` tokens; each run (text or image) is put on the clipboard (`arboard`) and pasted.
- **Linux**: paste via `xdotool key --clearmodifiers`. Terminal detection checks `WM_CLASS` to use `ctrl+shift+v` for terminals and `ctrl+v` elsewhere.
- **macOS/Windows**: clipboard runs + simulated paste (`enigo`: Cmd+V on macOS, Ctrl+V on Windows).
- On inject, popup closes immediately, giving focus back to the target app before injecting.

## Popup show/hide & focus (`src/main.rs`)

- Global hotkey: dynamically loaded from `settings.hotkey` (default `Super+Space` on Linux, `Control+Cmd+Space` on macOS/Windows). Supports dynamic re-registration on changes via `update_global_hotkey` without restarting the app.
- On macOS, a low-latency `CGEventTap` listener detects isolated `Control + Cmd` press-and-release chord (<800ms with no intervening keypresses). Both `Control+Cmd` chord tap and `Control+Cmd+Space` seamlessly toggle popup visibility.
- `update_global_hotkey` strict-parses (partial text like `Alt+S` never registers), restores the old hotkey if the new one is taken. Settings only persist valid hotkeys.
- Popup origin clamped to the display under the cursor; `place_x11_popup` on open (see below).
- Hotkey loop uses non-blocking polling (`receiver.try_recv()` + async 30ms timer on `BackgroundExecutor`) to prevent main thread blocking.
- Placement: default = last position (`popup_x/popup_y`, saved by `remember_position` once the window sits still 2 polls; drag the header to move); `follow_cursor` setting (off by default) places it at the mouse instead; none saved → centered. Clamped to the display.
- `cursor_position` (follow_cursor only): uses CoreGraphics (`CGEventGetLocation`) on macOS and `xdotool getmouselocation` on Linux to place popup 12px right / 16px below cursor.
- Window kind is `PopUp`, transparent background, borderless.

## AI streaming (`src/ai.rs` + `src/popup.rs`)

- OpenAI-compatible `/chat/completions` with `stream: true`, `temperature: 0.3`.
- Network request runs on GPUI `background_executor()` to avoid freezing the UI.
- Tokens are sent over `tokio::sync::mpsc::unbounded_channel` to the UI thread, updating the textarea directly at 120fps.
- `strip_think_tags`: removes `<think>...</think>` reasoning models output in real-time.
- Pre-stream text is saved in `undo_text` for one-click undo (`Cmd+Z`).

## Settings persistence (`src/settings.rs`)

- Saved as JSON at `~/.config/jink/settings.json`.
- Settings view in `src/popup.rs` includes:
  - Preset buttons for OpenAI (`gpt-4o-mini`), Ollama (`localhost:11434`), and OpenRouter.
  - "Paste API Key from Clipboard" button for one-click key configuration.

## macOS Packaging (`scripts/build-macos.sh`)

- Generates `target/release/JInk.app` with `LSUIElement = true` (Accessory mode, no Dock icon) and `icon.icns`.
