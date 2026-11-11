//! Read text aloud with the OS speech engine (no extra dependencies, works offline).
//! Each OS has its own module so a fix for one cannot break the other.

use std::process::Child;
use std::sync::Mutex;

static CURRENT: Mutex<Option<Child>> = Mutex::new(None);

/// Language of the text by script, so the OS picks a matching voice.
fn detect_lang(text: &str) -> &'static str {
    const VI: &str = "ăâđêôơưĂÂĐÊÔƠƯạảãáàậẩẫấầặẳẵắằẹẻẽéèệểễếềịỉĩíìọỏõóòộổỗốồợởỡớờụủũúùựửữứừỵỷỹýỳ";
    let mut vi = 0;
    for c in text.chars() {
        match c {
            '\u{3040}'..='\u{30ff}' => return "ja",
            '\u{ac00}'..='\u{d7af}' => return "ko",
            '\u{4e00}'..='\u{9fff}' => return "zh",
            c if VI.contains(c) || VI.contains(c.to_ascii_lowercase()) => vi += 1,
            _ => {}
        }
    }
    if vi > 0 {
        "vi"
    } else {
        "en"
    }
}

/// True while the engine is still talking.
pub fn is_speaking() -> bool {
    let mut guard = CURRENT.lock().unwrap();
    match guard.as_mut() {
        Some(child) => match child.try_wait() {
            Ok(None) => true,
            _ => {
                *guard = None;
                false
            }
        },
        None => false,
    }
}

pub fn stop() {
    if let Some(mut child) = CURRENT.lock().unwrap().take() {
        let _ = child.kill();
        let _ = child.wait();
    }
}

/// Start speaking `text`, replacing anything currently being read.
pub fn speak(text: &str) {
    stop();
    let text = text.trim();
    if text.is_empty() {
        return;
    }
    if let Some(child) = imp::spawn(text, detect_lang(text)) {
        *CURRENT.lock().unwrap() = Some(child);
    }
}

/// Click once to start, again to stop.
pub fn toggle(text: &str) {
    if is_speaking() {
        stop();
    } else {
        speak(text);
    }
}

#[cfg(target_os = "macos")]
mod imp {
    use std::process::{Child, Command, Stdio};

    pub fn spawn(text: &str, lang: &str) -> Option<Child> {
        let voice = match lang {
            "vi" => Some("Linh"),
            "ja" => Some("Kyoko"),
            "ko" => Some("Yuna"),
            "zh" => Some("Tingting"),
            _ => None,
        };
        let run = |voice: Option<&str>| {
            let mut cmd = Command::new("say");
            if let Some(v) = voice {
                cmd.args(["-v", v]);
            }
            // the audio device needs a moment to wake up and would swallow the first word,
            // so start with a short silence (Bluetooth speakers need most of a second)
            cmd.arg("--").arg(format!("[[slnc 800]] {text}")).stdout(Stdio::null()).stderr(Stdio::null());
            cmd
        };
        // `say -v` exits at once if the voice isn't installed: fall back to the default voice.
        let mut first = run(voice).spawn().ok()?;
        if voice.is_some() {
            std::thread::sleep(std::time::Duration::from_millis(150));
            if let Ok(Some(status)) = first.try_wait() {
                if !status.success() {
                    return run(None).spawn().ok();
                }
            }
        }
        Some(first)
    }
}

#[cfg(target_os = "linux")]
mod imp {
    use std::process::{Child, Command, Stdio};

    pub fn spawn(text: &str, lang: &str) -> Option<Child> {
        let quiet = |mut c: Command| {
            c.stdout(Stdio::null()).stderr(Stdio::null());
            c
        };
        // espeak-ng covers vi/ja/ko/zh/en; spd-say is the fallback
        for bin in ["espeak-ng", "espeak"] {
            if let Ok(child) = quiet(Command::new(bin)).args(["-v", lang, "--", text]).spawn() {
                return Some(child);
            }
        }
        quiet(Command::new("spd-say")).args(["-w", "-l", lang, "--", text]).spawn().ok()
    }
}

/// Windows: not supported yet.
#[cfg(not(any(target_os = "linux", target_os = "macos")))]
mod imp {
    use std::process::Child;

    pub fn spawn(_text: &str, _lang: &str) -> Option<Child> {
        None
    }
}
