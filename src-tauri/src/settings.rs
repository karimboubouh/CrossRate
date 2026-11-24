//! Reading and writing the settings JSON file.

use crate::model::Settings;
use std::fs;
use std::path::PathBuf;
use tauri::Manager;

/// Allows tests and headless runs to redirect the config directory.
pub fn dir(app: &tauri::AppHandle) -> PathBuf {
    if let Ok(custom) = std::env::var("CROSSRATE_CONFIG_DIR") {
        if !custom.trim().is_empty() {
            return PathBuf::from(custom);
        }
    }
    app.path()
        .app_config_dir()
        .unwrap_or_else(|_| PathBuf::from("."))
}

pub fn path(app: &tauri::AppHandle) -> PathBuf {
    dir(app).join("settings.json")
}

pub fn load(app: &tauri::AppHandle) -> Settings {
    let raw = match fs::read(path(app)) {
        Ok(bytes) => bytes,
        Err(_) => return Settings::default(),
    };
    match serde_json::from_slice::<Settings>(&raw) {
        Ok(settings) => settings.validate().unwrap_or_default(),
        Err(_) => Settings::default(),
    }
}

pub fn save(app: &tauri::AppHandle, settings: &Settings) -> Result<(), String> {
    let dir = dir(app);
    fs::create_dir_all(&dir).map_err(|e| format!("Could not create config folder: {e}"))?;
    let final_path = dir.join("settings.json");
    let tmp_path = dir.join("settings.json.tmp");
    let json = serde_json::to_vec_pretty(settings).map_err(|e| e.to_string())?;
    fs::write(&tmp_path, json).map_err(|e| format!("Could not write settings: {e}"))?;
    fs::rename(&tmp_path, &final_path).map_err(|e| format!("Could not save settings: {e}"))?;
    Ok(())
}
