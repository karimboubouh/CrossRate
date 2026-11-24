//! The polling engine: fetch both legs, filter, compute the cross rate, drive
//! alerts, the tray badge and the UI.

use crate::alerts;
use crate::binance::{self, Query};
use crate::filters;
use crate::fx;
use crate::model::*;
use crate::state::AppState;
use crate::tray;
use crate::util::{fmt_amount_compact, fmt_amount_grouped, fmt_rate, now_ms};
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
        // Headline rate comes from the best pair that is actually tradable.
        let (buy_view, sell_view) = resolve_pair(buy_view, sell_view, &settings);

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
            // A missing tradable size blocks a *new* alert, but an alert that is
            // already live only clears when the rate itself drops below R.
            // Otherwise a momentarily thin book would re-arm and re-notify.
            let active = snapshot.rate_met && (snapshot.feasible.ok || was_active);
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
            crate::util::log_line(&format!("ALERT \"{title}\" | \"{body}\""));
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

        crate::util::log_line(&format!(
            "{}→{} j={} R={} buy={}({}/{}) sell={}({}/{}) alert={}{}{}{}",
            snapshot.fiat_a,
            snapshot.fiat_b,
            snapshot.j.map(fmt_rate).unwrap_or_else(|| "-".into()),
            fmt_rate(snapshot.threshold),
            snapshot
                .buy
                .best
                .as_ref()
                .map(|o| fmt_rate(o.price))
                .unwrap_or_else(|| "-".into()),
            snapshot.buy.passed,
            snapshot.buy.total,
            snapshot
                .sell
                .best
                .as_ref()
                .map(|o| fmt_rate(o.price))
                .unwrap_or_else(|| "-".into()),
            snapshot.sell.passed,
            snapshot.sell.total,
            snapshot.alert_active,
            if snapshot.fragile { " fragile" } else { "" },
            if snapshot.feasible.ok {
                format!(
                    " size={}-{}USDT",
                    fmt_amount_compact(snapshot.feasible.usdt_min),
                    fmt_amount_compact(snapshot.feasible.usdt_max)
                )
            } else {
                format!(" nosize=\"{}\"", snapshot.feasible.reason)
            },
            match &snapshot.error {
                Some(err) => format!(" error={err}"),
                None => String::new(),
            }
        ));

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

async fn fetch_leg(client: &reqwest::Client, settings: &Settings, side: LegSide) -> LegView {    let side_filters = side.filters(settings);
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

/// The cheapest buy ad and the most expensive sell ad are not necessarily
/// tradable at the same size. Pick the pair with the best cross rate whose
/// advertised limits actually overlap, so the headline rate is actionable.
fn resolve_pair(mut buy: LegView, mut sell: LegView, settings: &Settings) -> (LegView, LegView) {
    let mut best: Option<(usize, usize, f64)> = None;

    for (bi, buy_offer) in buy.offers.iter().enumerate() {
        if buy_offer.price <= 0.0 {
            continue;
        }
        for (si, sell_offer) in sell.offers.iter().enumerate() {
            if sell_offer.price <= 0.0 {
                continue;
            }
            if !filters::feasibility(buy_offer, sell_offer, settings).ok {
                continue;
            }
            let j = sell_offer.price / buy_offer.price;
            if best.map_or(true, |(_, _, current)| j > current) {
                best = Some((bi, si, j));
            }
        }
    }

    if let Some((bi, si, _)) = best {
        if let Some(offer) = buy.offers.get(bi).cloned() {
            buy.best = Some(offer);
        }
        if let Some(offer) = sell.offers.get(si).cloned() {
            sell.best = Some(offer);
        }
    }

    (buy, sell)
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

/// Alert copy. Leads with the P2P rate, then the money at the configured size.
/// Deliberately avoids internal vocabulary: no "j", no "R".
pub fn alert_text(snapshot: &Snapshot, settings: &Settings) -> (String, String) {
    let a = &settings.fiat_a;
    let b = &settings.fiat_b;
    let rate = snapshot.j.unwrap_or(0.0);
    let title = format!("{a} → {b} · {}", fmt_rate(rate));

    let amount = settings.notify_amount.max(1.0);
    let received = amount * rate;

    let body = match snapshot.mid {
        Some(mid) if mid > 0.0 => {
            let at_bank = amount * mid;
            let gain = received - at_bank;
            let pct = (rate / mid - 1.0) * 100.0;
            format!(
                "{} {} buys {} {}. Bank rate {} — {} {} ({:+.1}%) better.",
                fmt_amount_grouped(amount),
                a,
                fmt_amount_grouped(received),
                b,
                fmt_rate(mid),
                signed_amount(gain),
                b,
                pct
            )
        }
        _ => format!(
            "{} {} buys {} {} at {} {} per {}.",
            fmt_amount_grouped(amount),
            a,
            fmt_amount_grouped(received),
            b,
            fmt_rate(rate),
            b,
            a
        ),
    };

    (title, body)
}

fn signed_amount(value: f64) -> String {
    if value >= 0.0 {
        format!("+{}", fmt_amount_grouped(value))
    } else {
        fmt_amount_grouped(value)
    }
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

#[cfg(test)]
mod pair_tests {
    use super::*;

    fn offer(user: &str, price: f64, min: f64, max: f64) -> Offer {
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
            min_amount: min,
            max_amount: max,
            available_usdt: 100_000.0,
            pay_types: vec!["BANK".to_string()],
            pay_time_limit: 15,
            is_favorite: false,
            suspicious: false,
        }
    }

    fn leg(side: LegSide, offers: Vec<Offer>) -> LegView {
        LegView {
            side,
            fiat: if side == LegSide::Buy { "AED".into() } else { "MAD".into() },
            best: offers.first().cloned(),
            passed: offers.len(),
            fetched: offers.len(),
            offers,
            total: 10,
            error: None,
            pay_methods: vec![],
        }
    }

    fn settings() -> Settings {
        Settings {
            buy_min_amount: 100.0,
            buy_max_amount: 50_000.0,
            sell_min_amount: 100.0,
            sell_max_amount: 100_000.0,
            ..Settings::default()
        }
    }

    #[test]
    fn skips_cheapest_ad_when_it_cannot_be_paired() {
        let s = settings();
        // Cheapest AED ad only trades 100–105 AED (~29 USDT) — too small for the
        // MAD seller, who needs at least ~1834 USDT.
        let cheap = offer("cheap", 3.60, 100.0, 105.0);
        let wide = offer("wide", 3.70, 1_000.0, 50_000.0);
        let seller = offer("seller", 10.90, 20_000.0, 100_000.0);

        let (buy, sell) = resolve_pair(
            leg(LegSide::Buy, vec![cheap, wide]),
            leg(LegSide::Sell, vec![seller]),
            &s,
        );

        assert_eq!(buy.best.as_ref().unwrap().nickname, "wide");
        assert_eq!(sell.best.as_ref().unwrap().nickname, "seller");
        let f = filters::feasibility(buy.best.as_ref().unwrap(), sell.best.as_ref().unwrap(), &s);
        assert!(f.ok, "{}", f.reason);
    }

    #[test]
    fn keeps_top_offers_when_they_pair_up() {
        let s = settings();
        let buy = leg(LegSide::Buy, vec![offer("b1", 3.69, 1_000.0, 50_000.0)]);
        let sell = leg(LegSide::Sell, vec![offer("s1", 10.85, 5_000.0, 100_000.0)]);
        let (buy, sell) = resolve_pair(buy, sell, &s);
        assert_eq!(buy.best.as_ref().unwrap().nickname, "b1");
        assert_eq!(sell.best.as_ref().unwrap().nickname, "s1");
    }

    #[test]
    fn falls_back_to_headline_offers_when_nothing_is_tradable() {
        let s = settings();
        let buy = leg(LegSide::Buy, vec![offer("b1", 3.69, 10.0, 20.0)]);
        let sell = leg(LegSide::Sell, vec![offer("s1", 10.85, 90_000.0, 100_000.0)]);
        let (buy, sell) = resolve_pair(buy, sell, &s);
        // Best offers are still reported, and feasibility will explain the gap.
        assert!(buy.best.is_some());
        assert!(sell.best.is_some());
        let f = filters::feasibility(buy.best.as_ref().unwrap(), sell.best.as_ref().unwrap(), &s);
        assert!(!f.ok);
    }
}

#[cfg(test)]
mod alert_copy_tests {
    use super::*;

    fn offer(price: f64) -> Offer {
        Offer {
            adv_no: "a".into(),
            user_no: "u".into(),
            nickname: "MBebars".into(),
            user_type: "merchant".into(),
            user_identity: "MASS_MERCHANT".into(),
            completion_rate: 0.96,
            positive_rate: 0.98,
            month_orders: 1007,
            active_secs: 30,
            price,
            min_amount: 1_000.0,
            max_amount: 100_000.0,
            available_usdt: 50_000.0,
            pay_types: vec!["cihmorocco".into()],
            pay_time_limit: 15,
            is_favorite: false,
            suspicious: false,
        }
    }

    fn leg(side: LegSide, fiat: &str, offers: Vec<Offer>) -> LegView {
        LegView {
            side,
            fiat: fiat.into(),
            best: offers.first().cloned(),
            passed: offers.len(),
            fetched: offers.len(),
            offers,
            total: 10,
            error: None,
            pay_methods: vec![],
        }
    }

    #[test]
    fn copy_leads_with_rate_and_uses_configured_amount() {
        let settings = Settings {
            fiat_a: "AED".into(),
            fiat_b: "MAD".into(),
            threshold: 2.75,
            notify_amount: 10_000.0,
            buy_min_amount: 100.0,
            buy_max_amount: 50_000.0,
            sell_min_amount: 100.0,
            sell_max_amount: 100_000.0,
            ..Settings::default()
        };
        // 10.752 / 3.691 = 2.9130 MAD per AED
        let buy = leg(LegSide::Buy, "AED", vec![offer(3.691)]);
        let sell = leg(LegSide::Sell, "MAD", vec![offer(10.752)]);
        let snapshot = compose(&settings, 1, buy, sell, Some(2.726018), "manual".into(), 0, 0, 0);

        let (title, body) = alert_text(&snapshot, &settings);
        assert_eq!(title, "AED → MAD · 2.913");
        assert!(body.contains("10,000 AED buys 29,130 MAD"), "{body}");
        assert!(body.contains("Bank rate 2.726"), "{body}");
        assert!(body.contains("+1,870 MAD"), "{body}");
        assert!(body.contains("+6.9%"), "{body}");
        assert!(!title.contains('j') && !body.contains("R "), "no internal jargon: {title} / {body}");
    }

    #[test]
    fn copy_works_without_a_mid_rate() {
        let settings = Settings {
            fiat_a: "AED".into(),
            fiat_b: "MAD".into(),
            notify_amount: 1_000.0,
            buy_min_amount: 100.0,
            buy_max_amount: 50_000.0,
            sell_min_amount: 100.0,
            sell_max_amount: 100_000.0,
            ..Settings::default()
        };
        let buy = leg(LegSide::Buy, "AED", vec![offer(3.691)]);
        let sell = leg(LegSide::Sell, "MAD", vec![offer(10.752)]);
        let snapshot = compose(&settings, 1, buy, sell, None, "unavailable".into(), 0, 0, 0);
        let (title, body) = alert_text(&snapshot, &settings);
        assert_eq!(title, "AED → MAD · 2.913");
        assert!(body.contains("1,000 AED buys 2,913 MAD"), "{body}");
        assert!(!body.contains("Bank"), "{body}");
    }
}
