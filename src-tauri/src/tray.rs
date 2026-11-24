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

/// Paint the monochrome glyph with the user's macOS accent colour so the
/// "alerting" state matches the rest of their system.
fn accent_icon() -> Option<Image<'static>> {
    let base = Image::from_bytes(IDLE_PNG).ok()?;
    let (width, height) = (base.width(), base.height());
    let (red, green, blue) = {
        let (light, dark) = crate::appearance::accent_colors();
        let hex = if crate::appearance::is_dark() { dark } else { light };
        let parse = |range: std::ops::Range<usize>| {
            u8::from_str_radix(&hex[range], 16).unwrap_or(0)
        };
        (parse(1..3), parse(3..5), parse(5..7))
    };

    let mut pixels = Vec::with_capacity((width * height * 4) as usize);
    for pixel in base.rgba().chunks_exact(4) {
        let alpha = pixel[3];
        if alpha == 0 {
            pixels.extend_from_slice(&[0, 0, 0, 0]);
        } else {
            pixels.extend_from_slice(&[red, green, blue, alpha]);
        }
    }
    Some(Image::new_owned(pixels, width, height))
}

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
                show_popup_at(app, position);
            }
        })
        .build(app)?;

    Ok(())
}

fn show_window(app: &AppHandle) {
    show_popup(app);
}

/// Convert a tray/monitor `Rect` into physical pixels using the window scale.
fn rect_physical(rect: &tauri::Rect, scale: f64) -> (f64, f64, f64, f64) {
    let (x, y) = match rect.position {
        tauri::Position::Physical(position) => (position.x as f64, position.y as f64),
        tauri::Position::Logical(position) => (position.x * scale, position.y * scale),
    };
    let (width, height) = match rect.size {
        tauri::Size::Physical(size) => (size.width as f64, size.height as f64),
        tauri::Size::Logical(size) => (size.width * scale, size.height * scale),
    };
    (x, y, width, height)
}

fn clamp_to_monitor(
    window: &tauri::WebviewWindow,
    mut x: f64,
    mut y: f64,
    width: f64,
    height: f64,
) -> (f64, f64) {
    if let Ok(Some(monitor)) = window.current_monitor() {
        let origin = monitor.position();
        let extent = monitor.size();
        let scale = window.scale_factor().unwrap_or(2.0);
        // Never overlap the menu bar (~25pt) even if the tray rect is odd.
        let top = origin.y as f64 + 28.0 * scale;
        let left = origin.x as f64 + 8.0;
        let right = origin.x as f64 + extent.width as f64 - width - 8.0;
        let bottom = origin.y as f64 + extent.height as f64 - height - 10.0;
        x = x.clamp(left, right.max(left));
        y = y.clamp(top, bottom.max(top));
    }
    (x, y)
}

fn place_and_show(window: &tauri::WebviewWindow, x: f64, y: f64, origin: &str) {
    let size = window
        .outer_size()
        .unwrap_or(tauri::PhysicalSize::new(408, 724));
    let (width, height) = (size.width as f64, size.height as f64);
    let (x, y) = clamp_to_monitor(window, x, y, width, height);

    crate::util::log_line(&format!(
        "popup at {x:.0},{y:.0} ({origin}) size {width:.0}x{height:.0}"
    ));

    let _ = window.set_position(tauri::Position::Physical(tauri::PhysicalPosition::new(
        x.round() as i32,
        y.round() as i32,
    )));
    let _ = window.show();
    let _ = window.set_focus();
}

/// Tray click: the click event carries the icon's exact position, so use it
/// before falling back to the tray rect or the screen corner.
pub fn show_popup_at(app: &AppHandle, position: tauri::PhysicalPosition<f64>) {
    let Some(window) = app.get_webview_window("main") else {
        return;
    };
    let size = window
        .outer_size()
        .unwrap_or(tauri::PhysicalSize::new(408, 724));
    let width = size.width as f64;
    place_and_show(&window, position.x - width / 2.0, position.y + 8.0, "click");
    let _ = app.emit("crossrate://opened", ());
}

/// Anchor the popup under the menu bar icon. Used for tray clicks, the
/// "Open CrossRate" menu item, first launch and notification clicks — the last
/// one matters because clicking an alert activates the app (Reopen event).
pub fn show_popup(app: &AppHandle) {
    let Some(window) = app.get_webview_window("main") else {
        return;
    };
    let scale = window.scale_factor().unwrap_or(2.0);
    let size = window
        .outer_size()
        .unwrap_or(tauri::PhysicalSize::new(408, 724));
    let width = size.width as f64;

    // Preferred: directly beneath the status item.
    if let Some(tray) = app.tray_by_id(TRAY_ID) {
        if let Ok(Some(rect)) = tray.rect() {
            let (rx, ry, rw, rh) = rect_physical(&rect, scale);
            crate::util::log_line(&format!(
                "tray rect=({rx:.0},{ry:.0},{rw:.0}x{rh:.0}) scale={scale}"
            ));
            if rw > 0.0 && rh > 0.0 {
                place_and_show(&window, rx + rw / 2.0 - width / 2.0, ry + rh + 6.0 * scale, "tray");
                let _ = app.emit("crossrate://opened", ());
                return;
            }
        }
    }

    // Fallback: top-right of the active screen, under the menu bar — never the
    // centre of the display.
    if let Ok(Some(monitor)) = window.current_monitor() {
        let origin = monitor.position();
        let extent = monitor.size();
        let x = origin.x as f64 + extent.width as f64 - width - 14.0;
        let y = origin.y as f64 + 34.0 * scale;
        place_and_show(&window, x, y, "corner");
        let _ = app.emit("crossrate://opened", ());
        return;
    }

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
    let changed = app
        .state::<crate::state::AppState>()
        .tray_alerting
        .swap(alert_visual, std::sync::atomic::Ordering::Relaxed)
        != alert_visual;

    if changed {
        let image = if alert_visual {
            accent_icon().or_else(|| Image::from_bytes(IDLE_PNG).ok())
        } else {
            Image::from_bytes(IDLE_PNG).ok()
        };
        if let Some(image) = image {
            let _ = tray.set_icon(Some(image));
            // Template mode lets macOS tint the idle glyph per appearance.
            let _ = tray.set_icon_as_template(!alert_visual);
        }
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
