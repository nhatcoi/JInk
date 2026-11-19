use std::path::{Path, PathBuf};

fn exe_path() -> Result<PathBuf, String> {
    #[cfg(target_os = "linux")]
    if let Some(appimage) = std::env::var_os("APPIMAGE") {
        let p = PathBuf::from(appimage);
        if p.is_absolute() {
            return Ok(p);
        }
    }
    std::env::current_exe().map_err(|e| e.to_string())
}

#[cfg(target_os = "linux")]
mod platform {
    use super::*;

    fn desktop_path() -> Result<PathBuf, String> {
        let base = dirs::config_dir().ok_or_else(|| "Cannot locate config directory".to_string())?;
        Ok(base.join("autostart").join("jink.desktop"))
    }

    pub(super) fn desktop_contents(exe: &Path) -> String {
        let escaped = exe
            .to_string_lossy()
            .replace('\\', "\\\\")
            .replace('"', "\\\"");
        format!(
            "[Desktop Entry]\n\
             Type=Application\n\
             Name=JInk\n\
             Comment=Quick input popup: type, AI-enhance, translate, voice\n\
             Exec=\"{escaped}\" --background\n\
             Icon=jink\n\
             Terminal=false\n\
             Categories=Utility;\n\
             X-GNOME-Autostart-enabled=true\n"
        )
    }

    #[allow(dead_code)]
    pub fn is_enabled() -> bool {
        desktop_path()
            .ok()
            .and_then(|p| std::fs::read_to_string(p).ok())
            .is_some_and(|c| !c.lines().any(|l| l.trim().eq_ignore_ascii_case("Hidden=true")))
    }

    pub fn set_enabled(enable: bool) -> Result<(), String> {
        let path = desktop_path()?;
        if enable {
            let exe = exe_path()?;
            if let Some(parent) = path.parent() {
                std::fs::create_dir_all(parent).map_err(|e| e.to_string())?;
            }
            std::fs::write(&path, desktop_contents(&exe)).map_err(|e| e.to_string())
        } else {
            match std::fs::remove_file(&path) {
                Ok(()) => Ok(()),
                Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(()),
                Err(e) => Err(e.to_string()),
            }
        }
    }
}

#[cfg(target_os = "macos")]
mod platform {
    use super::*;

    const LABEL: &str = "com.nhatcoi.jink";

    fn plist_path() -> Result<PathBuf, String> {
        let home = dirs::home_dir().ok_or_else(|| "Cannot locate home directory".to_string())?;
        Ok(home.join("Library").join("LaunchAgents").join(format!("{LABEL}.plist")))
    }

    pub(super) fn plist_contents(exe: &Path) -> String {
        let escaped = exe
            .to_string_lossy()
            .replace('&', "&amp;")
            .replace('<', "&lt;")
            .replace('>', "&gt;");
        format!(
            "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n\
             <!DOCTYPE plist PUBLIC \"-//Apple//DTD PLIST 1.0//EN\" \"http://www.apple.com/DTDs/PropertyList-1.0.dtd\">\n\
             <plist version=\"1.0\">\n\
             <dict>\n\
             \t<key>Label</key>\n\
             \t<string>{LABEL}</string>\n\
             \t<key>ProgramArguments</key>\n\
             \t<array>\n\
             \t\t<string>{escaped}</string>\n\
             \t\t<string>--background</string>\n\
             \t</array>\n\
             \t<key>RunAtLoad</key>\n\
             \t<true/>\n\
             \t<key>KeepAlive</key>\n\
             \t<false/>\n\
             </dict>\n\
             </plist>\n"
        )
    }

    #[allow(dead_code)]
    pub fn is_enabled() -> bool {
        plist_path().is_ok_and(|p| p.exists())
    }

    pub fn set_enabled(enable: bool) -> Result<(), String> {
        let path = plist_path()?;
        if enable {
            let exe = exe_path()?;
            if let Some(parent) = path.parent() {
                std::fs::create_dir_all(parent).map_err(|e| e.to_string())?;
            }
            std::fs::write(&path, plist_contents(&exe)).map_err(|e| e.to_string())
        } else {
            match std::fs::remove_file(&path) {
                Ok(()) => Ok(()),
                Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(()),
                Err(e) => Err(e.to_string()),
            }
        }
    }
}

#[cfg(target_os = "windows")]
mod platform {
    use super::*;
    use std::process::Command;

    const RUN_KEY: &str = r"HKCU\Software\Microsoft\Windows\CurrentVersion\Run";
    const VALUE_NAME: &str = "JInk";

    #[allow(dead_code)]
    pub fn is_enabled() -> bool {
        Command::new("reg")
            .args(["query", RUN_KEY, "/v", VALUE_NAME])
            .output()
            .is_ok_and(|o| o.status.success())
    }

    pub fn set_enabled(enable: bool) -> Result<(), String> {
        if enable {
            let exe = exe_path()?;
            let cmd = format!("\"{}\" --background", exe.to_string_lossy());
            let out = Command::new("reg")
                .args(["add", RUN_KEY, "/v", VALUE_NAME, "/t", "REG_SZ", "/d", &cmd, "/f"])
                .output()
                .map_err(|e| e.to_string())?;
            if out.status.success() {
                Ok(())
            } else {
                Err(String::from_utf8_lossy(&out.stderr).trim().to_string())
            }
        } else {
            let out = Command::new("reg")
                .args(["delete", RUN_KEY, "/v", VALUE_NAME, "/f"])
                .output()
                .map_err(|e| e.to_string())?;
            if out.status.success() || !is_enabled() {
                Ok(())
            } else {
                Err(String::from_utf8_lossy(&out.stderr).trim().to_string())
            }
        }
    }
}

#[cfg(not(any(target_os = "linux", target_os = "macos", target_os = "windows")))]
mod platform {
    #[allow(dead_code)]
    pub fn is_enabled() -> bool {
        false
    }

    pub fn set_enabled(_enable: bool) -> Result<(), String> {
        Err("Auto-launch is not supported on this OS".into())
    }
}

#[allow(dead_code)]
pub fn is_enabled() -> bool {
    platform::is_enabled()
}

pub fn set_enabled(enable: bool) -> Result<(), String> {
    platform::set_enabled(enable)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[cfg(target_os = "linux")]
    #[test]
    fn desktop_entry_quotes_exe_and_passes_background() {
        let entry = platform::desktop_contents(Path::new("/home/user/My Apps/jink"));
        assert!(entry.contains("Exec=\"/home/user/My Apps/jink\" --background\n"));
        assert!(entry.contains("X-GNOME-Autostart-enabled=true"));
    }
}
