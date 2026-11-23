<div align="center">

<img src="assets/icons/128x128@2x.png" width="96" alt="JInk logo" />

# JInk

**A floating quick-input popup for every text field on your machine.**

Press a hotkey anywhere → type, translate, AI-polish or dictate → hit Enter →
the text lands in whatever app had focus.

[![Release](https://img.shields.io/github/v/release/nhatcoi/JInk?style=flat-square)](https://github.com/nhatcoi/JInk/releases/latest)
[![Downloads](https://img.shields.io/github/downloads/nhatcoi/JInk/total?style=flat-square)](https://github.com/nhatcoi/JInk/releases)
[![License](https://img.shields.io/badge/license-MIT-blue?style=flat-square)](LICENSE)

</div>

---

## Download

Get the latest build from the [**Releases page**](https://github.com/nhatcoi/JInk/releases/latest).

| Platform | File |
| --- | --- |
| **macOS** (Apple Silicon + Intel) | `.dmg` |
| **Linux** (Debian/Ubuntu) | `.deb` — `sudo dpkg -i JInk_*.deb` |
| **Linux** (any distro) | `.AppImage` — `chmod +x`, then run |

- **macOS:** builds are unsigned — right-click → Open on first launch, then
  allow JInk under *Privacy & Security → Accessibility*.
- **Linux:** needs `xdotool` for pasting (`sudo apt install xdotool`). X11 is
  the main target; Wayland is best-effort.

## Usage

Press **Control+Space** (macOS) or **Super+Space** (Linux) to open the popup.

| Key | Action |
| --- | --- |
| `Enter` | Insert into the focused app (`Shift+Enter` = newline) |
| `Cmd/Ctrl+R` | Refine — fix grammar and clarity |
| `Cmd/Ctrl+E` | Enhance — turn a rough idea into a clear prompt |
| `Cmd/Ctrl+T` | Translate between your two languages |
| `Cmd/Ctrl+Enter` | Explain |
| `Cmd/Ctrl+M` | Voice to text (stops on silence) |
| `Cmd/Ctrl+L` | Read aloud |
| `Cmd/Ctrl+O` | Attach files or images |
| `Cmd/Ctrl+Z` | Undo the last AI change |

With an empty input, AI actions work on the text you had selected.

## Settings

Open from the gear icon or the menu bar icon:

- AI endpoint, model and key — any OpenAI-compatible API (OpenAI, Ollama, OpenRouter, …)
- Hotkey, your two main languages, UI language (English / Tiếng Việt)
- Auto-send, launch at login, selection as context

## License

[MIT](LICENSE) © nhatcoi
