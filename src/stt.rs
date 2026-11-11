//! Speech-to-text with the OS recognizer, one implementation per OS.
//! macOS: `SFSpeechRecognizer` (built in, no API needed). Other OSes have no built-in
//! recognizer, so `native_available()` is false and the caller falls back to the AI API.

/// Locale id for a language code, as the OS recognizers expect.
#[allow(dead_code)]
fn locale_id(code: &str) -> &'static str {
    match code {
        "vi" => "vi-VN",
        "ja" => "ja-JP",
        "ko" => "ko-KR",
        "zh" => "zh-CN",
        "fr" => "fr-FR",
        "de" => "de-DE",
        "es" => "es-ES",
        _ => "en-US",
    }
}

/// Whether the OS recognizer can be used right now.
pub fn native_available() -> bool {
    imp::native_available()
}

/// Recognize `wav` in each of `langs` and return the most confident transcript (blocking).
pub fn transcribe_native(wav: &[u8], langs: &[String]) -> Result<String, String> {
    imp::transcribe_native(wav, langs)
}

#[cfg(target_os = "macos")]
mod imp {
    #![allow(unexpected_cfgs)]
    use super::locale_id;
    use block::ConcreteBlock;
    use objc::rc::autoreleasepool;
    use objc::runtime::{Object, BOOL, NO, YES};
    use objc::{class, msg_send, sel, sel_impl};
    use std::ffi::{CStr, CString};
    use std::os::raw::c_char;
    use std::sync::atomic::{AtomicU64, Ordering};
    use std::sync::mpsc;
    use std::time::Duration;

    #[link(name = "Speech", kind = "framework")]
    extern "C" {}

    type Id = *mut Object;

    static FILE_SEQ: AtomicU64 = AtomicU64::new(1);

    unsafe fn nsstring(s: &str) -> Id {
        let c = CString::new(s).unwrap_or_default();
        msg_send![class!(NSString), stringWithUTF8String: c.as_ptr()]
    }

    unsafe fn rust_string(ns: Id) -> String {
        if ns.is_null() {
            return String::new();
        }
        let p: *const c_char = msg_send![ns, UTF8String];
        if p.is_null() {
            return String::new();
        }
        CStr::from_ptr(p).to_string_lossy().into_owned()
    }

    /// Asking for speech permission aborts a process without a usage description (a bare
    /// `cargo run` binary), so only the bundled app may use the native recognizer.
    pub fn native_available() -> bool {
        autoreleasepool(|| unsafe {
            let bundle: Id = msg_send![class!(NSBundle), mainBundle];
            let key = nsstring("NSSpeechRecognitionUsageDescription");
            let v: Id = msg_send![bundle, objectForInfoDictionaryKey: key];
            !v.is_null()
        })
    }

    fn open_settings() {
        let _ = std::process::Command::new("open")
            .arg("x-apple.systempreferences:com.apple.preference.security?Privacy_SpeechRecognition")
            .spawn();
    }

    fn ensure_authorized() -> Result<(), String> {
        autoreleasepool(|| unsafe {
            let status: i64 = msg_send![class!(SFSpeechRecognizer), authorizationStatus];
            match status {
                3 => return Ok(()),
                1 | 2 => {
                    open_settings();
                    return Err("speech recognition is blocked".into());
                }
                _ => {}
            }
            let (tx, rx) = mpsc::channel::<i64>();
            let block = ConcreteBlock::new(move |s: i64| {
                let _ = tx.send(s);
            })
            .copy();
            let _: () = msg_send![class!(SFSpeechRecognizer), requestAuthorization: &*block];
            match rx.recv_timeout(Duration::from_secs(60)) {
                Ok(3) => Ok(()),
                _ => {
                    open_settings();
                    Err("speech recognition is blocked".into())
                }
            }
        })
    }

    /// Recognize `path` for one locale, returning `(transcript, average_confidence)`.
    fn recognize(path: &str, locale: &str) -> Option<(String, f32)> {
        autoreleasepool(|| unsafe {
            let loc: Id = msg_send![class!(NSLocale), alloc];
            let loc: Id = msg_send![loc, initWithLocaleIdentifier: nsstring(locale)];
            if loc.is_null() {
                return None;
            }
            let rec: Id = msg_send![class!(SFSpeechRecognizer), alloc];
            let rec: Id = msg_send![rec, initWithLocale: loc];
            let _: () = msg_send![loc, release];
            if rec.is_null() {
                return None;
            }
            let available: BOOL = msg_send![rec, isAvailable];
            if available == NO {
                let _: () = msg_send![rec, release];
                return None;
            }
            let url: Id = msg_send![class!(NSURL), fileURLWithPath: nsstring(path)];
            let req: Id = msg_send![class!(SFSpeechURLRecognitionRequest), alloc];
            let req: Id = msg_send![req, initWithURL: url];
            let _: () = msg_send![req, setShouldReportPartialResults: NO];

            let (tx, rx) = mpsc::channel::<Option<(String, f32)>>();
            let block = ConcreteBlock::new(move |result: Id, error: Id| {
                if !error.is_null() {
                    let _ = tx.send(None);
                    return;
                }
                if result.is_null() {
                    return;
                }
                let is_final: BOOL = msg_send![result, isFinal];
                if is_final != YES {
                    return;
                }
                let best: Id = msg_send![result, bestTranscription];
                let text = rust_string(msg_send![best, formattedString]);
                let segs: Id = msg_send![best, segments];
                let count: usize = msg_send![segs, count];
                let mut sum = 0.0f32;
                for i in 0..count {
                    let seg: Id = msg_send![segs, objectAtIndex: i];
                    let c: f32 = msg_send![seg, confidence];
                    sum += c;
                }
                let avg = if count > 0 { sum / count as f32 } else { 0.0 };
                let _ = tx.send(Some((text, avg)));
            })
            .copy();

            let task: Id = msg_send![rec, recognitionTaskWithRequest: req resultHandler: &*block];
            let out = rx.recv_timeout(Duration::from_secs(15)).ok().flatten();
            if !task.is_null() {
                let _: () = msg_send![task, cancel];
            }
            let _: () = msg_send![req, release];
            let _: () = msg_send![rec, release];
            out
        })
    }

    pub fn transcribe_native(wav: &[u8], langs: &[String]) -> Result<String, String> {
        ensure_authorized()?;
        let seq = FILE_SEQ.fetch_add(1, Ordering::Relaxed);
        let path = std::env::temp_dir().join(format!("jink-stt-{}-{seq}.wav", std::process::id()));
        std::fs::write(&path, wav).map_err(|e| e.to_string())?;
        let path_str = path.to_string_lossy().to_string();

        // Deduplicate locales and run them in parallel so bilingual EN/VI latency isn't doubled.
        let mut locales: Vec<&'static str> = Vec::new();
        for l in langs {
            let loc = locale_id(l);
            if !locales.contains(&loc) {
                locales.push(loc);
            }
        }

        let handles: Vec<_> = locales
            .into_iter()
            .map(|loc| {
                let p = path_str.clone();
                std::thread::spawn(move || recognize(&p, loc))
            })
            .collect();

        let mut best: Option<(String, f32)> = None;
        for h in handles {
            if let Ok(Some((text, conf))) = h.join() {
                if text.trim().is_empty() {
                    continue;
                }
                if best.as_ref().is_none_or(|(_, c)| conf > *c) {
                    best = Some((text, conf));
                }
            }
        }
        let _ = std::fs::remove_file(&path);
        best.map(|(t, _)| t.trim().to_string()).ok_or_else(|| "silent".to_string())
    }
}

/// No built-in speech recognizer on Linux/Windows: callers use the AI API instead.
#[cfg(not(target_os = "macos"))]
mod imp {
    pub fn native_available() -> bool {
        false
    }

    pub fn transcribe_native(_wav: &[u8], _langs: &[String]) -> Result<String, String> {
        Err("no native recognizer on this OS".into())
    }
}
