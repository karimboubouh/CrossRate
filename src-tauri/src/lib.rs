//! CrossRate — Binance P2P cross-rate monitor for the macOS menu bar.

mod alerts;
mod appearance;
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
            commands::set_window_height,
            commands::set_appearance,
            commands::get_accent,
        ])
        .setup(|app| {
            #[cfg(target_os = "macos")]
            app.set_activation_policy(tauri::ActivationPolicy::Accessory);

            // Join every Space (incl. full-screen apps) from the first launch.
            if let Some(window) = app.get_webview_window("main") {
                tray::float_over_full_screen(&window);
            }

            // Native popover material behind the transparent webview, so the
            // panel picks up the desktop and both appearances for free.
            #[cfg(target_os = "macos")]
            if let Some(window) = app.get_webview_window("main") {
                use window_vibrancy::{apply_vibrancy, NSVisualEffectMaterial, NSVisualEffectState};
                let _ = apply_vibrancy(
                    &window,
                    NSVisualEffectMaterial::Popover,
                    Some(NSVisualEffectState::Active),
                    Some(12.0),
                );
            }

            let handle = app.handle().clone();
            let first_run = !settings::path(&handle).exists();
            let settings = settings::load(&handle);
            let wants_autostart = settings.launch_at_login;

            // Match the native material to the saved theme from the first frame.
            if let Some(window) = app.get_webview_window("main") {
                let _ = window.set_theme(match settings.theme.as_str() {
                    "light" => Some(tauri::Theme::Light),
                    "dark" => Some(tauri::Theme::Dark),
                    _ => None,
                });
            }

            app.manage(state::AppState::new(settings.clone()));
            tray::build(&handle)?;
            commands::apply_autostart(&handle, wants_autostart);

            if first_run {
                // Anchored under the status item (falls back to the top-right
                // corner), never the middle of the display.
                tray::show_popup(&handle);
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
        .run(|app, event| match event {
            // macOS calls applicationShouldHandleReopen when the app is
            // activated with no visible windows — that is what happens when the
            // user clicks a CrossRate notification.
            tauri::RunEvent::Reopen { .. } => {
                crate::util::log_line("reopen -> showing popup");
                tray::show_popup(app);
            }
            tauri::RunEvent::ExitRequested { api, code, .. } => {
                // Closing the popup must not quit a menu bar app.
                if code.is_none() {
                    api.prevent_exit();
                }
            }
            _ => {}
        });
}
