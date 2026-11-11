//! Text currently selected in the previously focused app. Each OS gets its own
//! implementation (no shared branches), so a fix for one cannot break the other.

/// Whether this OS can read the selection of another app (drives the Settings toggles).
pub const SUPPORTED: bool = cfg!(any(target_os = "linux", target_os = "macos"));

/// Result of the instant selection lookup done before the popup shows.
pub enum Fast {
    /// Answered right away (`None` = nothing selected).
    Done(Option<String>),
    /// The focused app doesn't expose its selection: call `slow()` off the main thread
    /// while the target app still has focus.
    Slow,
}

/// Instant lookup (a few ms), safe to run on the main thread before opening the popup.
pub fn fast() -> Fast {
    imp::fast()
}

/// Slow fallback (simulated copy, up to ~800ms). Run on a background thread, and only
/// while the previously focused app still has keyboard focus.
pub fn slow() -> Option<String> {
    imp::slow()
}

fn clean(text: String) -> Option<String> {
    let text = text.trim_end().to_string();
    (!text.is_empty()).then_some(text)
}

/// Linux: the PRIMARY selection is the highlighted text; no key simulation needed.
#[cfg(target_os = "linux")]
mod imp {
    use arboard::{GetExtLinux, LinuxClipboardKind};

    pub fn fast() -> super::Fast {
        super::Fast::Done(primary())
    }

    pub fn slow() -> Option<String> {
        None
    }

    fn primary() -> Option<String> {
        let text = arboard::Clipboard::new()
            .ok()?
            .get()
            .clipboard(LinuxClipboardKind::Primary)
            .text()
            .ok()?;
        super::clean(text)
    }
}

/// macOS has no PRIMARY selection: simulate Cmd+C in the focused app, read the clipboard,
/// then put the previous clipboard back.
#[cfg(target_os = "macos")]
mod imp {
    use std::{thread::sleep, time::{Duration, Instant, SystemTime, UNIX_EPOCH}};

    #[link(name = "CoreGraphics", kind = "framework")]
    extern "C" {
        fn CGEventSourceFlagsState(state_id: i32) -> u64;
    }

    // ctrl | alt | shift | cmd
    const MOD_MASK: u64 = 0x0004_0000 | 0x0008_0000 | 0x0002_0000 | 0x0010_0000;

    /// The hotkey's own modifiers must be released first, or Cmd+C turns into Ctrl+Cmd+C.
    fn wait_modifiers_released() {
        let start = Instant::now();
        while start.elapsed() < Duration::from_millis(500)
            && unsafe { CGEventSourceFlagsState(1) } & MOD_MASK != 0
        {
            sleep(Duration::from_millis(10));
        }
    }

    /// Fast path: ask the focused element for its selection over Accessibility (a few ms,
    /// no key simulation, no waiting for the hotkey's modifiers). An empty answer is only
    /// trusted from a real text control; web pages often report "" while text is
    /// highlighted, so anything uncertain falls back to Cmd+C (run in the background).
    mod ax {
        #![allow(unexpected_cfgs)]
        use objc::runtime::Object;
        use objc::{class, msg_send, sel, sel_impl};
        use std::ffi::{c_void, CStr, CString};
        use std::os::raw::c_char;

        type CFTypeRef = *const c_void;

        #[link(name = "ApplicationServices", kind = "framework")]
        extern "C" {
            fn AXUIElementCreateSystemWide() -> CFTypeRef;
            fn AXUIElementCopyAttributeValue(el: CFTypeRef, attr: CFTypeRef, out: *mut CFTypeRef) -> i32;
            fn AXUIElementSetMessagingTimeout(el: CFTypeRef, seconds: f32) -> i32;
            fn AXUIElementCopyParameterizedAttributeValue(
                el: CFTypeRef,
                attr: CFTypeRef,
                param: CFTypeRef,
                out: *mut CFTypeRef,
            ) -> i32;
        }
        #[link(name = "CoreFoundation", kind = "framework")]
        extern "C" {
            fn CFRelease(cf: CFTypeRef);
            fn CFGetTypeID(cf: CFTypeRef) -> usize;
            fn CFStringGetTypeID() -> usize;
        }

        /// NSString is toll-free bridged to CFString; autoreleased, so no CFRelease.
        unsafe fn cfstr(s: &str) -> CFTypeRef {
            let c = CString::new(s).unwrap_or_default();
            let ns: *mut Object = msg_send![class!(NSString), stringWithUTF8String: c.as_ptr()];
            ns as CFTypeRef
        }

        /// Copy an attribute; the caller owns (and must CFRelease) the result.
        unsafe fn copy_attr(el: CFTypeRef, name: &str) -> Option<CFTypeRef> {
            let mut out: CFTypeRef = std::ptr::null();
            let err = AXUIElementCopyAttributeValue(el, cfstr(name), &mut out);
            (err == 0 && !out.is_null()).then_some(out)
        }

        /// CFString -> String (None for any other CF type).
        unsafe fn string_of(v: CFTypeRef) -> Option<String> {
            if CFGetTypeID(v) != CFStringGetTypeID() {
                return None;
            }
            let p: *const c_char = msg_send![v as *mut Object, UTF8String];
            (!p.is_null()).then(|| CStr::from_ptr(p).to_string_lossy().into_owned())
        }

        unsafe fn string_attr(el: CFTypeRef, name: &str) -> Option<String> {
            let v = copy_attr(el, name)?;
            let s = string_of(v);
            CFRelease(v);
            s
        }

        pub enum Ax {
            Text(String),
            /// An editable text control that reports an empty selection: trust it.
            NoneSelected,
            /// Web content / custom views often report "" even with text highlighted.
            Unknown,
        }

        pub fn selected_text() -> Ax {
            objc::rc::autoreleasepool(|| unsafe {
                let sys = AXUIElementCreateSystemWide();
                if sys.is_null() {
                    return Ax::Unknown;
                }
                // a hung target app must not stall the popup
                AXUIElementSetMessagingTimeout(sys, 0.15);
                let focused = copy_attr(sys, "AXFocusedUIElement");
                CFRelease(sys);
                let Some(focused) = focused else { return Ax::Unknown };

                let mut out = Ax::Unknown;
                let text = string_attr(focused, "AXSelectedText").unwrap_or_default();
                if !text.trim().is_empty() {
                    out = Ax::Text(text);
                } else if let Some(t) = marker_range_text(focused).filter(|t| !t.trim().is_empty()) {
                    // Safari / Chromium page selection lives in a text-marker range
                    out = Ax::Text(t);
                } else {
                    let role = string_attr(focused, "AXRole").unwrap_or_default();
                    if matches!(role.as_str(), "AXTextField" | "AXTextArea" | "AXComboBox") {
                        out = Ax::NoneSelected;
                    }
                }
                CFRelease(focused);
                out
            })
        }

        unsafe fn marker_range_text(el: CFTypeRef) -> Option<String> {
            let range = copy_attr(el, "AXSelectedTextMarkerRange")?;
            let mut out: CFTypeRef = std::ptr::null();
            let err = AXUIElementCopyParameterizedAttributeValue(el, cfstr("AXStringForTextMarkerRange"), range, &mut out);
            CFRelease(range);
            if err != 0 || out.is_null() {
                return None;
            }
            let s = string_of(out);
            CFRelease(out);
            s
        }
    }

    pub fn fast() -> super::Fast {
        match ax::selected_text() {
            ax::Ax::Text(text) => super::Fast::Done(super::clean(text)),
            ax::Ax::NoneSelected => super::Fast::Done(None),
            ax::Ax::Unknown => super::Fast::Slow,
        }
    }

    /// Simulate Cmd+C and watch the clipboard (up to ~800ms).
    pub fn slow() -> Option<String> {
        wait_modifiers_released();

        let mut cb = arboard::Clipboard::new().ok()?;
        let old_text = cb.get_text().ok();
        let old_image = if old_text.is_none() { cb.get_image().ok() } else { None };

        let nanos = SystemTime::now().duration_since(UNIX_EPOCH).map(|d| d.as_nanos()).unwrap_or(0);
        let sentinel = format!("\u{1}jink-selection-{nanos}");
        cb.set_text(sentinel.clone()).ok()?;

        let copied = crate::mac_keys::cmd_key(crate::mac_keys::KEY_C).ok();

        let mut result = None;
        if copied.is_some() {
            let start = Instant::now();
            while start.elapsed() < Duration::from_millis(300) {
                sleep(Duration::from_millis(15));
                if let Ok(t) = cb.get_text() {
                    if t != sentinel {
                        result = Some(t);
                        break;
                    }
                }
            }
        }

        // restore whatever the user had on the clipboard
        match (old_text, old_image) {
            (Some(t), _) => {
                let _ = cb.set_text(t);
            }
            (None, Some(img)) => {
                let _ = cb.set_image(img);
            }
            (None, None) => {
                let _ = cb.clear();
            }
        }
        result.and_then(super::clean)
    }
}

/// Windows (untested target): not supported yet.
#[cfg(not(any(target_os = "linux", target_os = "macos")))]
mod imp {
    pub fn fast() -> super::Fast {
        super::Fast::Done(None)
    }

    pub fn slow() -> Option<String> {
        None
    }
}
