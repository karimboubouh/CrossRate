//! Small number/format helpers shared across modules.

use std::time::{SystemTime, UNIX_EPOCH};

pub fn now_ms() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis() as i64)
        .unwrap_or(0)
}

/// Diagnostic line on stderr, enabled with `CROSSRATE_LOG=1`. Finder-launched
/// apps have no terminal, so this stays opt-in and never writes to disk.
pub fn log_line(message: &str) {
    if std::env::var("CROSSRATE_LOG").is_ok() {
        eprintln!("[crossrate] {message}");
    }
}

/// Median of a slice, 0.0 when empty. Used to spot bait ads in a book page.
pub fn median(values: &[f64]) -> f64 {
    if values.is_empty() {
        return 0.0;
    }
    let mut sorted = values.to_vec();
    sorted.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
    let mid = sorted.len() / 2;
    if sorted.len() % 2 == 1 {
        sorted[mid]
    } else {
        (sorted[mid - 1] + sorted[mid]) / 2.0
    }
}

/// Prices in P2P books span several orders of magnitude, so pick decimals by
/// size. Fiat pairs land on 3 decimals (e.g. 2.929 MAD per AED), which is what
/// people actually quote.
pub fn fmt_rate(value: f64) -> String {
    if !value.is_finite() || value <= 0.0 {
        return "-".to_string();
    }
    if value >= 10_000.0 {
        format!("{value:.0}")
    } else if value >= 1_000.0 {
        format!("{value:.1}")
    } else if value >= 100.0 {
        format!("{value:.2}")
    } else if value >= 1.0 {
        format!("{value:.3}")
    } else {
        format!("{value:.4}")
    }
}

/// Human amount for notification copy: 10,000 / 29,130 / 0.2723.
pub fn fmt_amount_grouped(value: f64) -> String {
    if !value.is_finite() {
        return "-".to_string();
    }
    let negative = value < 0.0;
    let abs = value.abs();
    let body = if abs >= 100.0 {
        group_digits(&format!("{:.0}", abs))
    } else if abs >= 1.0 {
        let text = format!("{abs:.2}");
        group_digits(text.trim_end_matches('0').trim_end_matches('.'))
    } else {
        format!("{abs:.4}").trim_end_matches('0').trim_end_matches('.').to_string()
    };
    if negative { format!("-{body}") } else { body }
}

fn group_digits(text: &str) -> String {
    let (int_part, frac) = match text.split_once('.') {
        Some((int_part, frac)) => (int_part, Some(frac)),
        None => (text, None),
    };
    let mut out = String::with_capacity(text.len() + 4);
    for (index, ch) in int_part.chars().enumerate() {
        if index > 0 && (int_part.len() - index) % 3 == 0 {
            out.push(',');
        }
        out.push(ch);
    }
    match frac {
        Some(frac) => format!("{out}.{frac}"),
        None => out,
    }
}

/// Compact amount for the menu bar (max 7 characters).
pub fn fmt_amount_compact(value: f64) -> String {
    let abs = value.abs();
    if abs >= 1_000_000.0 {
        format!("{:.1}M", value / 1_000_000.0)
    } else if abs >= 10_000.0 {
        format!("{:.0}k", value / 1_000.0)
    } else if abs >= 1_000.0 {
        format!("{:.1}k", value / 1_000.0)
    } else {
        format!("{value:.0}")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn median_handles_even_and_odd() {
        assert_eq!(median(&[3.0, 1.0, 2.0]), 2.0);
        assert_eq!(median(&[4.0, 1.0, 3.0, 2.0]), 2.5);
        assert_eq!(median(&[]), 0.0);
    }

    #[test]
    fn rate_formatting_scales() {
        assert_eq!(fmt_rate(2.9287), "2.929");
        assert_eq!(fmt_rate(3.6904), "3.690");
        assert_eq!(fmt_rate(10.85), "10.850");
        assert_eq!(fmt_rate(1234.5), "1234.5");
        assert_eq!(fmt_rate(0.2723), "0.2723");
        assert_eq!(fmt_rate(f64::NAN), "-");
        assert_eq!(fmt_amount_grouped(10_000.0), "10,000");
        assert_eq!(fmt_amount_grouped(29_130.0), "29,130");
        assert_eq!(fmt_amount_grouped(-1_870.0), "-1,870");
        assert_eq!(fmt_amount_grouped(2.5), "2.5");
    }
}
