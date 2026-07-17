<div align="center">

<img src="src-tauri/icons/128x128@2x.png" width="96" alt="easyinput logo" />

# easyinput

**A floating quick-input popup for every text field on your machine.**

Press a hotkey anywhere → a small editor appears at your cursor → type, translate,
AI-polish or dictate → hit Insert → the text lands in whatever app had focus.

[![Release](https://img.shields.io/github/v/release/nhatcoi/JInk?style=flat-square)](https://github.com/nhatcoi/JInk/releases/latest)
[![Downloads](https://img.shields.io/github/downloads/nhatcoi/JInk/total?style=flat-square)](https://github.com/nhatcoi/JInk/releases)
[![License](https://img.shields.io/badge/license-MIT-blue?style=flat-square)](LICENSE)
[![Built with Tauri](https://img.shields.io/badge/built%20with-Tauri%202-24C8DB?style=flat-square&logo=tauri&logoColor=white)](https://tauri.app)

</div>

---

## How it works

```
┌──────────────────────────────────────────────────────────┐
│  Ctrl+Alt+Space  ──►  popup appears at your cursor        │
│                                                           │
│   ┌─────────────────────────────────────────────────┐     │
│   │  toi muon viet mail cho khach hang              │     │
│   │                                                 │     │
│   │  [ Enhance ] [ VI⇄EN ] [ Voice ] [ Settings ]   │     │
│   └─────────────────────────────────────────────────┘     │
│                                                           │
│  Enter  ──►  text pasted into Claude / terminal / editor  │
└───────────────────────────────────────────────────────────┘
```

## Download

Grab the latest build from the [**Releases page**](https://github.com/nhatcoi/JInk/releases/latest).

| Platform | File | Install |
| --- | --- | --- |
| **Windows** | `easyinput_x.y.z_x64-setup.exe` | Run the NSIS installer |
| **macOS** | `easyinput_x.y.z_universal.dmg` | Apple Silicon + Intel |
| **Linux (Debian/Ubuntu)** | `easyinput_x.y.z_amd64.deb` | `sudo dpkg -i easyinput_*.deb` |
| **Linux (Fedora/RHEL)** | `easyinput-x.y.z-1.x86_64.rpm` | `sudo rpm -i easyinput-*.rpm` |
| **Linux (any distro)** | `easyinput_x.y.z_amd64.AppImage` | `chmod +x` then run |

> Linux needs `xdotool` at runtime for paste injection:
> `sudo apt install xdotool` / `sudo dnf install xdotool`

Builds are unsigned. On macOS, right-click → Open on first launch. On Windows,
SmartScreen → More info → Run anyway.

## Features

| | |
| --- | --- |
| **Global hotkey** | Popup at the cursor, toggle show/hide. Rebindable. |
| **AI enhance** | Fix grammar / clarity via any OpenAI-compatible endpoint, streamed. |
| **Translate** | VI⇄EN with a one-click direction swap. |
| **Voice to text** | Record mic, transcribe via `/audio/transcriptions`. |
| **Attachments** | Paste or pick images / files (shown as chips). |
| **Undo** | Revert the last AI rewrite. |
| **System tray** | Open / Settings / Quit. Configurable hotkey & popup size. |
| **Editor** | Enter to insert, Shift+Enter for newline (configurable). |

## Configure

Open **Settings** from the popup's gear icon or the tray:

- AI base URL, model, API key — any OpenAI-compatible endpoint (OpenAI, Ollama, LM Studio, …)
- Global hotkey, submit key, popup size
- Default translate direction

Settings persist to `settings.json` via the Tauri store plugin.

## Stack

- **Tauri 2** (Rust core) + **React 19 + Vite + TypeScript**
- **Tailwind v4** + shadcn-style UI, **lucide** icons
- Rust: `enigo` (paste injection on macOS/Windows), `arboard` (clipboard),
  `reqwest` (AI stream), `tauri-plugin-global-shortcut`, `tauri-plugin-store`

## Build from source

Prerequisites: Node 20+, pnpm, Rust toolchain. On Linux also:

```bash
sudo apt install -y libwebkit2gtk-4.1-dev \
  libayatana-appindicator3-dev librsvg2-dev xdotool
```

`xdotool` is used at runtime to paste into the focused app, which avoids enigo's
`libxdo-dev` build dependency (enigo is only compiled on macOS/Windows).

```bash
pnpm install
pnpm tauri dev      # develop
pnpm tauri build    # installers land in src-tauri/target/release/bundle
```

## Notes / roadmap

- Popup positions at the **mouse cursor**. True text-caret positioning (via
  macOS Accessibility API / Linux AT-SPI) is a future enhancement.
- **X11** is the primary Linux target; Wayland restricts global hotkeys, cursor
  position and input injection — expect best-effort behaviour there.
- Image/file attachments are currently display-only; pasting image bytes into
  image-capable targets is planned.
- Ideas: snippet/template library, input history recall, per-app profiles,
  local Whisper (offline STT), local LLM via Ollama.

## License

[MIT](LICENSE) © nhatcoi
