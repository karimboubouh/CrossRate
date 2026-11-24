//! CrossRate — Binance P2P cross-rate monitor for the macOS menu bar.

mod alerts;
mod binance;
mod commands;
mod engine;
mod filters;
mod fx;
mod model;
mod settings;
mod state;
mod tray;
mod util;

use tauri::Manager;
use tauri_plugin_autostart::MacosLauncher;

pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_notification::init())
        .plugin(tauri_plugin_autostart::init(MacosLauncher::LaunchAgent, None))
        .invoke_handler(tauri::generate_handler![
            commands::get_settings,
            commands::get_snapshot,
            commands::save_settings,
            commands::refresh_now,
            commands::discover_pay_methods,
            commands::list_fiats,
            commands::open_advertiser,
            commands::play_sound_cmd,
            commands::test_alert,
            commands::hide_window,
            commands::quit_app,
        ])
        .setup(|app| {
            #[cfg(target_os = "macos")]
            app.set_activation_policy(tauri::ActivationPolicy::Accessory);

            let handle = app.handle().clone();
            let first_run = !settings::path(&handle).exists();
            let settings = settings::load(&handle);
            let wants_autostart = settings.launch_at_login;

            app.manage(state::AppState::new(settings));
            tray::build(&handle)?;
            commands::apply_autostart(&handle, wants_autostart);

            if first_run {
                if let Some(window) = app.get_webview_window("main") {
                    let _ = window.show();
                    let _ = window.set_focus();
                }
            }

            if let Some(window) = app.get_webview_window("main") {
                let window_handle = window.clone();
                window.on_window_event(move |event| match event {
                    tauri::WindowEvent::Focused(false) => {
                        // Hidden automatically like a native popover.
                        if std::env::var("CROSSRATE_NO_AUTOHIDE").is_err() {
                            let _ = window_handle.hide();
                        }
                    }
                    tauri::WindowEvent::CloseRequested { api, .. } => {
                        api.prevent_close();
                        let _ = window_handle.hide();
                    }
                    _ => {}
                });
            }

            let engine_handle = handle.clone();
            tauri::async_runtime::spawn(engine::run(engine_handle));
            Ok(())
        })
        .build(tauri::generate_context!())
        .expect("failed to build CrossRate")
        .run(|_app, event| {
            if let tauri::RunEvent::ExitRequested { api, code, .. } = event {
                // Closing the popup must not quit a menu bar app.
                if code.is_none() {
                    api.prevent_exit();
                }
            }
        });
}
