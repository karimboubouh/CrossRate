//! Native notifications and alert sounds.

use crate::model::SOUNDS;
use tauri::AppHandle;

pub fn play_sound(name: &str) {
    let clean = if SOUNDS.contains(&name) {
        name
    } else {
        "Glass"
    };
    let path = format!("/System/Library/Sounds/{clean}.aiff");
    let _ = std::process::Command::new("afplay").arg(path).spawn();
}

/// Send a native notification. Falls back to AppleScript when the app is not
/// allowed to use the notification centre (e.g. unsigned dev builds).
pub fn notify(app: &AppHandle, title: &str, body: &str) -> bool {
    use tauri_plugin_notification::NotificationExt;
    match app
        .notification()
        .builder()
        .title(title)
        .body(body)
        .show()
    {
        Ok(()) => true,
        Err(_) => {
            osascript_notify(title, body);
            false
        }
    }
}

pub fn osascript_notify(title: &str, body: &str) {
    let script = format!(
        "display notification \"{}\" with title \"{}\"",
        escape(body),
        escape(title)
    );
    let _ = std::process::Command::new("osascript")
        .args(["-e", &script])
        .spawn();
}

fn escape(text: &str) -> String {
    text.replace('\\', "\\\\").replace('"', "\\\"")
}
