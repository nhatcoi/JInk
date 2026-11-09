mod ai;
mod inject;
mod popup;
mod settings;
mod text_input;
mod theme;

use global_hotkey::hotkey::{Code, HotKey, Modifiers};
use global_hotkey::{GlobalHotKeyEvent, GlobalHotKeyManager};
use gpui::*;
use popup::JInkPopup;

#[cfg(target_os = "macos")]
fn cursor_position() -> Option<Point<Pixels>> {
    #[link(name = "CoreGraphics", kind = "framework")]
    extern "C" {
        fn CGEventCreate(source: *const std::ffi::c_void) -> *mut std::ffi::c_void;
        fn CGEventGetLocation(event: *mut std::ffi::c_void) -> CGPoint;
        fn CFRelease(cf: *const std::ffi::c_void);
    }
    #[repr(C)]
    struct CGPoint {
        x: f64,
        y: f64,
    }

    unsafe {
        let event = CGEventCreate(std::ptr::null());
        if event.is_null() {
            return None;
        }
        let pt = CGEventGetLocation(event);
        CFRelease(event);
        Some(Point::new(px(pt.x as f32), px(pt.y as f32)))
    }
}

#[cfg(target_os = "linux")]
fn cursor_position() -> Option<Point<Pixels>> {
    let out = std::process::Command::new("xdotool")
        .arg("getmouselocation")
        .output()
        .ok()?;
    let s = String::from_utf8_lossy(&out.stdout);
    let mut x = None;
    let mut y = None;
    for part in s.split_whitespace() {
        if let Some(val) = part.strip_prefix("x:") {
            x = val.parse::<f32>().ok();
        } else if let Some(val) = part.strip_prefix("y:") {
            y = val.parse::<f32>().ok();
        }
    }
    if let (Some(x), Some(y)) = (x, y) {
        Some(Point::new(px(x), px(y)))
    } else {
        None
    }
}

#[cfg(not(any(target_os = "macos", target_os = "linux")))]
fn cursor_position() -> Option<Point<Pixels>> {
    None
}

fn open_popup(cx: &mut App) {
    let popup_size = size(px(560.0), px(118.0));
    let bounds = if let Some(cursor) = cursor_position() {
        let x = cursor.x + px(12.0);
        let y = cursor.y + px(16.0);
        Bounds::new(Point::new(x, y), popup_size)
    } else {
        Bounds::centered(None, popup_size, cx)
    };

    let window_options = WindowOptions {
        window_bounds: Some(WindowBounds::Windowed(bounds)),
        titlebar: None,
        window_background: WindowBackgroundAppearance::Blurred,
        kind: WindowKind::PopUp,
        ..Default::default()
    };

    let _ = cx.open_window(window_options, |window, cx| {
        let popup = cx.new(|cx| JInkPopup::new(window, cx));
        let handle = popup.read(cx).input.focus_handle(cx);
        window.focus(&handle);
        popup
    });
}

fn toggle_popup(cx: &mut App) {
    let open_windows: Vec<AnyWindowHandle> = cx.windows();
    if open_windows.is_empty() {
        open_popup(cx);
    } else {
        for window in open_windows {
            window.update(cx, |_, window, _| {
                window.remove_window();
            }).ok();
        }
    }
}

fn parse_or_default_hotkey(hotkey_str: &str) -> HotKey {
    if let Ok(hk) = hotkey_str.parse::<HotKey>() {
        return hk;
    }
    // Nếu chuỗi chỉ có modifiers (ví dụ "Control+Cmd" hay "Ctrl+Cmd"), bổ sung thêm phím Space
    let with_space = format!("{}+Space", hotkey_str.trim_end_matches('+'));
    if let Ok(hk) = with_space.parse::<HotKey>() {
        return hk;
    }
    HotKey::new(Some(Modifiers::CONTROL | Modifiers::META), Code::Space)
}

#[cfg(target_os = "macos")]
mod macos_chord {
    use std::ptr;
    use std::sync::atomic::{AtomicBool, Ordering};
    use std::sync::mpsc::Sender;
    use std::sync::Mutex;
    use std::time::Instant;

    #[link(name = "CoreGraphics", kind = "framework")]
    #[link(name = "CoreFoundation", kind = "framework")]
    extern "C" {
        fn CGEventTapCreate(
            tap: u32,
            place: u32,
            options: u32,
            events_of_interest: u64,
            callback: extern "C" fn(*mut std::ffi::c_void, u32, *mut std::ffi::c_void, *mut std::ffi::c_void) -> *mut std::ffi::c_void,
            user_info: *mut std::ffi::c_void,
        ) -> *mut std::ffi::c_void;
        fn CGEventGetFlags(event: *mut std::ffi::c_void) -> u64;
        fn CGEventTapEnable(tap: *mut std::ffi::c_void, enable: bool);

        fn CFMachPortCreateRunLoopSource(
            allocator: *const std::ffi::c_void,
            port: *mut std::ffi::c_void,
            order: isize,
        ) -> *mut std::ffi::c_void;

        fn CFRunLoopGetCurrent() -> *mut std::ffi::c_void;
        fn CFRunLoopAddSource(rl: *mut std::ffi::c_void, source: *mut std::ffi::c_void, mode: *const std::ffi::c_void);
        fn CFRunLoopRun();

        static kCFRunLoopCommonModes: *const std::ffi::c_void;
    }

    static CHORD_ACTIVE: AtomicBool = AtomicBool::new(false);
    static KEY_PRESSED: AtomicBool = AtomicBool::new(false);
    static CHORD_START: Mutex<Option<Instant>> = Mutex::new(None);
    static SENDER: Mutex<Option<Sender<()>>> = Mutex::new(None);
    static TAP_PORT: Mutex<Option<usize>> = Mutex::new(None);

    extern "C" fn tap_callback(
        _proxy: *mut std::ffi::c_void,
        event_type: u32,
        event: *mut std::ffi::c_void,
        _user_info: *mut std::ffi::c_void,
    ) -> *mut std::ffi::c_void {
        // kCGEventTapDisabledByTimeout = 0xFFFFFFFE
        if event_type == 0xFFFF_FFFE || event_type == 0xFFFF_FFFF {
            if let Ok(guard) = TAP_PORT.lock() {
                if let Some(port) = *guard {
                    unsafe {
                        CGEventTapEnable(port as *mut std::ffi::c_void, true);
                    }
                }
            }
            return event;
        }

        if event_type == 10 { // kCGEventKeyDown
            KEY_PRESSED.store(true, Ordering::SeqCst);
        } else if event_type == 12 { // kCGEventFlagsChanged
            let flags = unsafe { CGEventGetFlags(event) };
            let has_ctrl = (flags & 0x00040000) != 0;
            let has_cmd = (flags & 0x00100000) != 0;
            let has_alt = (flags & 0x00080000) != 0;
            let has_shift = (flags & 0x00020000) != 0;

            if has_ctrl && has_cmd && !has_alt && !has_shift {
                if !CHORD_ACTIVE.swap(true, Ordering::SeqCst) {
                    KEY_PRESSED.store(false, Ordering::SeqCst);
                    if let Ok(mut guard) = CHORD_START.lock() {
                        *guard = Some(Instant::now());
                    }
                }
            } else if CHORD_ACTIVE.swap(false, Ordering::SeqCst) {
                if !KEY_PRESSED.load(Ordering::SeqCst) {
                    let start = CHORD_START.lock().ok().and_then(|mut g| g.take());
                    if let Some(start) = start {
                        // Nhấn nhả Control + Cmd trong khoảng 800ms
                        if start.elapsed() < std::time::Duration::from_millis(800) {
                            if let Ok(guard) = SENDER.lock() {
                                if let Some(ref tx) = *guard {
                                    let _ = tx.send(());
                                }
                            }
                        }
                    }
                }
            }
        }
        event
    }

    pub fn start_chord_listener(tx: Sender<()>) {
        std::thread::spawn(move || unsafe {
            if let Ok(mut guard) = SENDER.lock() {
                *guard = Some(tx);
            }
            let mask = (1 << 12) | (1 << 10);
            let tap = CGEventTapCreate(1, 0, 1, mask, tap_callback, ptr::null_mut());
            if tap.is_null() {
                return;
            }
            if let Ok(mut guard) = TAP_PORT.lock() {
                *guard = Some(tap as usize);
            }
            let source = CFMachPortCreateRunLoopSource(ptr::null(), tap, 0);
            if source.is_null() {
                return;
            }
            let rl = CFRunLoopGetCurrent();
            CFRunLoopAddSource(rl, source, kCFRunLoopCommonModes);
            CGEventTapEnable(tap, true);
            CFRunLoopRun();
        });
    }
}

/// Shared Tokio runtime: reqwest needs one, GPUI's executors are not Tokio.
pub fn runtime() -> &'static tokio::runtime::Runtime {
    static RT: std::sync::OnceLock<tokio::runtime::Runtime> = std::sync::OnceLock::new();
    RT.get_or_init(|| {
        tokio::runtime::Builder::new_multi_thread()
            .worker_threads(2)
            .enable_all()
            .build()
            .expect("tokio runtime")
    })
}

struct Assets;

impl AssetSource for Assets {
    fn load(&self, path: &str) -> anyhow::Result<Option<std::borrow::Cow<'static, [u8]>>> {
        let bytes: &'static [u8] = match path {
            "svg/plus.svg" => include_bytes!("../assets/svg/plus.svg"),
            "svg/arrow-up.svg" => include_bytes!("../assets/svg/arrow-up.svg"),
            "svg/sparkles.svg" => include_bytes!("../assets/svg/sparkles.svg"),
            "svg/languages.svg" => include_bytes!("../assets/svg/languages.svg"),
            "svg/help.svg" => include_bytes!("../assets/svg/help.svg"),
            "svg/settings.svg" => include_bytes!("../assets/svg/settings.svg"),
            "svg/undo.svg" => include_bytes!("../assets/svg/undo.svg"),
            _ => return Ok(None),
        };
        Ok(Some(std::borrow::Cow::Borrowed(bytes)))
    }

    fn list(&self, _path: &str) -> anyhow::Result<Vec<SharedString>> {
        Ok(Vec::new())
    }
}

fn main() {
    let settings = settings::load_settings();
    let hotkey_manager = GlobalHotKeyManager::new().expect("Failed to init global hotkey manager");
    let hotkey = parse_or_default_hotkey(&settings.hotkey);
    let _ = hotkey_manager.register(hotkey);
    let receiver = GlobalHotKeyEvent::receiver();

    #[cfg(target_os = "macos")]
    let (chord_tx, chord_rx) = std::sync::mpsc::channel();
    #[cfg(target_os = "macos")]
    macos_chord::start_chord_listener(chord_tx);

    Application::new().with_assets(Assets).run(move |cx| {
        text_input::bind_keys(cx);
        // Mở popup ngay khi khởi động
        open_popup(cx);

        // Vòng lặp lắng nghe phím tắt toàn cục bất đồng bộ (Non-blocking async timer)
        cx.spawn(async move |cx| {
            loop {
                // Yield to event loop, sleep 30ms không bao giờ làm block main thread
                cx.background_executor().timer(std::time::Duration::from_millis(30)).await;

                while let Ok(event) = receiver.try_recv() {
                    if event.state == global_hotkey::HotKeyState::Pressed {
                        let _ = cx.update(|cx| {
                            toggle_popup(cx);
                        });
                    }
                }

                #[cfg(target_os = "macos")]
                while let Ok(()) = chord_rx.try_recv() {
                    let _ = cx.update(|cx| {
                        toggle_popup(cx);
                    });
                }
            }
        })
        .detach();
    });
}
