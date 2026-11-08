# JInk — logic notes for agents

100% Pure Native Rust + GPUI quick-input popup. Global hotkey → popup at cursor → type/AI-enhance/translate → inject into whatever app had focus.

## Update rule

**Whenever you add, change, or remove app logic (not pure styling), update the relevant section below in the same commit/PR.** Keep entries short — what it does + why, not full code walkthrough. If a section grows stale (doesn't match code), fix it before adding new ones.

---

## Architecture (GPUI Native Rust)

- UI framework: **GPUI 0.2.2** (GPU-accelerated via Metal on macOS, Vulkan on Linux).
- Footprint: ~20MB RAM idle, 4.3MB release binary. Zero Webview runtime overhead.
- Styling: Catppuccin Mocha theme defined in `src/theme.rs`.

## Text injection (`src/inject.rs`)

- Text is split on `[#ImageN]` tokens; each run (text or image) is put on the clipboard (`arboard`) and pasted.
- **Linux**: paste via `xdotool key --clearmodifiers`. Terminal detection checks `WM_CLASS` to use `ctrl+shift+v` for terminals and `ctrl+v` elsewhere.
- **macOS/Windows**: clipboard runs + simulated paste (`enigo`: Cmd+V on macOS, Ctrl+V on Windows).
- On inject, popup closes immediately, giving focus back to the target app before injecting.

## Popup show/hide & focus (`src/main.rs`)

- Global hotkey (default `Alt+Space`) toggles popup visibility.
- Hotkey loop uses non-blocking polling (`receiver.try_recv()` + async 30ms timer on `BackgroundExecutor`) to prevent main thread blocking.
- `cursor_position`: uses CoreGraphics (`CGEventGetLocation`) on macOS and `xdotool getmouselocation` on Linux to place popup 12px right / 16px below cursor.
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
