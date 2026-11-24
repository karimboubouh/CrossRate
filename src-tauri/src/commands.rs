//! IPC commands exposed to the React frontend.

use crate::model::*;
use crate::settings as settings_store;
use crate::state::AppState;
use crate::util::now_ms;
use tauri::{AppHandle, Emitter, State};

#[tauri::command]
pub async fn get_settings(state: State<'_, AppState>) -> Result<Settings, String> {
    Ok(state.settings.read().await.clone())
}

#[tauri::command]
pub async fn get_snapshot(state: State<'_, AppState>) -> Result<Option<Snapshot>, String> {
    Ok(state.snapshot.read().await.clone())
}

#[tauri::command]
pub async fn save_settings(
    app: AppHandle,
    state: State<'_, AppState>,
    settings: Settings,
) -> Result<Settings, String> {
    let previous = state.settings.read().await.clone();
    let validated = settings.validate()?;
    settings_store::save(&app, &validated)?;

    *state.settings.write().await = validated.clone();
    state
        .settings_rev
        .fetch_add(1, std::sync::atomic::Ordering::Relaxed);

    {
        let mut alert = state.alert.lock().await;
        alert.active = false;
        alert.since = None;
    }

    if previous.launch_at_login != validated.launch_at_login {
        apply_autostart(&app, validated.launch_at_login);
    }

    let _ = app.emit("crossrate://settings", &validated);
    state.wake.notify_waiters();
    Ok(validated)
}

#[tauri::command]
pub async fn refresh_now(state: State<'_, AppState>) -> Result<(), String> {
    state.wake.notify_waiters();
    Ok(())
}

#[tauri::command]
pub async fn discover_pay_methods(
    state: State<'_, AppState>,
    fiat: String,
    side: String,
) -> Result<Vec<String>, String> {
    let code = normalize_fiat(&fiat).ok_or_else(|| "Invalid currency code".to_string())?;
    let leg = LegSide::parse(&side).ok_or_else(|| "Invalid side".to_string())?;
    let key = format!("{code}-{}", leg.trade_type());
    let now = now_ms();

    {
        let cache = state.pay_cache.lock().await;
        if let Some((cached_at, methods)) = cache.get(&key) {
            if now - cached_at < 10 * 60 * 1000 {
                return Ok(methods.clone());
            }
        }
    }

    let methods = crate::binance::discover_pay_methods(&state.http, &code, leg).await?;
    state
        .pay_cache
        .lock()
        .await
        .insert(key, (now, methods.clone()));
    Ok(methods)
}

#[tauri::command]
pub fn list_fiats() -> Vec<FiatInfo> {
    fiat_list()
}

#[tauri::command]
pub fn open_advertiser(user_no: String) -> Result<(), String> {
    if user_no.is_empty() || !user_no.chars().all(|c| c.is_ascii_alphanumeric()) {
        return Err("Invalid advertiser id".to_string());
    }
    let url = format!("https://p2p.binance.com/en/advertiserDetail?advertiser_no={user_no}");
    std::process::Command::new("open")
        .arg(url)
        .spawn()
        .map_err(|e| format!("Could not open the browser: {e}"))?;
    Ok(())
}

#[tauri::command]
pub fn play_sound_cmd(name: String) {
    crate::alerts::play_sound(&name);
}

/// Sends a notification with the exact production copy, so the user can judge
/// the wording without waiting for a real opportunity.
#[tauri::command]
pub async fn test_alert(
    app: AppHandle,
    state: State<'_, AppState>,
    sound: String,
) -> Result<(), String> {
    crate::alerts::play_sound(&sound);
    let settings = state.settings.read().await.clone();
    let snapshot = state.snapshot.read().await.clone();
    let (title, body) = match snapshot {
        Some(snapshot) if snapshot.j.is_some() => crate::engine::alert_text(&snapshot, &settings),
        _ => (
            format!("{} → {} · waiting for data", settings.fiat_a, settings.fiat_b),
            "No live market data yet — the alert will look like this once quotes load.".to_string(),
        ),
    };
    crate::alerts::notify(&app, &title, &body);
    Ok(())
}

#[tauri::command]
pub fn hide_window(window: tauri::WebviewWindow) {
    let _ = window.hide();
}

#[tauri::command]
pub fn quit_app(app: AppHandle) {
    app.exit(0);
}

pub fn apply_autostart(app: &AppHandle, enabled: bool) {
    use tauri_plugin_autostart::ManagerExt;
    let autolaunch = app.autolaunch();
    if enabled {
        let _ = autolaunch.enable();
    } else {
        let _ = autolaunch.disable();
    }
}
