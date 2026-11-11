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
- Multi-line, wrapped at `WRAP_WIDTH` (516px), popup grows with the text up to `MAX_HEIGHT` (308px ≈ 14 lines), then scrolls with a thin scrollbar thumb painted right of the text. Text shaped with `shape_text` in `request_layout`; offsets <-> points via `WrappedLine::position_for_index` / `closest_index_for_position`.
- Keys are GPUI actions bound in context `JInkInput` (`bind_keys`): selection (Shift+arrows), word (Alt/Ctrl+arrows), line (Cmd+arrows/Home/End), doc (Cmd+Up/Down), delete word/line, Cmd+A/C/X/V, undo/redo (Cmd+Z / Cmd+Shift+Z, typing coalesced 900ms, one step per IME composition). Mouse: click, drag, double-click word, triple-click line, wheel scroll.
- Events to `JInkPopup`: `Submit` (Enter -> inject; Shift/Alt+Enter = newline), `UndoEmpty` (falls back to AI undo), `Changed`.
- AI streaming writes via `set_text_silent` / `append_silent` (no undo history; `undo_text` in popup restores the pre-AI text).

## Attachments, selection, click-outside (`src/popup.rs`)

- Ctrl+V with image-only clipboard (`TextInputEvent::PasteImage`) → saved as temp PNG via `arboard`; `^O`/attach button (`prompt_for_paths`) and drag-drop add files. Images insert a `[#ImageN]` token and ride inline on inject + go to Explain as multimodal; non-image files are appended as path lines on inject. Chips row shows attachments (× removes, token too).
- Selection capture lives in `src/selection.rs`, one separate impl per OS (Linux PRIMARY selection; macOS reads `AXSelectedText` (then `AXSelectedTextMarkerRange` → `AXStringForTextMarkerRange` for Safari/Chromium pages) of the focused element over Accessibility first (few ms; an empty answer is trusted only from AXTextField/AXTextArea/AXComboBox, since web pages report "" while text is highlighted), and only if the app doesn't expose AX falls back to simulated Cmd+C with the clipboard restored, waiting for hotkey modifiers to be released (`selection::fast` / `selection::slow`). The popup never waits for it: `open_popup` takes the fast result inline; on `Fast::Slow` it opens the window unfocused (`focus: false`, target app keeps keyboard focus so the Cmd+C lands there), runs `slow()` on the background executor, then `attach_selection` + activates/focuses. Measured: popup drawn ~20ms after the hotkey (was ~330ms+); Windows none). Settings toggles show where `selection::SUPPORTED`.
- With `prefill_selection` on (off by default), the captured selection prefills the popup on open (`selection::selected_text`).
- With `selection_context` on (default), the selection is captured on open as `JInkPopup.context` (chip with ×, never injected); `ai::refine_prompt`/`enhance_prompt`/`explain_prompt` append it (≤4000 chars) as background context. Ignored when `prefill_selection` is on (selection goes into the input instead).
- Click-outside: 120ms poll of `window.is_window_active()` (gpui 0.2.2 has no activation observer); closes on active→inactive when `hide_on_blur` (toggle in settings), skipped while our file dialog is open.
- Drag: header (JINK wordmark/status) `mouse_down` → `window.start_window_move()`; `moving_since` pauses click-outside because the WM drops activation during a move.
- Linux window: `WindowKind::Normal` + `WindowDecorations::Client` (gpui `PopUp` = X11 NOTIFICATION type, KWin never keeps keyboard focus on it; Normal gets a KWin titlebar unless client decorations). KWin ignores the requested position, so `place_x11_popup` (xdotool/wmctrl) moves it to the chosen origin, pins above/skip-taskbar and focuses it after open.
- Linux: opaque window + opaque `pill_bg` (no reliable X11 blur → unreadable over terminals). Ctrl+Shift+T = reverse translate.
- Not ported from main yet: image/clipboard-helper exemption for blur.

## Text injection (`src/inject.rs`)

- Text is split on `[#ImageN]` tokens; each run (text or image) is put on the clipboard (`arboard`) and pasted.
- **Linux**: paste via `xdotool key --clearmodifiers`. Terminal detection checks `WM_CLASS` to use `ctrl+shift+v` for terminals and `ctrl+v` elsewhere.
- **macOS**: clipboard runs + Cmd+V posted via CoreGraphics (`src/mac_keys.rs`; enigo traps with SIGTRAP off the main thread). **Windows**: `enigo` Ctrl+V.
- On inject, popup closes immediately, giving focus back to the target app before injecting.

## Popup show/hide & focus (`src/main.rs`)

- Global hotkey: dynamically loaded from `settings.hotkey` (default `Control+Space` on macOS/Windows, `Super+Space` on Linux; Settings shows the per-OS presets from `settings::HOTKEY_PRESETS`). Supports dynamic re-registration on changes via `update_global_hotkey` without restarting the app.
- On macOS, a low-latency `CGEventTap` listener detects isolated `Control + Cmd` press-and-release chord (<800ms with no intervening keypresses). Both `Control+Cmd` chord tap and `Control+Cmd+Space` seamlessly toggle popup visibility.
- `update_global_hotkey` strict-parses (partial text like `Alt+S` never registers), restores the old hotkey if the new one is taken. Settings only persist valid hotkeys.
- Popup origin clamped to the display under the cursor; `place_x11_popup` on open (see below).
- Hotkey loop uses non-blocking polling (`receiver.try_recv()` + async 8ms timer on `BackgroundExecutor`) to prevent main thread blocking.
- Placement: default = last position (`popup_x/popup_y`, saved by `remember_position` once the window sits still 2 polls; drag the header to move); `follow_cursor` setting (off by default) places it at the mouse instead; none saved → centered. Clamped to the display; follow_cursor flips the popup above the pointer (not far up) when there is no room below.
- `cursor_position` (follow_cursor only): uses CoreGraphics (`CGEventGetLocation`) on macOS and `xdotool getmouselocation` on Linux to place popup 12px right / 16px below cursor.
- Window kind is `PopUp`, transparent background, borderless.

## AI streaming (`src/ai.rs` + `src/popup.rs`)

- OpenAI-compatible `/chat/completions` with `stream: true`, `temperature: 0.3`.
- Network request runs on GPUI `background_executor()` to avoid freezing the UI.
- Tokens are sent over `tokio::sync::mpsc::unbounded_channel` to the UI thread, updating the textarea directly at 120fps.
- `strip_think_tags`: removes `<think>...</think>` reasoning models output in real-time.
- Pre-stream text is saved in `undo_text` for one-click undo (`Cmd+Z`).

## Debug timing

- `JINK_TRACE=/path/log` (e.g. `open --env JINK_TRACE=/tmp/t.log /Applications/JInk.app`) appends `<epoch_ms> <tag>` lines from `trace()` in `main.rs` (hotkey recv, settings, selection fast/slow, open_window; tray icon hidden? usually a menu bar manager (Hidden Bar/Ice) pushing it off-screen). Off when unset.

## Settings persistence (`src/settings.rs`)

- Saved as JSON at `~/.config/jink/settings.json`.
- Settings view in `src/popup.rs` includes:
  - Preset buttons for OpenAI (`gpt-4o-mini`), Ollama (`localhost:11434`), and OpenRouter.
  - "Paste API Key from Clipboard" button for one-click key configuration.

## macOS Packaging (`scripts/build-macos.sh`)

- Signed with the first `Apple Development`/`Developer ID` identity (override `JINK_SIGN_ID`, `-` = ad-hoc) and identifier `com.nhatcoi.jink`, so the designated requirement is stable. Ad-hoc = cdhash requirement → every rebuild silently loses Accessibility (selection capture + paste stop working). No hardened runtime (it would need the audio-input entitlement for the mic).
- On launch `mac_tray::ensure_accessibility` calls `AXIsProcessTrustedWithOptions(prompt)`; `JINK_TRACE` logs `accessibility:trusted=…`.
- Menu bar: gpui forces `NSApplicationActivationPolicyRegular` at launch (overrides `LSUIElement`), so `mac_tray::hide_dock_icon` sets Accessory again; `mac_tray::install` adds a template tray icon (`assets/tray.svg` → `assets/icons/tray@2x.png`) with Open / Settings… / Quit, handled in the hotkey poll loop via `MenuEvent::receiver()` → `show_popup`.

- Generates `target/release/JInk.app` with `LSUIElement = true` (Accessory mode, no Dock icon) and `icon.icns`.

## UI language (`src/i18n.rs`)

- `settings.ui_lang` (`en` default / `vi`), switched in Settings; `i18n::tr(lang, key)` returns the string. Add new UI text there, never inline. Settings view is kept terse (chips, no hint paragraphs); window 720x500 in settings (scrollable).

## Main languages (`settings.lang_a` / `lang_b`, default en/vi)

- Picked in Settings (one chip row, exactly two selected; a new pick replaces the oldest). Every AI prompt (`ai::enhance/translate/explain_prompt`) tells the model the two languages: enhance/explain reply in the language of the user's text; translate auto-flips A↔B (other language → A). The popup just shows `A · B`; `Cmd+Shift+T`/reverse is the same action.

## AI result display (`src/popup.rs`)

- Enhance/explain keep the old text visible while the model works, then type the result into the input (typewriter, ~12ms ticks, whole text in ≤~40 steps). Errors leave the text untouched.
- Translate and explain never touch the input: the result is typed (same typewriter) into a `→` card under it (✕ or Cmd+Z dismisses; editing the input clears it). INSERT/Enter injects the translation when the card is shown.

## AI actions (`src/popup.rs`, prompts in `src/ai.rs`)

- `Cmd/Ctrl+R` refine (fix grammar/clarity, same language), `Cmd/Ctrl+E` enhance (rewrite a rough idea into a structured prompt for an AI, same language), `Cmd/Ctrl+T` translate, `Cmd/Ctrl+Enter` explain, `Cmd/Ctrl+O` attach. Input window is 680px wide. With an empty input, refine/enhance/translate/explain work on the selected-text context instead (`action_source`).

## Read aloud (`src/speech.rs`)

- `Cmd/Ctrl+L` / speaker ("loud") icon reads the translate/explain card, else the input, else the selected context; again to stop. Header bar and translation card have a dedicated speaker button that lights up vermilion while speaking (`speech::is_speaking()`). OS speech engine, one module per OS: macOS `say` (voice by script: Linh/Kyoko/Yuna/Tingting, falls back to default; starts with `[[slnc 800]]` lead silence for Bluetooth speakers), Linux `espeak-ng`/`espeak` then `spd-say`, Windows none. Language is detected from the script.

## Microphone & continuous voice-to-text (`src/mic.rs`, `src/stt.rs`)

- Mic icon / `Cmd/Ctrl+M` starts continuous voice-to-text with auto-stop on silence:
  - **Live Audio Waveform**: A 10-bar animated audio visualizer pulses in vermilion next to the language chips in the header in real-time based on RMS microphone levels.
  - **Continuous Dictation & Auto-stop**: Uses adaptive Voice Activity Detection (VAD). As you speak, utterances separated by short pauses (~760ms) are recognized and appended into the input field via a fast typewriter animation.
  - **Auto-stop on silence**: After finishing an utterance and pausing for ~3.5s (or ~8s if no speech occurred), the microphone shuts off automatically, or you can press `Cmd/Ctrl+M` to stop at any time.
  - **Native macOS Recognition**: Runs `SFSpeechRecognizer` in `src/stt.rs` in parallel across both configured main languages (`lang_a`, `lang_b`), picking the highest-confidence transcript with no API key or remote server required. Linux / bare binary falls back to `ai::transcribe`.
  - Transcription runs via `crate::runtime().spawn` (shared Tokio runtime). Never bare `tokio::spawn` inside a GPUI `cx.spawn`: no Tokio context there → panic → whole app aborts the moment a phrase ends.
- Permission is per OS in `mic::perm`: macOS reads `AVCaptureDevice` authorization (via `objc`); if blocked, or a recording is silent while not authorized, it opens System Settings → Privacy → Microphone. The first start triggers the system prompt; `NSMicrophoneUsageDescription` and `NSSpeechRecognitionUsageDescription` are set in `scripts/build-macos.sh` (a bare `cargo run` binary has no plist, so it uses the AI API path). Linux has no prompt (always granted).
