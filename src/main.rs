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

static POPUP_WINDOW: std::sync::Mutex<Option<AnyWindowHandle>> = std::sync::Mutex::new(None);

#[cfg(target_os = "linux")]
struct KeeperView;
#[cfg(target_os = "linux")]
impl Render for KeeperView {
    fn render(&mut self, _window: &mut Window, _cx: &mut Context<Self>) -> impl IntoElement {
        div()
    }
}

// KWin ignores the requested position of a Normal window and gpui's X11 window_handle() is
// unimplemented: find our widest window (not the 1px keeper) via xdotool, then place/pin/focus it.
// 20ms poll: slower polling lets KWin show the window at its own spot (centered) before the move.
#[cfg(target_os = "linux")]
pub fn place_x11_popup(origin: Option<(i32, i32)>) {
    let script = r#"for i in $(seq 100); do sleep 0.02
for w in $(xdotool search --pid $PID_JINK); do
eval $(xdotool getwindowgeometry --shell $w)
[ "$WIDTH" -gt 1 ] || continue
[ -n "$JINK_X" ] && xdotool windowmove $w $JINK_X $JINK_Y
[ -n "$JINK_X" ] && wmctrl -i -r $w -b add,above,skip_taskbar,skip_pager
xdotool windowfocus $w && exit 0
done; done"#;
    let mut cmd = std::process::Command::new("sh");
    cmd.args(["-c", script]).stderr(std::process::Stdio::null()).env("PID_JINK", std::process::id().to_string());
    if let Some((x, y)) = origin {
        cmd.env("JINK_X", x.to_string()).env("JINK_Y", y.to_string());
    }
    let _ = cmd.spawn();
}

// PRIMARY selection = text highlighted in the previously focused app
#[cfg(target_os = "linux")]
fn selected_text() -> Option<String> {
    use arboard::{GetExtLinux, LinuxClipboardKind};
    let text = arboard::Clipboard::new()
        .ok()?
        .get()
        .clipboard(LinuxClipboardKind::Primary)
        .text()
        .ok()?;
    let text = text.trim_end().to_string();
    (!text.is_empty()).then_some(text)
}

#[cfg(not(target_os = "linux"))]
fn selected_text() -> Option<String> {
    None
}

fn open_popup(cx: &mut App) {
    let popup_size = size(px(560.0), px(118.0));
    let settings = settings::load_settings();
    let selection = if settings.prefill_selection || settings.selection_context { selected_text() } else { None };
    let (initial_text, context) = if settings.prefill_selection { (selection, None) } else { (None, selection) };
    let anchor = if settings.follow_cursor {
        cursor_position().map(|c| Point::new(c.x + px(12.0), c.y + px(16.0)))
    } else {
        settings.popup_x.zip(settings.popup_y).map(|(x, y)| Point::new(px(x), px(y)))
    };
    let bounds = match anchor {
        Some(mut origin) => {
            // keep the popup on a display (settings panel growth included)
            let max_h = px(410.0);
            if let Some(area) = cx.displays().into_iter().map(|d| d.bounds()).find(|d| d.contains(&origin)) {
                origin.x = origin.x.min(area.right() - popup_size.width).max(area.left());
                origin.y = origin.y.min(area.bottom() - max_h).max(area.top());
            }
            Bounds::new(origin, popup_size)
        }
        None => Bounds::centered(None, popup_size, cx),
    };

    #[cfg(target_os = "linux")]
    let placed_at = Some((f32::from(bounds.origin.x) as i32, f32::from(bounds.origin.y) as i32));

    let window_options = WindowOptions {
        window_bounds: Some(WindowBounds::Windowed(bounds)),
        titlebar: None,
        #[cfg(target_os = "linux")]
        window_background: WindowBackgroundAppearance::Opaque,
        #[cfg(not(target_os = "linux"))]
        window_background: WindowBackgroundAppearance::Blurred,
        // X11 PopUp = NOTIFICATION window type: KWin never keeps keyboard focus on it
        #[cfg(target_os = "linux")]
        kind: WindowKind::Normal,
        // no KWin titlebar: our own header is the drag handle
        #[cfg(target_os = "linux")]
        window_decorations: Some(WindowDecorations::Client),
        #[cfg(not(target_os = "linux"))]
        kind: WindowKind::PopUp,
        ..Default::default()
    };

    if let Ok(handle) = cx.open_window(window_options, |window, cx| {
        let popup = cx.new(|cx| JInkPopup::new(initial_text, context, window, cx));
        let input_handle = popup.read(cx).input.focus_handle(cx);
        window.activate_window();
        #[cfg(target_os = "linux")]
        place_x11_popup(placed_at);
        window.focus(&input_handle);
        popup
    }) {
        if let Ok(mut guard) = POPUP_WINDOW.lock() {
            *guard = Some(handle.into());
        }
    }
}

fn toggle_popup(cx: &mut App) {
    let mut is_open = false;
    if let Ok(mut guard) = POPUP_WINDOW.lock() {
        if let Some(handle) = guard.take() {
            let open_windows: Vec<AnyWindowHandle> = cx.windows();
            if open_windows.contains(&handle) {
                is_open = true;
                handle.update(cx, |_, window, _| {
                    window.remove_window();
                }).ok();
            }
        }
    }

    if !is_open {
        open_popup(cx);
    }
}

static HOTKEY_MANAGER: std::sync::Mutex<Option<GlobalHotKeyManager>> = std::sync::Mutex::new(None);
static CURRENT_HOTKEY: std::sync::Mutex<Option<HotKey>> = std::sync::Mutex::new(None);

pub fn update_global_hotkey(hotkey_str: &str) -> bool {
    // strict parse: partial text typed in settings ("Alt+S") must not grab a real key combo
    let Ok(new_hotkey) = hotkey_str.parse::<HotKey>() else {
        return false;
    };
    let (Ok(guard), Ok(mut current)) = (HOTKEY_MANAGER.lock(), CURRENT_HOTKEY.lock()) else {
        return false;
    };
    let Some(manager) = guard.as_ref() else {
        return false;
    };
    if *current == Some(new_hotkey) {
        return true;
    }
    let previous = *current;
    if let Some(old) = previous {
        let _ = manager.unregister(old);
    }
    if manager.register(new_hotkey).is_ok() {
        *current = Some(new_hotkey);
        return true;
    }
    // new combo taken by another app: restore the old one
    if let Some(old) = previous {
        let _ = manager.register(old);
    }
    false
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
    #[cfg(target_os = "linux")]
    return HotKey::new(Some(Modifiers::ALT), Code::Space);
    #[cfg(not(target_os = "linux"))]
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
    if let Err(e) = hotkey_manager.register(hotkey) {
        // X11 reports a grab owned by the desktop (BadAccess) as "already registered"
        eprintln!("[jink] cannot register hotkey {:?}: {e}", settings.hotkey);
    }
    if let Ok(mut current) = CURRENT_HOTKEY.lock() {
        *current = Some(hotkey);
    }
    if let Ok(mut guard) = HOTKEY_MANAGER.lock() {
        *guard = Some(hotkey_manager);
    }
    let receiver = GlobalHotKeyEvent::receiver();

    #[cfg(target_os = "macos")]
    let (chord_tx, chord_rx) = std::sync::mpsc::channel();
    #[cfg(target_os = "macos")]
    macos_chord::start_chord_listener(chord_tx);

    Application::new().with_assets(Assets).run(move |cx| {
        text_input::bind_keys(cx);

        #[cfg(target_os = "linux")]
        {
            let keeper_options = WindowOptions {
                window_bounds: Some(WindowBounds::Windowed(Bounds::new(
                    Point::new(px(-9999.0), px(-9999.0)),
                    size(px(1.0), px(1.0)),
                ))),
                titlebar: None,
                kind: WindowKind::PopUp,
                ..Default::default()
            };
            let _ = cx.open_window(keeper_options, |_window, cx| {
                cx.new(|_| KeeperView)
            });
        }

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
