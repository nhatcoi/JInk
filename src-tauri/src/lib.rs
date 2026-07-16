mod ai;
mod inject;

use ai::{AiConfig, ChatMessage};
use tauri::{
    menu::{Menu, MenuItem},
    tray::TrayIconBuilder,
    AppHandle, Emitter, Manager, PhysicalPosition, Window,
};
use tauri_plugin_global_shortcut::{GlobalShortcutExt, ShortcutState};

const POPUP: &str = "popup";

/// Show the popup at the cursor and focus it.
fn show_popup(app: &AppHandle) {
    let Some(win) = app.get_webview_window(POPUP) else {
        return;
    };
    position_at_cursor(app, &win);
    let _ = win.show();
    let _ = win.unminimize();
    let _ = win.set_always_on_top(true);
    let _ = win.set_focus();
    let _ = win.emit("popup-shown", ());

    // KWin's focus-stealing prevention rejects set_focus() on a re-shown window
    // (works only on first map). Re-assert focus off-thread; on Linux activate
    // via xdotool as a fallback.
    let w = win.clone();
    std::thread::spawn(move || {
        for _ in 0..3 {
            std::thread::sleep(std::time::Duration::from_millis(50));
            let _ = w.set_focus();
            #[cfg(target_os = "linux")]
            force_focus_x11();
        }
    });
}

/// Activate the popup via xdotool (EWMH `_NET_ACTIVE_WINDOW`), bypassing KWin's
/// focus-stealing prevention. `windowactivate` avoids the BadMatch that
/// `windowfocus` (raw XSetInputFocus) throws on a not-yet-viewable window.
#[cfg(target_os = "linux")]
fn force_focus_x11() {
    let _ = std::process::Command::new("xdotool")
        .args(["search", "--onlyvisible", "--name", "^easyinput$", "windowactivate"])
        .status();
}

fn hide_popup(win: &Window) {
    let _ = win.hide();
}

/// Position the popup below-right of the cursor, clamped to its monitor.
fn position_at_cursor(app: &AppHandle, win: &tauri::WebviewWindow) {
    let Ok(cursor) = app.cursor_position() else {
        let _ = win.center();
        return;
    };
    let size = win.outer_size().unwrap_or(tauri::PhysicalSize {
        width: 700,
        height: 150,
    });

    // Monitor under the cursor, for bounds clamping.
    let monitors = win.available_monitors().unwrap_or_default();
    let monitor = monitors.into_iter().find(|m| {
        let p = m.position();
        let s = m.size();
        let cx = cursor.x as i32;
        let cy = cursor.y as i32;
        cx >= p.x && cx < p.x + s.width as i32 && cy >= p.y && cy < p.y + s.height as i32
    });

    let mut x = cursor.x as i32 + 12;
    let mut y = cursor.y as i32 + 16;

    if let Some(m) = monitor {
        let mp = m.position();
        let ms = m.size();
        let max_x = mp.x + ms.width as i32 - size.width as i32;
        let max_y = mp.y + ms.height as i32 - size.height as i32;
        x = x.clamp(mp.x, max_x.max(mp.x));
        y = y.clamp(mp.y, max_y.max(mp.y));
    }

    let _ = win.set_position(PhysicalPosition::new(x, y));
}

// ---- Commands ----

#[tauri::command]
fn hide_window(window: Window) {
    hide_popup(&window);
}

/// Clipboard managers and file managers, by window class / process name.
/// Focus-out to one of these means "don't hide" — drop below it instead.
const HELPER_APPS: &[&str] = &[
    // clipboard managers (Linux + macOS)
    "copyq", "clipit", "greenclip", "clipman", "gpaste", "klipper", "diodon",
    "parcellite", "copyclip", "maccy", "clipy", "flycut", "paste", "raycast",
    // file managers (Linux + macOS)
    "nautilus", "dolphin", "thunar", "nemo", "pcmanfm", "konqueror", "files",
    "finder", "forklift", "path finder",
];

/// True if the window that currently has OS focus is a known helper app.
#[tauri::command]
fn focused_window_is_helper() -> bool {
    let name = active_window_name().unwrap_or_default().to_lowercase();
    !name.is_empty() && HELPER_APPS.iter().any(|a| name.contains(a))
}

#[cfg(target_os = "linux")]
fn active_window_name() -> Option<String> {
    let out = std::process::Command::new("xdotool")
        .args(["getactivewindow", "getwindowclassname"])
        .output()
        .ok()?;
    Some(String::from_utf8_lossy(&out.stdout).trim().to_string())
}

#[cfg(target_os = "macos")]
fn active_window_name() -> Option<String> {
    let out = std::process::Command::new("osascript")
        .args([
            "-e",
            "tell application \"System Events\" to get name of first process whose frontmost is true",
        ])
        .output()
        .ok()?;
    Some(String::from_utf8_lossy(&out.stdout).trim().to_string())
}

#[cfg(not(any(target_os = "linux", target_os = "macos")))]
fn active_window_name() -> Option<String> {
    None
}

/// Hide the popup (returning focus to the target app), then paste `text` there.
#[tauri::command]
async fn inject_text(window: Window, text: String) -> Result<(), String> {
    hide_popup(&window);
    tokio::task::spawn_blocking(move || inject::inject_text(&text))
        .await
        .map_err(|e| e.to_string())?
}

/// Start a streaming AI completion. Tokens arrive via `ai-token` events.
#[tauri::command]
async fn ai_stream(
    window: Window,
    config: AiConfig,
    messages: Vec<ChatMessage>,
    request_id: String,
) {
    ai::stream_chat(window, config, messages, request_id).await;
}

/// Re-register the global hotkey (unregister everything first).
#[tauri::command]
fn set_hotkey(app: AppHandle, accelerator: String) -> Result<(), String> {
    let gs = app.global_shortcut();
    let _ = gs.unregister_all();
    gs.register(accelerator.as_str()).map_err(|e| e.to_string())
}

/// Open (or focus) the settings window.
#[tauri::command]
fn open_settings(app: AppHandle) -> Result<(), String> {
    if let Some(win) = app.get_webview_window("settings") {
        let _ = win.show();
        let _ = win.set_focus();
        return Ok(());
    }
    tauri::WebviewWindowBuilder::new(
        &app,
        "settings",
        tauri::WebviewUrl::App("index.html#/settings".into()),
    )
    .title("easyinput — Settings")
    .inner_size(560.0, 640.0)
    .resizable(true)
    .build()
    .map_err(|e| e.to_string())?;
    Ok(())
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_opener::init())
        .plugin(tauri_plugin_store::Builder::new().build())
        .plugin(tauri_plugin_clipboard_manager::init())
        .plugin(tauri_plugin_dialog::init())
        .plugin(
            tauri_plugin_global_shortcut::Builder::new()
                .with_handler(|app, _shortcut, event| {
                    if event.state() == ShortcutState::Pressed {
                        // Toggle visibility.
                        if let Some(win) = app.get_webview_window(POPUP) {
                            if win.is_visible().unwrap_or(false) {
                                let _ = win.hide();
                                return;
                            }
                        }
                        show_popup(app);
                    }
                })
                .build(),
        )
        .invoke_handler(tauri::generate_handler![
            hide_window,
            inject_text,
            ai_stream,
            set_hotkey,
            open_settings,
            focused_window_is_helper
        ])
        .setup(|app| {
            // Default hotkey; the frontend re-registers from settings.
            // (Alt+Space may collide with KDE KRunner — free it in KDE first.)
            let gs = app.global_shortcut();
            let _ = gs.unregister_all();
            if let Err(e) = gs.register("Alt+Space") {
                eprintln!("hotkey register failed: {e}");
            }

            // System tray.
            let open_i = MenuItem::with_id(app, "open", "Open input", true, None::<&str>)?;
            let settings_i = MenuItem::with_id(app, "settings", "Settings", true, None::<&str>)?;
            let quit_i = MenuItem::with_id(app, "quit", "Quit", true, None::<&str>)?;
            let menu = Menu::with_items(app, &[&open_i, &settings_i, &quit_i])?;
            TrayIconBuilder::new()
                .icon(app.default_window_icon().unwrap().clone())
                .menu(&menu)
                .tooltip("easyinput")
                .on_menu_event(move |app, event| match event.id.as_ref() {
                    "open" => show_popup(app),
                    "settings" => {
                        let _ = open_settings(app.clone());
                    }
                    "quit" => app.exit(0),
                    _ => {}
                })
                .build(app)?;

            Ok(())
        })
        .build(tauri::generate_context!())
        .expect("error while building tauri application")
        .run(|_app, event| {
            // Stay alive in the tray after windows close.
            if let tauri::RunEvent::ExitRequested { api, .. } = event {
                api.prevent_exit();
            }
        });
}
