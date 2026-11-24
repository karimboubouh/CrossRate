//! Menu bar item: live cross-rate title, alert icon and the context menu.

use crate::model::Snapshot;
use crate::state::AppState;
use crate::util::fmt_rate;
use tauri::image::Image;
use tauri::menu::{Menu, MenuItem, PredefinedMenuItem};
use tauri::tray::{MouseButton, MouseButtonState, TrayIconBuilder, TrayIconEvent};
use tauri::{AppHandle, Emitter, Manager};

pub const TRAY_ID: &str = "crossrate-tray";

const IDLE_PNG: &[u8] = include_bytes!("../icons/tray-idle.png");
const ALERT_PNG: &[u8] = include_bytes!("../icons/tray-alert.png");

pub fn build(app: &AppHandle) -> tauri::Result<()> {
    let open = MenuItem::with_id(app, "open", "Open CrossRate", true, None::<&str>)?;
    let refresh = MenuItem::with_id(app, "refresh", "Refresh now", true, None::<&str>)?;
    let settings = MenuItem::with_id(app, "settings", "Settings…", true, None::<&str>)?;
    let separator = PredefinedMenuItem::separator(app)?;
    let quit = MenuItem::with_id(app, "quit", "Quit CrossRate", true, None::<&str>)?;
    let menu = Menu::with_items(app, &[&open, &refresh, &settings, &separator, &quit])?;

    TrayIconBuilder::with_id(TRAY_ID)
        .icon(Image::from_bytes(IDLE_PNG)?)
        .icon_as_template(true)
        .tooltip("CrossRate · waiting for data")
        .menu(&menu)
        .show_menu_on_left_click(false)
        .on_menu_event(|app, event| match event.id.as_ref() {
            "open" => show_window(app),
            "settings" => {
                show_window(app);
                let _ = app.emit("crossrate://open-settings", ());
            }
            "refresh" => {
                if let Some(state) = app.try_state::<AppState>() {
                    state.wake.notify_waiters();
                }
            }
            "quit" => app.exit(0),
            _ => {}
        })
        .on_tray_icon_event(|tray, event| {
            if let TrayIconEvent::Click {
                button: MouseButton::Left,
                button_state: MouseButtonState::Up,
                position,
                ..
            } = event
            {
                let app = tray.app_handle();
                show_window_at(app, position);
            }
        })
        .build(app)?;

    Ok(())
}

fn show_window(app: &AppHandle) {
    if let Some(window) = app.get_webview_window("main") {
        let _ = window.show();
        let _ = window.set_focus();
    }
}

/// Anchor the popup under the tray icon, clamped to the active screen.
fn show_window_at(app: &AppHandle, position: tauri::PhysicalPosition<f64>) {
    let Some(window) = app.get_webview_window("main") else {
        return;
    };

    let size = window
        .outer_size()
        .unwrap_or(tauri::PhysicalSize::new(408, 724));
    let width = size.width as f64;
    let height = size.height as f64;

    let mut x = position.x - width / 2.0;
    let mut y = position.y + 8.0;

    if let Ok(Some(monitor)) = window.current_monitor() {
        let origin = monitor.position();
        let extent = monitor.size();
        let left = origin.x as f64 + 8.0;
        let right = origin.x as f64 + extent.width as f64 - width - 8.0;
        let bottom = origin.y as f64 + extent.height as f64 - height - 12.0;
        x = x.clamp(left, right.max(left));
        if y > bottom {
            y = bottom;
        }
    }

    let _ = window.set_position(tauri::Position::Physical(tauri::PhysicalPosition::new(
        x.round() as i32,
        y.round() as i32,
    )));
    let _ = window.show();
    let _ = window.set_focus();
    let _ = app.emit("crossrate://opened", ());
}

/// Refresh the menu bar title + icon from a snapshot.
pub fn update(app: &AppHandle, snapshot: &Snapshot) {
    let Some(tray) = app.tray_by_id(TRAY_ID) else {
        return;
    };

    let title = snapshot
        .j
        .map(fmt_rate)
        .filter(|text| text != "-")
        .unwrap_or_default();
    let _ = tray.set_title(if title.is_empty() {
        None
    } else {
        Some(title.clone())
    });

    let alert_visual = snapshot.alert_active && snapshot.alert_badge;
    let png = if alert_visual { ALERT_PNG } else { IDLE_PNG };
    if let Ok(image) = Image::from_bytes(png) {
        let _ = tray.set_icon(Some(image));
        let _ = tray.set_icon_as_template(!alert_visual);
    }

    let tooltip = match snapshot.j {
        Some(j) => format!(
            "CrossRate · {} → {} · {}{}",
            snapshot.fiat_a,
            snapshot.fiat_b,
            fmt_rate(j),
            if snapshot.alert_active { " · ALERT" } else { "" }
        ),
        None => format!(
            "CrossRate · {} → {} · waiting for data",
            snapshot.fiat_a, snapshot.fiat_b
        ),
    };
    let _ = tray.set_tooltip(Some(tooltip));
}
