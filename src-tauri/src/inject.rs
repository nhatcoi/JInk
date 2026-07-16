// Text injection into the previously-focused app.
//
// Linux: type the characters directly with `xdotool type`. This is app-agnostic
// — GUI apps, terminals, and terminals embedded in IDEs all receive the same
// keystrokes, so we avoid the Ctrl+V vs Ctrl+Shift+V ambiguity (an embedded
// terminal reports its host window's class, so paste-shortcut detection can't
// tell it apart from a plain GUI app).
//
// macOS / Windows: clipboard + simulated paste shortcut, restoring the clipboard.
use std::{thread, time::Duration};

#[cfg(target_os = "linux")]
pub fn inject_text(text: &str) -> Result<(), String> {
    // Give focus a moment to return to the target app after the popup hides.
    thread::sleep(Duration::from_millis(120));

    // `--` stops flag parsing so text starting with '-' is safe; the text is a
    // single process arg, so there's no shell interpolation.
    let status = std::process::Command::new("xdotool")
        .args(["type", "--clearmodifiers", "--delay", "4", "--", text])
        .status()
        .map_err(|e| format!("xdotool not available: {e}"))?;
    if !status.success() {
        return Err("xdotool type failed".into());
    }
    Ok(())
}

/// Copy `text` to the clipboard, then simulate the platform paste shortcut so it
/// lands in whatever app currently has focus. Restores the previous clipboard.
#[cfg(not(target_os = "linux"))]
pub fn inject_text(text: &str) -> Result<(), String> {
    // Save old clipboard so we don't clobber the user's content.
    let mut clipboard = arboard::Clipboard::new().map_err(|e| e.to_string())?;
    let previous = clipboard.get_text().ok();

    clipboard
        .set_text(text.to_string())
        .map_err(|e| e.to_string())?;

    // Give focus a moment to return to the target app after the popup hides.
    thread::sleep(Duration::from_millis(120));

    paste()?;

    // Restore previous clipboard after the paste has been consumed.
    thread::sleep(Duration::from_millis(150));
    if let Some(prev) = previous {
        let _ = clipboard.set_text(prev);
    }
    Ok(())
}

/// macOS / Windows: simulate the paste shortcut with enigo.
#[cfg(not(target_os = "linux"))]
fn paste() -> Result<(), String> {
    use enigo::{
        Direction::{Click, Press, Release},
        Enigo, Key, Keyboard, Settings as EnigoSettings,
    };

    let mut enigo = Enigo::new(&EnigoSettings::default()).map_err(|e| e.to_string())?;

    #[cfg(target_os = "macos")]
    let modifier = Key::Meta;
    #[cfg(not(target_os = "macos"))]
    let modifier = Key::Control;

    enigo.key(modifier, Press).map_err(|e| e.to_string())?;
    enigo
        .key(Key::Unicode('v'), Click)
        .map_err(|e| e.to_string())?;
    enigo.key(modifier, Release).map_err(|e| e.to_string())?;
    Ok(())
}
