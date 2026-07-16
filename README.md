# easyinput

A fast, cross-platform (macOS + Linux) quick-input popup. Press a global hotkey
(**Ctrl+Alt+Space** by default), a small floating editor appears at your cursor, you
type / translate / AI-polish / dictate, hit **Insert**, and the text is pasted
into whatever app had focus — Claude, a terminal, an editor, any text field.

## Features

- **Global hotkey** popup at the cursor (toggle show/hide).
- **Text editor** with newline support (Enter to insert, Shift+Enter for newline — configurable).
- **AI enhance** — fix grammar / clarity via any OpenAI-compatible endpoint (streaming).
- **Translate** VI⇄EN with a one-click direction swap.
- **Voice to text** — record mic, transcribe via `/audio/transcriptions`.
- **Attachments** — paste or pick images / files (shown as chips).
- **Undo** the last AI rewrite.
- **System tray** (Open / Settings / Quit), configurable hotkey & popup size.

## Stack

- **Tauri 2** (Rust core) + **React 19 + Vite + TypeScript**
- **Tailwind v4** + shadcn-style UI, **lucide** icons
- Rust: `enigo` (paste injection), `arboard` (clipboard), `reqwest` (AI stream),
  `tauri-plugin-global-shortcut`, `tauri-plugin-store`

## Prerequisites (Linux)

```bash
sudo apt install -y libwebkit2gtk-4.1-dev \
  libayatana-appindicator3-dev librsvg2-dev xdotool
```

`xdotool` is used at runtime to paste into the focused app (avoids the
`libxdo-dev` build dependency of enigo, which is only used on macOS/Windows).
Node 20+, pnpm, and a Rust toolchain are also required.

## Develop

```bash
pnpm install
pnpm tauri dev
```

## Build

```bash
pnpm tauri build
```

## Configure

Open **Settings** from the popup's gear icon or the tray. Set the AI base URL,
model, API key, global hotkey, submit key, popup size, and default translate
direction. Settings persist to `settings.json` via the store plugin.

## Notes / roadmap

- Popup positions at the **mouse cursor**. True text-caret positioning (via
  macOS Accessibility API / Linux AT-SPI) is a future enhancement.
- **X11** is the primary Linux target; Wayland restricts global hotkeys, cursor
  position and input injection — expect best-effort behaviour there.
- Image/file attachments are currently display-only; pasting image bytes into
  image-capable targets is planned.
- Ideas: snippet/template library, input history recall, per-app profiles,
  local Whisper (offline STT), local LLM via Ollama.
