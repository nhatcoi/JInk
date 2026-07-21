// Text + inline-image injection into the previously-focused app.
//
// Text is split on `[#ImageN]` tokens. Literal text is typed; each token is
// replaced by pasting that image from the clipboard, so an app like Claude Code
// turns it into its own inline `[Image #N]` placeholder at the right position.
//
// Linux: type via `xdotool type`, paste images via clipboard + `xdotool key
// ctrl+v`. macOS / Windows: clipboard + enigo paste.
use std::{borrow::Cow, thread, time::Duration};

enum Part {
    Text(String),
    Image(u32),
}

/// Split text into literal runs and `[#ImageN]` image tokens, in order.
fn split_parts(text: &str) -> Vec<Part> {
    let mut parts = Vec::new();
    let mut buf = String::new();
    let mut i = 0;
    while i < text.len() {
        if text[i..].starts_with("[#Image") {
            let rest = &text[i + 7..];
            let digits: String = rest.chars().take_while(|c| c.is_ascii_digit()).collect();
            if !digits.is_empty() && rest[digits.len()..].starts_with(']') {
                if !buf.is_empty() {
                    parts.push(Part::Text(std::mem::take(&mut buf)));
                }
                parts.push(Part::Image(digits.parse().unwrap()));
                i += 7 + digits.len() + 1;
                continue;
            }
        }
        let ch = text[i..].chars().next().unwrap();
        buf.push(ch);
        i += ch.len_utf8();
    }
    if !buf.is_empty() {
        parts.push(Part::Text(buf));
    }
    parts
}

/// Decode an image file and place it on the clipboard as raw RGBA.
fn set_clipboard_image(cb: &mut arboard::Clipboard, path: &str) -> Result<(), String> {
    let img = image::open(path)
        .map_err(|e| format!("decode image: {e}"))?
        .to_rgba8();
    let (w, h) = img.dimensions();
    cb.set_image(arboard::ImageData {
        width: w as usize,
        height: h as usize,
        bytes: Cow::Owned(img.into_raw()),
    })
    .map_err(|e| format!("clipboard image: {e}"))
}

#[cfg(target_os = "linux")]
pub fn inject_text(text: &str, images: &[(u32, String)]) -> Result<(), String> {
    // Give focus a moment to return to the target app after the popup hides.
    thread::sleep(Duration::from_millis(120));

    let mut clipboard: Option<arboard::Clipboard> = None;
    for part in split_parts(text) {
        match part {
            Part::Text(s) => type_text(&s)?,
            Part::Image(n) => {
                let Some((_, path)) = images.iter().find(|(i, _)| *i == n) else {
                    // No image for this token — keep the marker so nothing's lost.
                    type_text(&format!("[#Image{n}]"))?;
                    continue;
                };
                let cb = clipboard
                    .get_or_insert_with(|| arboard::Clipboard::new().expect("clipboard init"));
                set_clipboard_image(cb, path)?;
                paste()?;
                // Let the target consume the paste before the next keystroke.
                thread::sleep(Duration::from_millis(250));
            }
        }
    }
    Ok(())
}

#[cfg(target_os = "linux")]
fn type_text(s: &str) -> Result<(), String> {
    if s.is_empty() {
        return Ok(());
    }
    // `--` stops flag parsing so text starting with '-' is safe; the text is a
    // single process arg, so there's no shell interpolation.
    let status = std::process::Command::new("xdotool")
        .args(["type", "--clearmodifiers", "--delay", "4", "--", s])
        .status()
        .map_err(|e| format!("xdotool not available: {e}"))?;
    if !status.success() {
        return Err("xdotool type failed".into());
    }
    Ok(())
}

#[cfg(target_os = "linux")]
fn paste() -> Result<(), String> {
    let status = std::process::Command::new("xdotool")
        .args(["key", "--clearmodifiers", "ctrl+v"])
        .status()
        .map_err(|e| format!("xdotool not available: {e}"))?;
    if !status.success() {
        return Err("xdotool key failed".into());
    }
    Ok(())
}

/// macOS / Windows: paste each run via the clipboard. Text and images alike go
/// through clipboard + a simulated paste shortcut.
#[cfg(not(target_os = "linux"))]
pub fn inject_text(text: &str, images: &[(u32, String)]) -> Result<(), String> {
    let mut clipboard = arboard::Clipboard::new().map_err(|e| e.to_string())?;
    thread::sleep(Duration::from_millis(120));

    for part in split_parts(text) {
        match part {
            Part::Text(s) => {
                if s.is_empty() {
                    continue;
                }
                clipboard.set_text(s).map_err(|e| e.to_string())?;
            }
            Part::Image(n) => {
                let Some((_, path)) = images.iter().find(|(i, _)| *i == n) else {
                    clipboard
                        .set_text(format!("[#Image{n}]"))
                        .map_err(|e| e.to_string())?;
                    paste()?;
                    thread::sleep(Duration::from_millis(150));
                    continue;
                };
                set_clipboard_image(&mut clipboard, path)?;
            }
        }
        paste()?;
        thread::sleep(Duration::from_millis(200));
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
