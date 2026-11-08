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
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            ai_base_url: String::new(),
            ai_model: String::new(),
            ai_key: String::new(),
            ai_history: Vec::new(),
            search_key: String::new(),
            hotkey: "Alt+Space".to_string(),
            width: 680.0,
            height: 150.0,
            translate_from: "auto".to_string(),
            translate_to: "en".to_string(),
            main_language: "en".to_string(),
            hide_on_blur: true,
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
    Settings::default()
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

