mod ai;
mod inject;
mod popup;
mod settings;
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
    let popup_size = size(px(680.0), px(160.0));
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
        window_background: WindowBackgroundAppearance::Transparent,
        kind: WindowKind::PopUp,
        ..Default::default()
    };

    let _ = cx.open_window(window_options, |_window, cx| {
        cx.new(|cx| JInkPopup::new(cx))
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

fn main() {
    let hotkey_manager = GlobalHotKeyManager::new().expect("Failed to init global hotkey manager");
    let hotkey = HotKey::new(Some(Modifiers::ALT), Code::Space);
    hotkey_manager.register(hotkey).expect("Failed to register Alt+Space");
    let receiver = GlobalHotKeyEvent::receiver();

    Application::new().run(move |cx| {
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
            }
        })
        .detach();
    });
}
