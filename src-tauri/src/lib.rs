mod ai;
mod inject;

use ai::{AiConfig, ChatMessage, LocalProvider};
use tauri::{
    menu::{Menu, MenuItem},
    tray::TrayIconBuilder,
    AppHandle, Emitter, Manager, PhysicalPosition, Window,
};
use tauri_plugin_global_shortcut::{GlobalShortcutExt, ShortcutState};

const POPUP: &str = "popup";
const SETTINGS: &str = "settings";

/// Show the popup at the cursor and focus it.
fn show_popup(app: &AppHandle) {
    let Some(win) = app.get_webview_window(POPUP) else {
        return;
    };
    position_at_cursor(app, &win);
    let _ = win.show();
    let _ = win.unminimize();
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
        .args(["search", "--onlyvisible", "--name", "^JInk$", "windowactivate"])
        .status();
}

fn hide_popup(win: &Window) {
    let _ = win.hide();
}

/// WebKitGTK denies `getUserMedia` with no prompt unless the host app grants
/// it via the `permission-request` signal. Auto-grant our own windows so the
/// voice-input mic works (there's no OS-level mic permission dialog on Linux
/// the way there is on macOS).
#[cfg(target_os = "linux")]
fn allow_media_permissions(win: &tauri::WebviewWindow) {
    use webkit2gtk::{PermissionRequestExt, WebViewExt};
    let _ = win.with_webview(|webview| {
        webview.inner().connect_permission_request(|_, request| {
            request.allow();
            true
        });
    });
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

#[derive(serde::Deserialize)]
struct ImagePart {
    index: u32,
    path: String,
}

/// Hide the popup (returning focus to the target app), then inject `text` there.
/// `[#ImageN]` tokens in `text` are replaced by pasting the matching image.
#[tauri::command]
async fn inject_text(window: Window, text: String, images: Vec<ImagePart>) -> Result<(), String> {
    hide_popup(&window);
    let images: Vec<(u32, String)> = images.into_iter().map(|i| (i.index, i.path)).collect();
    tokio::task::spawn_blocking(move || inject::inject_text(&text, &images))
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

/// Scan well-known local ports (Ollama, LM Studio, llama.cpp server, …) for a
/// running OpenAI-compatible endpoint and list their available models.
#[tauri::command]
async fn detect_local_ai() -> Vec<LocalProvider> {
    ai::detect_local().await
}

/// Start a local runtime found installed-but-not-running. Daemons (Ollama, LM
/// Studio) ignore `model`; launchers (llama-server) are spawned bound to it.
#[tauri::command]
async fn start_local_ai(name: String, model: Option<ai::LocalModel>) -> Result<String, String> {
    ai::start_local(&name, model).await
}

/// Stop a running local runtime (kills what we started; else its stop CLI).
#[tauri::command]
async fn stop_local_ai(name: String) -> Result<String, String> {
    ai::stop_local(&name).await
}

/// Write pasted image bytes to a temp file, returning its path so it can be
/// injected into the target app (which accepts an image by path).
#[tauri::command]
fn save_temp_image(bytes: Vec<u8>, ext: String) -> Result<String, String> {
    use std::io::Write;
    let ext = if ext.is_empty() { "png" } else { &ext };
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_nanos())
        .unwrap_or(0);
    let path = std::env::temp_dir().join(format!("jink-{nanos}.{ext}"));
    std::fs::File::create(&path)
        .and_then(|mut f| f.write_all(&bytes))
        .map_err(|e| format!("Couldn't save image: {e}"))?;
    Ok(path.to_string_lossy().to_string())
}

/// Re-register the global hotkey (unregister everything first).
#[tauri::command]
fn set_hotkey(app: AppHandle, accelerator: String) -> Result<(), String> {
    let gs = app.global_shortcut();
    let _ = gs.unregister_all();
    gs.register(accelerator.as_str()).map_err(|e| e.to_string())
}

/// Build the settings window hidden. A webview costs ~a second to create, so
/// it's built once at startup and reused — closing it only hides it.
fn build_settings_window(app: &AppHandle) -> Result<tauri::WebviewWindow, String> {
    let win = tauri::WebviewWindowBuilder::new(
        app,
        SETTINGS,
        tauri::WebviewUrl::App("index.html#/settings".into()),
    )
    .title("JInk — Settings")
    .inner_size(560.0, 640.0)
    .resizable(true)
    .visible(false)
    .build()
    .map_err(|e| e.to_string())?;
    #[cfg(target_os = "linux")]
    allow_media_permissions(&win);
    Ok(win)
}

/// Open (or focus) the settings window.
#[tauri::command]
fn open_settings(app: AppHandle) -> Result<(), String> {
    // Prewarmed in setup(); rebuild only if that failed.
    let win = match app.get_webview_window(SETTINGS) {
        Some(w) => w,
        None => build_settings_window(&app)?,
    };
    let _ = win.show();
    let _ = win.unminimize();
    let _ = win.set_focus();
    // Window outlives a close, so its state is stale.
    let _ = win.emit("settings-shown", ());
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
            focused_window_is_helper,
            detect_local_ai,
            start_local_ai,
            stop_local_ai,
            save_temp_image
        ])
        .setup(|app| {
            #[cfg(target_os = "linux")]
            if let Some(win) = app.get_webview_window(POPUP) {
                allow_media_permissions(&win);
            }

            // Prewarm the settings webview so the first open is instant.
            if let Err(e) = build_settings_window(app.handle()) {
                eprintln!("settings window prewarm failed: {e}");
            }

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
                .tooltip("JInk")
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
        .on_window_event(|window, event| {
            // Keep the settings webview alive — a destroy means the next open
            // pays for a full rebuild.
            if window.label() == SETTINGS {
                if let tauri::WindowEvent::CloseRequested { api, .. } = event {
                    api.prevent_close();
                    let _ = window.hide();
                }
            }
        })
        .build(tauri::generate_context!())
        .expect("error while building tauri application")
        .run(|_app, event| {
            // Stay alive in the tray after windows close, but let an
            // explicit app.exit() (tray "Quit") go through — code is None
            // when the exit was triggered by closing the last window, and
            // Some(_) when requested programmatically.
            if let tauri::RunEvent::ExitRequested { api, code, .. } = event {
                if code.is_none() {
                    api.prevent_exit();
                }
            }
        });
}
