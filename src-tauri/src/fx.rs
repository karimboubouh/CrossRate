//! Mid-market FX rate from a keyless public API, with a local cache.

use crate::util::now_ms;
use serde::Deserialize;

pub const FX_URL: &str = "https://open.er-api.com/v6/latest";
/// Refresh the mid rate every 30 minutes; it is a reference number, not a tick.
pub const FX_TTL_SECS: i64 = 1_800;
/// Back off for two minutes after a failed FX lookup.
const FX_RETRY_MS: i64 = 120_000;

#[derive(Debug)]
pub struct FxState {
    pub mid: Option<f64>,
    pub source: String,
    pub fetched_at: i64,
    pub last_error: Option<String>,
    pub next_attempt_at: i64,
}

impl Default for FxState {
    fn default() -> Self {
        Self {
            mid: None,
            source: "unavailable".to_string(),
            fetched_at: 0,
            last_error: None,
            next_attempt_at: 0,
        }
    }
}

#[derive(Deserialize)]
struct ApiResponse {
    #[serde(default)]
    result: String,
    #[serde(default)]
    rates: std::collections::HashMap<String, f64>,
}

/// Fetch the rate of 1 `from` expressed in `to`.
pub async fn fetch_mid(client: &reqwest::Client, from: &str, to: &str) -> Result<f64, String> {
    let url = format!("{FX_URL}/{}", from.trim().to_ascii_uppercase());
    let response = client
        .get(&url)
        .send()
        .await
        .map_err(|e| format!("FX request failed: {e}"))?;
    if !response.status().is_success() {
        return Err(format!("FX provider returned HTTP {}", response.status()));
    }
    let text = response
        .text()
        .await
        .map_err(|e| format!("Could not read FX response: {e}"))?;
    parse_mid(&text, to)
}

pub fn parse_mid(text: &str, to: &str) -> Result<f64, String> {
    let parsed: ApiResponse =
        serde_json::from_str(text).map_err(|e| format!("FX response was not valid JSON: {e}"))?;
    if parsed.result != "success" {
        return Err("FX provider did not return a success result".to_string());
    }
    let rate = parsed
        .rates
        .get(&to.trim().to_ascii_uppercase())
        .copied()
        .ok_or_else(|| format!("FX provider has no {to} rate"))?;
    if !rate.is_finite() || rate <= 0.0 {
        return Err("FX provider returned an invalid rate".to_string());
    }
    Ok(rate)
}

/// Returns `(mid, source, age_seconds)`, refreshing from the network when stale.
/// A manual override in settings always wins.
pub async fn resolve(
    client: &reqwest::Client,
    state: &mut FxState,
    from: &str,
    to: &str,
    manual_override: Option<f64>,
) -> (Option<f64>, String, i64) {
    if let Some(manual) = manual_override {
        return (Some(manual), "manual".to_string(), 0);
    }

    let now = now_ms();
    let age_secs = if state.fetched_at > 0 {
        ((now - state.fetched_at) / 1000).max(0)
    } else {
        i64::MAX
    };

    let fresh = state.mid.is_some() && age_secs < FX_TTL_SECS;
    let backed_off = state.mid.is_some() && now < state.next_attempt_at;
    if fresh || backed_off {
        let source = if age_secs < FX_TTL_SECS * 4 {
            state.source.clone()
        } else {
            format!("{} (cached)", state.source)
        };
        return (state.mid, source, age_secs);
    }

    match fetch_mid(client, from, to).await {
        Ok(rate) => {
            state.mid = Some(rate);
            state.source = "ecb-er-api".to_string();
            state.fetched_at = now;
            state.last_error = None;
            state.next_attempt_at = 0;
            (state.mid, state.source.clone(), 0)
        }
        Err(error) => {
            state.last_error = Some(error);
            state.next_attempt_at = now + FX_RETRY_MS;
            let age = if state.fetched_at > 0 {
                ((now - state.fetched_at) / 1000).max(0)
            } else {
                0
            };
            (
                state.mid,
                if state.mid.is_some() {
                    format!("{} (stale)", state.source)
                } else {
                    "unavailable".to_string()
                },
                age,
            )
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const SAMPLE: &str = r#"{"result":"success","provider":"https://www.exchangerate-api.com","docs":"https://www.exchangerate-api.com/docs/free","time_last_update_unix":1795507201,"time_last_update_utc":"Tue, 24 Nov 2026 00:00:01 +0000","base_code":"AED","rates":{"AED":1,"USD":0.272294,"MAD":2.726018,"EUR":0.235225}}"#;

    #[test]
    fn parses_mid_rate() {
        assert!((parse_mid(SAMPLE, "MAD").unwrap() - 2.726018).abs() < 1e-9);
        assert!((parse_mid(SAMPLE, "usd").unwrap() - 0.272294).abs() < 1e-9);
    }

    #[test]
    fn rejects_unknown_currency() {
        let error = parse_mid(SAMPLE, "XYZ").unwrap_err();
        assert!(error.contains("XYZ"), "{error}");
    }

    #[test]
    fn rejects_error_payload() {
        assert!(parse_mid(r#"{"result":"error","rates":{}}"#, "MAD").is_err());
        assert!(parse_mid("not json", "MAD").is_err());
    }
}
