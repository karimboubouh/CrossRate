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
        Ok(settings) => repair(settings),
        Err(_) => Settings::default(),
    }
}

/// A single bad field must not throw away the user's whole configuration.
fn repair(mut settings: Settings) -> Settings {
    if settings.clone().validate().is_ok() {
        return settings;
    }

    if crate::model::normalize_fiat(&settings.fiat_a).is_none() {
        settings.fiat_a = "AED".to_string();
    }
    if crate::model::normalize_fiat(&settings.fiat_b).is_none() {
        settings.fiat_b = "MAD".to_string();
    }
    if settings.fiat_a == settings.fiat_b {
        settings.fiat_b = if settings.fiat_a == "MAD" { "AED" } else { "MAD" }.to_string();
    }
    if !settings.threshold.is_finite() || settings.threshold <= 0.0 {
        settings.threshold = 2.75;
    }

    settings.validate().unwrap_or_default()
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
