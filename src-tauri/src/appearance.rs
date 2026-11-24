//! Reads the user's macOS accent colour so the UI matches System Settings
//! instead of imposing a brand palette.

use std::process::Command;

/// `(light, dark)` pair matching `NSColor.controlAccentColor`.
pub fn accent_colors() -> (String, String) {
    let index = Command::new("defaults")
        .args(["read", "-g", "AppleAccentColor"])
        .output()
        .ok()
        .filter(|output| output.status.success())
        .and_then(|output| String::from_utf8(output.stdout).ok())
        .and_then(|text| text.trim().parse::<i32>().ok());

    let (light, dark) = match index {
        Some(-1) => ("#8e8e93", "#98989d"), // graphite
        Some(0) => ("#ff3b30", "#ff453a"),  // red
        Some(1) => ("#ff9500", "#ff9f0a"),  // orange
        Some(2) => ("#ffcc00", "#ffd60a"),  // yellow
        Some(3) => ("#34c759", "#30d158"),  // green
        Some(5) => ("#af52de", "#bf5af2"),  // purple
        Some(6) => ("#ff2d55", "#ff375f"),  // pink
        // 4 (blue) and anything unset: the macOS default.
        _ => ("#007aff", "#0a84ff"),
    };
    (light.to_string(), dark.to_string())
}

/// Whether the system is currently in dark appearance.
pub fn is_dark() -> bool {
    Command::new("defaults")
        .args(["read", "-g", "AppleInterfaceStyle"])
        .output()
        .map(|output| String::from_utf8_lossy(&output.stdout).to_lowercase().contains("dark"))
        .unwrap_or(false)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn accents_are_valid_hex() {
        let (light, dark) = accent_colors();
        for value in [light, dark] {
            assert_eq!(value.len(), 7, "{value}");
            assert!(value.starts_with('#'));
            assert!(value[1..].chars().all(|c| c.is_ascii_hexdigit()));
        }
    }
}
