//! Read text aloud: OS speech engine (macOS `say`, Windows none), Linux uses Edge neural voices with espeak as offline fallback.
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
    use msedge_tts::tts::{client::connect, SpeechConfig};
    use std::io::Write;
    use std::process::{Child, Command, Stdio};

    const PLAYERS: [(&str, &[&str]); 3] =
        [("paplay", &[]), ("pw-play", &["-"]), ("aplay", &["-q"])];

    fn edge_voice(lang: &str) -> &'static str {
        match lang {
            "vi" => "vi-VN-HoaiMyNeural",
            "ja" => "ja-JP-NanamiNeural",
            "ko" => "ko-KR-SunHiNeural",
            "zh" => "zh-CN-XiaoxiaoNeural",
            _ => "en-US-AriaNeural",
        }
    }

    fn synthesize(text: &str, lang: &str) -> anyhow::Result<Vec<u8>> {
        let config = SpeechConfig {
            voice_name: edge_voice(lang).to_string(),
            audio_format: "audio-24khz-48kbitrate-mono-mp3".to_string(),
            pitch: 0,
            rate: 0,
            volume: 0,
        };
        // Edge resets the first connection now and then; a second try goes through
        let audio = connect()?.synthesize(text, &config).or_else(|_| connect()?.synthesize(text, &config))?;
        mp3_to_wav(audio.audio_bytes)
    }

    // Edge only serves mp3; players like paplay/aplay need WAV, so decode in-process.
    fn mp3_to_wav(mp3: Vec<u8>) -> anyhow::Result<Vec<u8>> {
        let mut decoder = minimp3::Decoder::new(std::io::Cursor::new(mp3));
        let (mut pcm, mut rate, mut channels) = (Vec::new(), 24000u32, 1u16);
        loop {
            match decoder.next_frame() {
                Ok(frame) => {
                    rate = frame.sample_rate as u32;
                    channels = frame.channels as u16;
                    pcm.extend(frame.data.iter().flat_map(|s| s.to_le_bytes()));
                }
                Err(minimp3::Error::Eof) => break,
                Err(err) => return Err(anyhow::anyhow!("mp3 decode: {err}")),
            }
        }
        let data_len = pcm.len() as u32;
        let mut wav = Vec::with_capacity(pcm.len() + 44);
        wav.extend(b"RIFF");
        wav.extend((36 + data_len).to_le_bytes());
        wav.extend(b"WAVEfmt ");
        wav.extend(16u32.to_le_bytes());
        wav.extend(1u16.to_le_bytes());
        wav.extend(channels.to_le_bytes());
        wav.extend(rate.to_le_bytes());
        wav.extend((rate * channels as u32 * 2).to_le_bytes());
        wav.extend((channels * 2).to_le_bytes());
        wav.extend(16u16.to_le_bytes());
        wav.extend(b"data");
        wav.extend(data_len.to_le_bytes());
        wav.extend(pcm);
        Ok(wav)
    }

    fn espeak(text: &str, lang: &str) -> Option<Child> {
        let quiet = |mut c: Command| {
            c.stdout(Stdio::null()).stderr(Stdio::null());
            c
        };
        for bin in ["espeak-ng", "espeak"] {
            if let Ok(child) = quiet(Command::new(bin)).args(["-v", lang, "--", text]).spawn() {
                return Some(child);
            }
        }
        quiet(Command::new("spd-say")).args(["-w", "-l", lang, "--", text]).spawn().ok()
    }

    fn spawn_player() -> Option<Child> {
        PLAYERS.iter().find_map(|(bin, args)| {
            Command::new(bin)
                .args(*args)
                .stdin(Stdio::piped())
                .stdout(Stdio::null())
                .stderr(Stdio::null())
                .spawn()
                .ok()
        })
    }

    // Edge neural voice streamed into a WAV player; the player child stays the "speaking" handle
    // so stop() still kills it. Offline or no player: espeak replaces it.
    pub fn spawn(text: &str, lang: &str) -> Option<Child> {
        let Some(mut player) = spawn_player() else {
            return espeak(text, lang);
        };
        let mut stdin = player.stdin.take()?;
        let (text, lang, pid) = (text.to_string(), lang.to_string(), player.id());
        std::thread::spawn(move || match synthesize(&text, &lang) {
            Ok(wav) => {
                let _ = stdin.write_all(&wav);
            }
            Err(err) => {
                eprintln!("[jink] edge tts failed: {err}");
                drop(stdin);
                let mut current = super::CURRENT.lock().unwrap();
                if current.as_ref().is_some_and(|c| c.id() == pid) {
                    if let Some(mut old) = current.take() {
                        let _ = old.kill();
                        let _ = old.wait();
                    }
                    *current = espeak(&text, &lang);
                }
            }
        });
        Some(player)
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
