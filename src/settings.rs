use serde::{Deserialize, Serialize};
use std::path::PathBuf;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AiHistoryEntry {
    pub base_url: String,
    pub model: String,
    pub key: String,
    pub used_at: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct Settings {
    pub ai_base_url: String,
    pub ai_model: String,
    pub ai_key: String,
    pub ai_history: Vec<AiHistoryEntry>,
    pub search_key: String,
    pub hotkey: String,
    pub width: f32,
    pub height: f32,
    pub translate_from: String,
    pub translate_to: String,
    pub main_language: String,
    pub hide_on_blur: bool,
    pub follow_cursor: bool,
    pub prefill_selection: bool,
    pub selection_context: bool,
    pub popup_x: Option<f32>,
    pub popup_y: Option<f32>,
    pub ui_lang: String,
    /// The two main languages AI actions work in (codes, see `ai::lang_name`).
    pub lang_a: String,
    pub lang_b: String,
    /// Speech-to-text model for the microphone button.
    pub stt_model: String,
}

/// Per-OS hotkey presets (label, value); the first one is the default.
#[cfg(target_os = "macos")]
pub const HOTKEY_PRESETS: &[(&str, &str)] = &[("Ctrl+Space", "Control+Space"), ("Ctrl+Cmd+Space", "Control+Cmd+Space")];
#[cfg(target_os = "linux")]
pub const HOTKEY_PRESETS: &[(&str, &str)] = &[("Super+Space", "Super+Space"), ("Ctrl+Space", "Control+Space")];
#[cfg(not(any(target_os = "macos", target_os = "linux")))]
pub const HOTKEY_PRESETS: &[(&str, &str)] = &[("Ctrl+Space", "Control+Space"), ("Alt+Space", "Alt+Space")];

impl Default for Settings {
    fn default() -> Self {
        let hotkey = HOTKEY_PRESETS[0].1.to_string();

        Self {
            ai_base_url: "https://api.openai.com/v1".to_string(),
            ai_model: "gpt-4o-mini".to_string(),
            ai_key: String::new(),
            ai_history: Vec::new(),
            search_key: String::new(),
            hotkey,
            width: 680.0,
            height: 150.0,
            translate_from: "auto".to_string(),
            translate_to: "en".to_string(),
            main_language: "en".to_string(),
            hide_on_blur: true,
            follow_cursor: false,
            prefill_selection: false,
            selection_context: true,
            popup_x: None,
            popup_y: None,
            ui_lang: "en".to_string(),
            lang_a: "en".to_string(),
            lang_b: "vi".to_string(),
            stt_model: "whisper-1".to_string(),
        }
    }
}

pub fn get_settings_path() -> PathBuf {
    let base = dirs::config_dir().unwrap_or_else(|| PathBuf::from("."));
    base.join("jink").join("settings.json")
}

pub fn load_settings() -> Settings {
    let path = get_settings_path();
    if let Ok(content) = std::fs::read_to_string(&path) {
        if let Ok(settings) = serde_json::from_str::<Settings>(&content) {
            return settings;
        }
    }

    // Try fallback from legacy Tauri location if exists
    #[cfg(target_os = "macos")]
    {
        if let Some(home) = dirs::home_dir() {
            let legacy = home.join("Library/Application Support/com.nhatcoi.easyinput/settings.json");
            if let Ok(content) = std::fs::read_to_string(&legacy) {
                if let Ok(val) = serde_json::from_str::<serde_json::Value>(&content) {
                    if let Some(s) = val.get("settings") {
                        let mut defaults = Settings::default();
                        if let Some(url) = s.get("aiBaseUrl").and_then(|v| v.as_str()) {
                            defaults.ai_base_url = url.to_string();
                        }
                        if let Some(m) = s.get("aiModel").and_then(|v| v.as_str()) {
                            defaults.ai_model = m.to_string();
                        }
                        if let Some(k) = s.get("aiKey").and_then(|v| v.as_str()) {
                            defaults.ai_key = k.to_string();
                        }
                        let _ = save_settings(&defaults);
                        return defaults;
                    }
                }
            }
        }
    }

    let defaults = Settings::default();
    let _ = save_settings(&defaults);
    defaults
}

pub fn save_settings(settings: &Settings) -> Result<(), String> {
    let path = get_settings_path();
    if let Some(parent) = path.parent() {
        let _ = std::fs::create_dir_all(parent);
    }
    let json = serde_json::to_string_pretty(settings).map_err(|e| e.to_string())?;
    std::fs::write(&path, json).map_err(|e| e.to_string())?;
    Ok(())
}
