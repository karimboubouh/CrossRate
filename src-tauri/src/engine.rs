//! The polling engine: fetch both legs, filter, compute the cross rate, drive
//! alerts, the tray badge and the UI.

use crate::alerts;
use crate::binance::{self, Query};
use crate::filters;
use crate::fx;
use crate::model::*;
use crate::state::AppState;
use crate::tray;
use crate::util::now_ms;
use std::time::{Duration, Instant};
use tauri::{AppHandle, Emitter, Manager};

pub const SNAPSHOT_EVENT: &str = "crossrate://snapshot";

pub async fn run(app: AppHandle) {
    // Give the webview a moment to attach its event listeners.
    tokio::time::sleep(Duration::from_millis(500)).await;
    let mut consecutive_failures: u32 = 0;

    loop {
        let started = Instant::now();
        let state = app.state::<AppState>();
        let settings = state.settings.read().await.clone();
        let rev = state
            .settings_rev
            .load(std::sync::atomic::Ordering::Relaxed);

        let (mid, mid_source, mid_age) = {
            let mut fx_state = state.fx.write().await;
            fx::resolve(
                &state.http,
                &mut fx_state,
                &settings.fiat_a,
                &settings.fiat_b,
                settings.mid_override,
            )
            .await
        };

        let (buy_view, sell_view) = tokio::join!(
            fetch_leg(&state.http, &settings, LegSide::Buy),
            fetch_leg(&state.http, &settings, LegSide::Sell)
        );

        let now = now_ms();
        let next_poll_ms = now + (settings.refresh_secs as i64 * 1000);
        let mut snapshot = compose(
            &settings,
            rev,
            buy_view,
            sell_view,
            mid,
            mid_source,
            mid_age,
            next_poll_ms,
            now,
        );

        let poll_failed = snapshot.buy.best.is_none() && snapshot.sell.best.is_none();
        let previous = state.snapshot.read().await.clone();
        let mut fresh_alert = false;

        if poll_failed {
            snapshot.stale = true;
            if let Some(previous) = previous {
                if previous.j.is_some() {
                    // Keep showing the last good market while surfacing the error.
                    snapshot.buy = previous.buy;
                    snapshot.sell = previous.sell;
                    snapshot.j = previous.j;
                    snapshot.j_second = previous.j_second;
                    snapshot.fragile = previous.fragile;
                    snapshot.feasible = previous.feasible;
                    snapshot.rate_met = previous.rate_met;
                    snapshot.spread_pct = previous.spread_pct;
                }
            }
            let alert = state.alert.lock().await;
            snapshot.alert_active = alert.active;
            snapshot.alert_since = alert.since;
            snapshot.alert_count = alert.count;
        } else {
            let mut alert = state.alert.lock().await;
            let was_active = alert.active;
            let active = snapshot.alert_active;
            if active && !was_active {
                alert.since = Some(now);
                alert.count = alert.count.saturating_add(1);
            }
            if !active {
                alert.since = None;
            }
            alert.active = active;
            fresh_alert = active && !was_active;
            snapshot.alert_active = active;
            snapshot.alert_since = alert.since;
            snapshot.alert_count = alert.count;
        }

        if fresh_alert {
            let (title, body) = alert_text(&snapshot, &settings);
            if settings.alert_notification {
                alerts::notify(&app, &title, &body);
            }
            if settings.alert_sound {
                alerts::play_sound(&settings.sound_name);
            }
        }

        *state.snapshot.write().await = Some(snapshot.clone());
        let _ = app.emit(SNAPSHOT_EVENT, &snapshot);
        tray::update(&app, &snapshot);

        if poll_failed {
            consecutive_failures = consecutive_failures.saturating_add(1);
        } else {
            consecutive_failures = 0;
        }

        let base_ms = settings.refresh_secs * 1000;
        let backoff_ms = if consecutive_failures > 0 {
            (1u64 << consecutive_failures.min(4)) * 1000
        } else {
            0
        };
        let interval_ms = base_ms.max(backoff_ms).min(120_000);
        let elapsed_ms = started.elapsed().as_millis() as u64;
        let sleep_ms = interval_ms.saturating_sub(elapsed_ms).max(1_200);

        tokio::select! {
            _ = tokio::time::sleep(Duration::from_millis(sleep_ms)) => {}
            _ = state.wake.notified() => {}
        }
    }
}

async fn fetch_leg(client: &reqwest::Client, settings: &Settings, side: LegSide) -> LegView {
    let side_filters = side.filters(settings);
    let favorites = side.favorites(settings);
    let band = side.amount_band(settings);
    let fiat = side.fiat(settings);

    let mut page = 1u32;
    let mut fetched = 0usize;
    let mut total = 0i64;
    let mut pay_methods: Vec<String> = Vec::new();
    let mut collected: Vec<Offer> = Vec::new();
    let mut error: Option<String> = None;

    loop {
        let query = Query {
            fiat: &fiat,
            side,
            page,
            rows: PAGE_ROWS,
            publisher: side_filters.publisher,
            pay_types: &side_filters.pay_types,
            favorites,
            outlier_pct: settings.outlier_pct,
        };
        match binance::search(client, &query).await {
            Ok(raw) => {
                total = raw.total;
                fetched += raw.offers.len();
                if pay_methods.is_empty() {
                    pay_methods = raw.pay_methods;
                }
                let mut kept = filters::apply(raw.offers, side, side_filters, band);
                let want_more =
                    kept.is_empty() && total > (page * PAGE_ROWS) as i64 && page < 3;
                collected.append(&mut kept);
                if want_more {
                    page += 1;
                    continue;
                }
                break;
            }
            Err(err) => {
                error = Some(err);
                break;
            }
        }
    }

    if collected.len() > 1 {
        filters::sort(&mut collected, side);
        filters::dedupe_by_user(&mut collected);
    }

    let passed = collected.len();
    let best = collected.first().cloned();
    let offers: Vec<Offer> = collected.into_iter().take(MAX_OFFERS_SHOWN).collect();

    LegView {
        side,
        fiat,
        best,
        offers,
        fetched,
        passed,
        total,
        error,
        pay_methods,
    }
}

#[allow(clippy::too_many_arguments)]
fn compose(
    settings: &Settings,
    rev: u64,
    buy: LegView,
    sell: LegView,
    mid: Option<f64>,
    mid_source: String,
    mid_age: i64,
    next_poll_ms: i64,
    now: i64,
) -> Snapshot {
    let best_buy = buy.best.clone();
    let best_sell = sell.best.clone();

    let (j, feasible, rate_met, alert_active, fragile, j_second) =
        match (best_buy.as_ref(), best_sell.as_ref()) {
            (Some(buy_offer), Some(sell_offer)) => {
                let j = sell_offer.price / buy_offer.price;
                let feasible = filters::feasibility(buy_offer, sell_offer, settings);
                let rate_met = j >= settings.threshold;
                let alert_active = rate_met && feasible.ok;
                let (j_second, fragile) = runner_up(&buy, &sell, settings.threshold, alert_active);
                (
                    Some(j),
                    feasible,
                    rate_met,
                    alert_active,
                    fragile,
                    j_second,
                )
            }
            _ => (
                None,
                Feasible::none("Waiting for matching offers on both legs"),
                false,
                false,
                false,
                None,
            ),
        };

    let spread_pct = match (j, mid) {
        (Some(j), Some(mid)) if mid > 0.0 => Some((j / mid - 1.0) * 100.0),
        _ => None,
    };

    let error = match (&buy.error, &sell.error) {
        (Some(a), Some(b)) => Some(format!("{a} · {b}")),
        (Some(a), None) => Some(a.clone()),
        (None, Some(b)) => Some(b.clone()),
        (None, None) => None,
    };

    Snapshot {
        ts: now,
        rev,
        fiat_a: settings.fiat_a.clone(),
        fiat_b: settings.fiat_b.clone(),
        buy,
        sell,
        j,
        j_second,
        fragile,
        threshold: settings.threshold,
        mid,
        mid_source,
        mid_age_secs: mid_age,
        spread_pct,
        alert_active,
        alert_since: None,
        alert_count: 0,
        feasible,
        rate_met,
        stale: false,
        error,
        next_poll_ms,
        refresh_secs: settings.refresh_secs,
        alert_badge: settings.alert_badge,
        alert_sound: settings.alert_sound,
        alert_notification: settings.alert_notification,
        sound_name: settings.sound_name.clone(),
    }
}

/// Best achievable cross rate if the single top offer disappears on either side.
fn runner_up(
    buy: &LegView,
    sell: &LegView,
    threshold: f64,
    active: bool,
) -> (Option<f64>, bool) {
    let buy_top = buy.offers.first();
    let buy_next = buy.offers.get(1);
    let sell_top = sell.offers.first();
    let sell_next = sell.offers.get(1);

    let mut alternatives: Vec<f64> = Vec::new();
    if let (Some(buy_top), Some(sell_next)) = (buy_top, sell_next) {
        alternatives.push(sell_next.price / buy_top.price);
    }
    if let (Some(buy_next), Some(sell_top)) = (buy_next, sell_top) {
        alternatives.push(sell_top.price / buy_next.price);
    }

    let best_alternative = alternatives
        .into_iter()
        .filter(|value| value.is_finite() && *value > 0.0)
        .reduce(f64::max);

    // Only call it fragile when we actually know of an alternative that drops
    // below R. With a single ad per leg we have no evidence either way.
    let fragile = active && best_alternative.is_some_and(|alt| alt < threshold);
    (best_alternative, fragile)
}

fn alert_text(snapshot: &Snapshot, settings: &Settings) -> (String, String) {
    let j = snapshot.j.unwrap_or(0.0);
    let title = format!("CrossRate · {} → {}", settings.fiat_a, settings.fiat_b);

    let spread = snapshot
        .spread_pct
        .map(|value| format!(" · {value:+.2}% vs mid"))
        .unwrap_or_default();

    let size = match (snapshot.buy.best.as_ref(), snapshot.sell.best.as_ref()) {
        (Some(buy), Some(sell)) => filters::describe_trade_size(buy, sell, settings),
        _ => String::new(),
    };

    let body = if size.is_empty() {
        format!(
            "j {:.4} {} per {} ≥ R {:.4}{}",
            j, settings.fiat_b, settings.fiat_a, settings.threshold, spread
        )
    } else {
        format!(
            "j {:.4} {} per {} ≥ R {:.4}{} · {}",
            j, settings.fiat_b, settings.fiat_a, settings.threshold, spread, size
        )
    };

    (title, body)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn offer(user: &str, price: f64) -> Offer {
        Offer {
            adv_no: format!("adv-{user}"),
            user_no: user.to_string(),
            nickname: user.to_string(),
            user_type: "merchant".to_string(),
            user_identity: "MASS_MERCHANT".to_string(),
            completion_rate: 0.99,
            positive_rate: 0.98,
            month_orders: 500,
            active_secs: 30,
            price,
            min_amount: 100.0,
            max_amount: 100_000.0,
            available_usdt: 100_000.0,
            pay_types: vec!["BANK".to_string()],
            pay_time_limit: 15,
            is_favorite: false,
            suspicious: false,
        }
    }

    fn leg(side: LegSide, fiat: &str, offers: Vec<Offer>) -> LegView {
        LegView {
            side,
            fiat: fiat.to_string(),
            best: offers.first().cloned(),
            passed: offers.len(),
            fetched: offers.len(),
            offers,
            total: 10,
            error: None,
            pay_methods: vec!["BANK".to_string()],
        }
    }

    fn settings() -> Settings {
        Settings {
            threshold: 2.75,
            buy_min_amount: 100.0,
            buy_max_amount: 50_000.0,
            sell_min_amount: 100.0,
            sell_max_amount: 100_000.0,
            ..Settings::default()
        }
    }

    #[test]
    fn computes_cross_rate_and_alert() {
        let s = settings();
        let buy = leg(LegSide::Buy, "AED", vec![offer("b1", 3.69)]);
        let sell = leg(LegSide::Sell, "MAD", vec![offer("s1", 10.85)]);
        let snap = compose(&s, 1, buy, sell, Some(2.726018), "manual".into(), 0, 0, 0);
        let j = snap.j.unwrap();
        assert!((j - 2.940379).abs() < 1e-5, "j={j}");
        assert!(snap.rate_met);
        assert!(snap.alert_active);
        assert!(!snap.fragile);
        let spread = snap.spread_pct.unwrap();
        assert!(spread > 7.0 && spread < 9.0, "spread={spread}");
    }

    #[test]
    fn alert_clears_below_threshold() {
        let s = settings();
        let buy = leg(LegSide::Buy, "AED", vec![offer("b1", 3.69)]);
        let sell = leg(LegSide::Sell, "MAD", vec![offer("s1", 10.10)]);
        let snap = compose(&s, 1, buy, sell, None, "unavailable".into(), 0, 0, 0);
        assert!(!snap.rate_met);
        assert!(!snap.alert_active);
        assert!(snap.mid.is_none());
        assert!(snap.spread_pct.is_none());
    }

    #[test]
    fn missing_leg_means_no_alert() {
        let s = settings();
        let buy = leg(LegSide::Buy, "AED", vec![offer("b1", 3.69)]);
        let sell = leg(LegSide::Sell, "MAD", vec![]);
        let snap = compose(&s, 1, buy, sell, None, "unavailable".into(), 0, 0, 0);
        assert!(snap.j.is_none());
        assert!(!snap.alert_active);
        assert!(!snap.feasible.ok);
    }

    #[test]
    fn detects_fragile_alert() {
        let s = settings();
        // 10.85/3.60 = 3.01 (alerting) but every alternative pair falls below R:
        // 9.80/3.60 = 2.72 and 10.85/4.20 = 2.58.
        let buy = leg(
            LegSide::Buy,
            "AED",
            vec![offer("b1", 3.60), offer("b2", 4.20)],
        );
        let sell = leg(
            LegSide::Sell,
            "MAD",
            vec![offer("s1", 10.85), offer("s2", 9.80)],
        );
        let snap = compose(&s, 1, buy, sell, None, "unavailable".into(), 0, 0, 0);
        assert!(snap.j.unwrap() > 2.75);
        assert!(snap.fragile);
        let alt = snap.j_second.unwrap();
        assert!(alt < 2.75, "alt={alt}");
    }

    #[test]
    fn not_fragile_without_alternatives() {
        let s = settings();
        // One ad per leg: no runner-up evidence, so we do not claim fragility.
        let buy = leg(LegSide::Buy, "AED", vec![offer("b1", 3.69)]);
        let sell = leg(LegSide::Sell, "MAD", vec![offer("s1", 10.85)]);
        let snap = compose(&s, 1, buy, sell, None, "unavailable".into(), 0, 0, 0);
        assert!(snap.alert_active);
        assert!(snap.j_second.is_none());
        assert!(!snap.fragile);
    }

    #[test]
    fn alert_requires_executable_size() {
        let mut s = settings();
        s.buy_max_amount = 500.0; // max 135 USDT at 3.69
        s.sell_min_amount = 50_000.0; // min ~4608 USDT at 10.85
        let buy = leg(LegSide::Buy, "AED", vec![offer("b1", 3.69)]);
        let sell = leg(LegSide::Sell, "MAD", vec![offer("s1", 10.85)]);
        let snap = compose(&s, 1, buy, sell, None, "unavailable".into(), 0, 0, 0);
        assert!(snap.rate_met);
        assert!(!snap.feasible.ok);
        assert!(!snap.alert_active);
    }
}
