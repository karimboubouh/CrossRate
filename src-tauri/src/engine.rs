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
/// Offers kept per leg for pair matching (the UI renders the best few).
const MAX_CANDIDATES: usize = 40;

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

        // Register interest *before* the slow network work. `Notify` stores no
        // permit, so a wake-up raised during the poll would otherwise be lost.
        let wake = state.wake.notified();
        tokio::pin!(wake);

        // FX: copy the cache out, fetch without holding the lock, write back.
        let fx_before = state.fx.read().await.clone();
        let (fx_after, mid, mid_source, mid_age) = fx::resolve(
            &state.http,
            &fx_before,
            &settings.fiat_a,
            &settings.fiat_b,
            settings.mid_override,
        )
        .await;
        *state.fx.write().await = fx_after;

        let (buy_view, sell_view) = tokio::join!(
            fetch_leg(&state.http, &settings, LegSide::Buy),
            fetch_leg(&state.http, &settings, LegSide::Sell)
        );
        // Headline rate comes from the best pair that is actually tradable.
        let (buy_view, sell_view) = resolve_pair(buy_view, sell_view, &settings);

        // If the user saved settings while this poll was in flight, the numbers
        // belong to the previous configuration: drop them and poll again.
        if state
            .settings_rev
            .load(std::sync::atomic::Ordering::Relaxed)
            != rev
        {
            crate::util::log_line("settings changed during poll; discarding result");
            continue;
        }

        let now = now_ms();
        let base_ms = settings.refresh_secs * 1000;
        let backoff_ms = if consecutive_failures > 0 {
            (1u64 << consecutive_failures.min(4)) * 1000
        } else {
            0
        };
        let interval_ms = base_ms.max(backoff_ms);
        let next_poll_ms = now + interval_ms as i64;

        let poll_failed = classify_poll(&buy_view, &sell_view) == PollOutcome::Failed;

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

        if poll_failed {
            consecutive_failures = consecutive_failures.saturating_add(1);
            snapshot.stale = true;

            // Keep the last good market, but only for the same pair and with its
            // original timestamp so "updated Xs ago" stays truthful.
            let previous = state.snapshot.read().await.clone();
            merge_stale(&mut snapshot, previous.as_ref());

            // Unknown data neither arms nor clears an alert.
            let alert = state.alert.lock().await;
            snapshot.alert_active = alert.active;
            snapshot.alert_since = alert.since;
            snapshot.alert_count = alert.count;
        } else {
            consecutive_failures = 0;

            let (active, since, count, fresh_alert) = {
                let mut alert = state.alert.lock().await;
                let was_active = alert.active;
                // A missing tradable size blocks a *new* alert, but a live alert
                // only clears when the rate itself drops below target; otherwise
                // a momentarily thin book would re-arm and re-notify.
                let active = snapshot.rate_met && (snapshot.feasible.ok || was_active);
                if active && !was_active {
                    alert.since = Some(now);
                    alert.count = alert.count.saturating_add(1);
                }
                if !active {
                    alert.since = None;
                }
                alert.active = active;
                (active, alert.since, alert.count, active && !was_active)
            };

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

            snapshot.alert_active = active;
            snapshot.alert_since = since;
            snapshot.alert_count = count;
        }

        *state.snapshot.write().await = Some(snapshot.clone());
        let _ = app.emit(SNAPSHOT_EVENT, &snapshot);
        tray::update(&app, &snapshot);

        crate::util::log_line(&format!(
            "{}→{} j={} R={} buy={}({}/{}) sell={}({}/{}) alert={}{}{}{}{}",
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
            if snapshot.stale { " stale" } else { "" },
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

        let elapsed_ms = started.elapsed().as_millis() as u64;
        let sleep_ms = interval_ms.saturating_sub(elapsed_ms).max(1_200);

        tokio::select! {
            _ = tokio::time::sleep(Duration::from_millis(sleep_ms)) => {}
            _ = &mut wake => {
                crate::util::log_line("woken early by settings change or manual refresh");
            }
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

    // Keep a deeper candidate set than the UI shows: the best individually
    // priced ads are not always the ones that can be paired at a tradable size.
    let passed = collected.len();
    let best = collected.first().cloned();
    let offers: Vec<Offer> = collected.into_iter().take(MAX_CANDIDATES).collect();

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

/// Whether a poll produced usable numbers.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum PollOutcome {
    Ok,
    /// Unknown data: an error, or nothing came back at all.
    Failed,
}

/// A leg that errored — or a completely empty response — means "unknown", not
/// "rate below target". Filters rejecting every advertisement is NOT a failure:
/// that is a valid market state and must clear the headline rate honestly.
pub(crate) fn classify_poll(buy: &LegView, sell: &LegView) -> PollOutcome {
    if buy.error.is_some() || sell.error.is_some() {
        return PollOutcome::Failed;
    }
    if buy.fetched == 0 && sell.fetched == 0 {
        return PollOutcome::Failed;
    }
    PollOutcome::Ok
}

/// Carry the last good market into a failed poll, but only for the same pair,
/// and keep its original timestamp so "updated Xs ago" stays truthful.
pub(crate) fn merge_stale(snapshot: &mut Snapshot, previous: Option<&Snapshot>) {
    let Some(previous) = previous else {
        return;
    };
    if previous.fiat_a != snapshot.fiat_a
        || previous.fiat_b != snapshot.fiat_b
        || previous.j.is_none()
    {
        return;
    }

    snapshot.buy = previous.buy.clone();
    snapshot.sell = previous.sell.clone();
    snapshot.j = previous.j;
    snapshot.j_second = previous.j_second;
    snapshot.fragile = previous.fragile;
    snapshot.feasible = previous.feasible.clone();
    snapshot.rate_met = previous.rate_met;
    snapshot.spread_pct = previous.spread_pct;
    snapshot.ts = previous.ts;
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
                let (j_second, fragile) =
                    runner_up(&buy, &sell, settings, settings.threshold, alert_active);
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
    settings: &Settings,
    threshold: f64,
    active: bool,
) -> (Option<f64>, bool) {
    // `best` may not be offers[0] after resolve_pair(), so locate the actual
    // pair members and take the next advertisement on each side.
    let index_of = |leg: &LegView| {
        leg.best
            .as_ref()
            .and_then(|best| leg.offers.iter().position(|offer| offer.adv_no == best.adv_no))
            .unwrap_or(0)
    };
    let (buy_index, sell_index) = (index_of(buy), index_of(sell));

    let buy_top = buy.offers.get(buy_index);
    let buy_next = buy.offers.get(buy_index + 1);
    let sell_top = sell.offers.get(sell_index);
    let sell_next = sell.offers.get(sell_index + 1);

    // Only count an alternative that could actually be executed; a cheaper ad
    // whose limits cannot meet the counterparty is not a real fallback.
    let mut alternatives: Vec<f64> = Vec::new();
    if let (Some(buy_top), Some(sell_next)) = (buy_top, sell_next) {
        if filters::feasibility(buy_top, sell_next, settings).ok {
            alternatives.push(sell_next.price / buy_top.price);
        }
    }
    if let (Some(buy_next), Some(sell_top)) = (buy_next, sell_top) {
        if filters::feasibility(buy_next, sell_top, settings).ok {
            alternatives.push(sell_top.price / buy_next.price);
        }
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

/// How the quoted size relates to what the user configured.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SizeQuote {
    /// The configured amount is tradable as-is.
    Exact,
    /// Capped by the largest size the books allow ("Up to …").
    Capped,
    /// Raised to the smallest tradable ticket ("Minimum …").
    Floored,
}

/// The size to quote: the user's configured amount, clamped into the window
/// that is actually tradable right now.
pub fn quoted_amount(settings: &Settings, snapshot: &Snapshot) -> (f64, SizeQuote) {
    let want = settings.notify_amount.max(1.0);
    if snapshot.feasible.ok {
        let (low, high) = (snapshot.feasible.a_min, snapshot.feasible.a_max);
        if high >= low && high > 0.0 {
            if want > high {
                return (high, SizeQuote::Capped);
            }
            if want < low {
                return (low, SizeQuote::Floored);
            }
        }
    }
    (want, SizeQuote::Exact)
}

fn quote_prefix(quote: SizeQuote) -> &'static str {
    match quote {
        SizeQuote::Exact => "",
        SizeQuote::Capped => "Up to ",
        SizeQuote::Floored => "Minimum ",
    }
}

/// Alert copy. Leads with the P2P rate, then the money at the configured size.
/// Deliberately avoids internal vocabulary: no "j", no "R".
pub fn alert_text(snapshot: &Snapshot, settings: &Settings) -> (String, String) {
    let a = &settings.fiat_a;
    let b = &settings.fiat_b;
    let rate = snapshot.j.unwrap_or(0.0);
    let title = format!("{a} → {b} · {}", fmt_rate(rate));

    let (amount, quote) = quoted_amount(settings, snapshot);
    let lead = quote_prefix(quote);
    let received = amount * rate;

    let body = match snapshot.mid {
        Some(mid) if mid > 0.0 => {
            let at_bank = amount * mid;
            let gain = received - at_bank;
            let pct = (rate / mid - 1.0) * 100.0;
            format!(
                "{}{} {} buys {} {}. Bank rate {} — {} {} ({:+.1}%) better.",
                lead,
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
            "{}{} {} buys {} {} at {} {} per {}.",
            lead,
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

#[cfg(test)]
mod quoting_tests {
    use super::*;

    fn offer(price: f64, min: f64, max: f64) -> Offer {
        Offer {
            adv_no: format!("adv-{price}"),
            user_no: "u".into(),
            nickname: "MBebars".into(),
            user_type: "merchant".into(),
            user_identity: "MASS_MERCHANT".into(),
            completion_rate: 0.96,
            positive_rate: 0.98,
            month_orders: 1000,
            active_secs: 30,
            price,
            min_amount: min,
            max_amount: max,
            available_usdt: 50_000.0,
            pay_types: vec!["BANK".into()],
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

    fn base_settings(notify_amount: f64) -> Settings {
        Settings {
            fiat_a: "AED".into(),
            fiat_b: "MAD".into(),
            threshold: 2.5,
            notify_amount,
            buy_min_amount: 100.0,
            buy_max_amount: 50_000.0,
            sell_min_amount: 100.0,
            sell_max_amount: 100_000.0,
            ..Settings::default()
        }
    }

    #[test]
    fn quotes_configured_size_when_it_is_tradable() {
        let settings = base_settings(5_000.0);
        let buy = leg(LegSide::Buy, "AED", vec![offer(3.69, 1_000.0, 50_000.0)]);
        let sell = leg(LegSide::Sell, "MAD", vec![offer(10.85, 1_000.0, 100_000.0)]);
        let snapshot = compose(&settings, 1, buy, sell, Some(2.726), "manual".into(), 0, 0, 0);

        let (amount, quote) = quoted_amount(&settings, &snapshot);
        assert_eq!(quote, SizeQuote::Exact);
        assert!((amount - 5_000.0).abs() < 0.01);
    }

    #[test]
    fn never_quotes_more_than_is_tradable() {
        // Ask for 100,000 AED but the sell ad caps out around 9,200 USDT.
        let settings = base_settings(100_000.0);
        let buy = leg(LegSide::Buy, "AED", vec![offer(3.69, 1_000.0, 50_000.0)]);
        let sell = leg(LegSide::Sell, "MAD", vec![offer(10.85, 1_000.0, 100_000.0)]);
        let snapshot = compose(&settings, 1, buy, sell, Some(2.726), "manual".into(), 0, 0, 0);
        assert!(snapshot.feasible.ok, "{}", snapshot.feasible.reason);

        let (amount, quote) = quoted_amount(&settings, &snapshot);
        assert_eq!(quote, SizeQuote::Capped);
        assert!(amount <= snapshot.feasible.a_max + 0.01);
        assert!(amount >= snapshot.feasible.a_min - 0.01);

        let (_, body) = alert_text(&snapshot, &settings);
        assert!(body.starts_with("Up to "), "{body}");
    }

    #[test]
    fn floors_to_the_smallest_tradable_ticket() {
        // Ask for 100 AED, but the sell ad will not take less than ~2,000 AED.
        let settings = base_settings(100.0);
        let buy = leg(LegSide::Buy, "AED", vec![offer(3.69, 100.0, 500_000.0)]);
        let sell = leg(LegSide::Sell, "MAD", vec![offer(10.85, 21_700.0, 500_000.0)]);
        let snapshot = compose(&settings, 1, buy, sell, Some(2.726), "manual".into(), 0, 0, 0);
        assert!(snapshot.feasible.ok, "{}", snapshot.feasible.reason);

        let (amount, quote) = quoted_amount(&settings, &snapshot);
        assert_eq!(quote, SizeQuote::Floored);
        assert!(amount > 100.0);
        let (_, body) = alert_text(&snapshot, &settings);
        assert!(body.starts_with("Minimum "), "{body}");
    }

    #[test]
    fn fragile_uses_the_chosen_pair_members() {
        // The engine picked the second buy ad; its next alternative must be the
        // third, not the (better) first ad that was rejected on size.
        let settings = base_settings(10_000.0);
        let buy = leg(
            LegSide::Buy,
            "AED",
            vec![offer(3.60, 1.0, 50.0), offer(3.70, 1_000.0, 50_000.0), offer(4.20, 1_000.0, 50_000.0)],
        );
        let sell = leg(LegSide::Sell, "MAD", vec![offer(10.85, 1_000.0, 100_000.0)]);
        let (mut buy, mut sell) = (buy, sell);
        buy.best = buy.offers.get(1).cloned();
        sell.best = sell.offers.first().cloned();

        let snapshot = compose(&settings, 1, buy, sell, None, "unavailable".into(), 0, 0, 0);
        // Best alternative swaps in the ad AFTER the chosen one (4.20), giving
        // 10.85/4.20 = 2.583, which is below the threshold of 2.5? No: 2.583 > 2.5.
        let alternative = snapshot.j_second.expect("runner-up expected");
        assert!((alternative - 2.583).abs() < 0.01, "alt={alternative}");
    }
}

#[cfg(test)]
mod poll_tests {
    use super::*;

    fn offer(price: f64) -> Offer {
        Offer {
            adv_no: "a".into(),
            user_no: "u".into(),
            nickname: "n".into(),
            user_type: "merchant".into(),
            user_identity: "MASS_MERCHANT".into(),
            completion_rate: 0.99,
            positive_rate: 0.99,
            month_orders: 500,
            active_secs: 10,
            price,
            min_amount: 1_000.0,
            max_amount: 100_000.0,
            available_usdt: 50_000.0,
            pay_types: vec!["BANK".into()],
            pay_time_limit: 15,
            is_favorite: false,
            suspicious: false,
        }
    }

    fn leg(side: LegSide, offers: Vec<Offer>) -> LegView {
        LegView {
            side,
            fiat: "X".into(),
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
    fn one_errored_leg_is_a_failure_not_a_rate_drop() {
        let mut buy = leg(LegSide::Buy, vec![offer(3.69)]);
        let mut sell = leg(LegSide::Sell, vec![offer(10.85)]);
        assert_eq!(classify_poll(&buy, &sell), PollOutcome::Ok);

        sell.error = Some("HTTP 429".into());
        assert_eq!(classify_poll(&buy, &sell), PollOutcome::Failed);

        // Even if the other leg has good data, the pair is unknown.
        buy.error = None;
        sell.error = None;
        sell.fetched = 0;
        sell.offers.clear();
        sell.best = None;
        sell.passed = 0;
        assert_eq!(classify_poll(&buy, &sell), PollOutcome::Ok);
    }

    #[test]
    fn both_legs_empty_is_a_failure() {
        let buy = LegView {
            side: LegSide::Buy,
            fiat: "AED".into(),
            best: None,
            offers: vec![],
            fetched: 0,
            passed: 0,
            total: 0,
            error: None,
            pay_methods: vec![],
        };
        let mut sell = buy.clone();
        sell.side = LegSide::Sell;
        assert_eq!(classify_poll(&buy, &sell), PollOutcome::Failed);
    }

    #[test]
    fn filtered_to_empty_is_not_a_failure() {
        // Ads came back but the user's filters rejected them: honest "no rate".
        let mut buy = leg(LegSide::Buy, vec![]);
        buy.fetched = 20;
        let mut sell = leg(LegSide::Sell, vec![]);
        sell.fetched = 20;
        assert_eq!(classify_poll(&buy, &sell), PollOutcome::Ok);
    }

    fn snapshot(settings: &Settings, buy: LegView, sell: LegView, now: i64) -> Snapshot {
        compose(settings, 1, buy, sell, None, "unavailable".into(), 0, now + 15_000, now)
    }

    #[test]
    fn stale_merge_keeps_last_good_numbers_and_timestamp() {
        let settings = Settings {
            buy_min_amount: 100.0,
            buy_max_amount: 50_000.0,
            sell_min_amount: 100.0,
            sell_max_amount: 100_000.0,
            ..Settings::default()
        };
        let good = snapshot(
            &settings,
            leg(LegSide::Buy, vec![offer(3.69)]),
            leg(LegSide::Sell, vec![offer(10.85)]),
            1_000,
        );
        assert!(good.j.is_some());

        let mut failed = snapshot(
            &settings,
            leg(LegSide::Buy, vec![]),
            leg(LegSide::Sell, vec![]),
            2_000,
        );
        merge_stale(&mut failed, Some(&good));
        assert_eq!(failed.j, good.j);
        assert_eq!(failed.ts, 1_000, "timestamp must stay with the data");
    }

    #[test]
    fn stale_merge_refuses_a_different_pair() {
        let settings = Settings::default();
        let good = snapshot(
            &settings,
            leg(LegSide::Buy, vec![offer(3.69)]),
            leg(LegSide::Sell, vec![offer(10.85)]),
            1_000,
        );
        let mut other = settings.clone();
        other.fiat_b = "USD".into();
        let mut failed = snapshot(
            &other,
            leg(LegSide::Buy, vec![]),
            leg(LegSide::Sell, vec![]),
            2_000,
        );
        merge_stale(&mut failed, Some(&good));
        assert!(failed.j.is_none(), "old pair's rate must not be reused");
    }
}
