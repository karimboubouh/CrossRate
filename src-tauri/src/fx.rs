//! Mid-market FX rate from a keyless public API, with a local cache.
//!
//! The cache is keyed by currency pair: switching A→B must never reuse the
//! previous pair's rate as the current "bank rate".

use crate::util::now_ms;
use serde::Deserialize;

pub const FX_URL: &str = "https://open.er-api.com/v6/latest";
/// Refresh the mid rate every 30 minutes; it is a reference number, not a tick.
pub const FX_TTL_SECS: i64 = 1_800;
/// Back off for two minutes after a failed FX lookup.
const FX_RETRY_MS: i64 = 120_000;

#[derive(Debug, Clone, Default)]
pub struct FxState {
    /// Which pair the cached rate belongs to.
    pub pair: Option<(String, String)>,
    pub mid: Option<f64>,
    pub source: String,
    pub fetched_at: i64,
    pub last_error: Option<String>,
    pub next_attempt_at: i64,
}

impl FxState {
    fn matches(&self, from: &str, to: &str) -> bool {
        self.pair
            .as_ref()
            .is_some_and(|(cached_from, cached_to)| cached_from == from && cached_to == to)
    }

    fn age_secs(&self, now: i64) -> i64 {
        if self.fetched_at > 0 {
            ((now - self.fetched_at) / 1000).max(0)
        } else {
            i64::MAX
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

/// Resolve the mid rate for `from → to`.
///
/// Returns `(state_to_store, mid, source, age_secs)`. The caller owns the shared
/// state, so no lock is held across the network call.
pub async fn resolve(
    client: &reqwest::Client,
    state: &FxState,
    from: &str,
    to: &str,
    manual_override: Option<f64>,
) -> (FxState, Option<f64>, String, i64) {
    if let Some(manual) = manual_override {
        return (state.clone(), Some(manual), "manual".to_string(), 0);
    }

    let now = now_ms();
    let same_pair = state.matches(from, to);
    let age = if same_pair { state.age_secs(now) } else { i64::MAX };
    let fresh = same_pair && state.mid.is_some() && age < FX_TTL_SECS;
    let backed_off = now < state.next_attempt_at;

    // Serve the cached rate while it is fresh, or while we are backing off.
    if fresh || (backed_off && same_pair && state.mid.is_some()) {
        let source = if age < FX_TTL_SECS * 4 {
            state.source.clone()
        } else {
            format!("{} (cached)", state.source)
        };
        return (state.clone(), state.mid, source, age);
    }

    // Honour the retry backoff even when nothing is cached, so a down FX
    // provider cannot stall every poll.
    if backed_off {
        let source = if same_pair && state.mid.is_some() {
            format!("{} (stale)", state.source)
        } else {
            "unavailable".to_string()
        };
        return (state.clone(), state.mid.filter(|_| same_pair), source, age);
    }

    match fetch_mid(client, from, to).await {
        Ok(rate) => {
            let next = FxState {
                pair: Some((from.to_string(), to.to_string())),
                mid: Some(rate),
                source: "ecb-er-api".to_string(),
                fetched_at: now,
                last_error: None,
                next_attempt_at: 0,
            };
            (next, Some(rate), "ecb-er-api".to_string(), 0)
        }
        Err(error) => {
            let mut next = state.clone();
            next.last_error = Some(error);
            next.next_attempt_at = now + FX_RETRY_MS;
            if same_pair && next.mid.is_some() {
                let source = format!("{} (stale)", next.source);
                (next, state.mid, source, age)
            } else {
                next.pair = None;
                next.mid = None;
                next.fetched_at = 0;
                next.source = "unavailable".to_string();
                (next, None, "unavailable".to_string(), 0)
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const SAMPLE: &str = r#"{"result":"success","provider":"https://www.exchangerate-api.com","docs":"https://www.exchangerate-api.com/docs/free","time_last_update_unix":1795507201,"time_last_update_utc":"Tue, 24 Nov 2026 00:00:01 +0000","base_code":"AED","rates":{"AED":1,"USD":0.272294,"MAD":2.726018,"EUR":0.235225}}"#;

    fn cached(pair: (&str, &str), mid: f64) -> FxState {
        FxState {
            pair: Some((pair.0.to_string(), pair.1.to_string())),
            mid: Some(mid),
            source: "ecb-er-api".to_string(),
            fetched_at: now_ms(),
            last_error: None,
            next_attempt_at: 0,
        }
    }

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

    #[tokio::test]
    async fn reuses_cache_only_for_the_same_pair() {
        let client = reqwest::Client::new();
        let state = cached(("AED", "MAD"), 2.726);

        // Same pair, fresh: served from cache, no network.
        let (_, mid, source, age) = resolve(&client, &state, "AED", "MAD", None).await;
        assert_eq!(mid, Some(2.726));
        assert_eq!(source, "ecb-er-api");
        assert!(age < 5);

        // Different pair: the cached MAD rate must NOT be reported as USD.
        assert!(!state.matches("AED", "USD"));
    }

    #[test]
    fn manual_override_wins() {
        let state = cached(("AED", "MAD"), 2.726);
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .unwrap();
        let (_, mid, source, _) =
            runtime.block_on(resolve(&reqwest::Client::new(), &state, "AED", "MAD", Some(2.8)));
        assert_eq!(mid, Some(2.8));
        assert_eq!(source, "manual");
    }
}
