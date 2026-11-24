//! Filtering, ranking and size-feasibility math for P2P offers.

use crate::model::*;
use crate::util::fmt_amount_compact;
use std::collections::HashSet;

/// Does an offer satisfy the user's filters and overlap their amount band?
pub fn passes(offer: &Offer, filters: &SideFilters, band_min: f64, band_max: f64) -> bool {
    // Amount band must intersect the advertisement's own [min, max] range.
    let lowest_acceptable = offer.min_amount.max(band_min);
    let highest_acceptable = if band_max > 0.0 {
        offer.max_amount.min(band_max)
    } else {
        offer.max_amount
    };
    if highest_acceptable < lowest_acceptable {
        return false;
    }

    if offer.completion_rate + 1e-9 < filters.min_completion_rate {
        return false;
    }
    if offer.positive_rate + 1e-9 < filters.min_positive_rate {
        return false;
    }
    if offer.month_orders < filters.min_month_orders as i64 {
        return false;
    }
    if filters.online_only && offer.active_secs > filters.online_within_secs as i64 {
        return false;
    }
    match filters.publisher {
        PublisherFilter::Any => {}
        PublisherFilter::Merchant => {
            if offer.user_type != "merchant" {
                return false;
            }
        }
        PublisherFilter::User => {
            if offer.user_type != "user" {
                return false;
            }
        }
    }
    if !filters.pay_types.is_empty()
        && !offer
            .pay_types
            .iter()
            .any(|method| filters.pay_types.contains(method))
    {
        return false;
    }

    true
}

pub fn sort(offers: &mut [Offer], side: LegSide) {
    offers.sort_by(|a, b| {
        let by_price = match side {
            LegSide::Buy => a.price.partial_cmp(&b.price),
            LegSide::Sell => b.price.partial_cmp(&a.price),
        }
        .unwrap_or(std::cmp::Ordering::Equal);
        // Break ties in favour of the more reliable counterparty.
        by_price.then_with(|| {
            b.completion_rate
                .partial_cmp(&a.completion_rate)
                .unwrap_or(std::cmp::Ordering::Equal)
        })
    });
}

/// Keep only the best advertisement per advertiser, so a single merchant cannot
/// occupy every slot in the list.
pub fn dedupe_by_user(offers: &mut Vec<Offer>) {
    let mut seen: HashSet<String> = HashSet::new();
    offers.retain(|offer| seen.insert(offer.user_no.clone()));
}

pub fn apply(
    offers: Vec<Offer>,
    side: LegSide,
    filters: &SideFilters,
    band: (f64, f64),
) -> Vec<Offer> {
    let mut kept: Vec<Offer> = offers
        .into_iter()
        .filter(|offer| passes(offer, filters, band.0, band.1))
        .collect();
    sort(&mut kept, side);
    dedupe_by_user(&mut kept);
    kept
}

/// Intersect both legs' advertised limits to find a trade size that is actually
/// executable from start to finish.
pub fn feasibility(best_buy: &Offer, best_sell: &Offer, settings: &Settings) -> Feasible {
    if best_buy.price <= 0.0 || best_sell.price <= 0.0 {
        return Feasible::none("Missing a usable price");
    }

    let (buy_band_min, buy_band_max) = settings.buy_amount_band();
    let buy_low = best_buy.min_amount.max(buy_band_min);
    let buy_high = if buy_band_max > 0.0 {
        best_buy.max_amount.min(buy_band_max)
    } else {
        best_buy.max_amount
    };
    if buy_high < buy_low {
        return Feasible::none(format!(
            "Best {} offer trades {}–{} {}",
            settings.fiat_a,
            fmt_amount_compact(best_buy.min_amount),
            fmt_amount_compact(best_buy.max_amount),
            settings.fiat_a
        ));
    }

    let (sell_band_min, sell_band_max) = settings.sell_amount_band();
    let sell_low = best_sell.min_amount.max(sell_band_min);
    let sell_high = if sell_band_max > 0.0 {
        best_sell.max_amount.min(sell_band_max)
    } else {
        best_sell.max_amount
    };
    if sell_high < sell_low {
        return Feasible::none(format!(
            "Best {} offer trades {}–{} {}",
            settings.fiat_b,
            fmt_amount_compact(best_sell.min_amount),
            fmt_amount_compact(best_sell.max_amount),
            settings.fiat_b
        ));
    }

    let buy_usdt = (buy_low / best_buy.price, buy_high / best_buy.price);
    let sell_usdt = (sell_low / best_sell.price, sell_high / best_sell.price);

    let usdt_min = buy_usdt.0.max(sell_usdt.0);
    let mut usdt_max = buy_usdt.1.min(sell_usdt.1);

    // An ad's advertised limit can exceed what the advertiser still holds; the
    // real executable size is bounded by the available balance on both legs.
    let liquidity = [best_buy.available_usdt, best_sell.available_usdt]
        .into_iter()
        .filter(|value| *value > 0.0)
        .fold(f64::INFINITY, f64::min);
    if liquidity.is_finite() {
        usdt_max = usdt_max.min(liquidity);
    }
    if usdt_max < usdt_min || usdt_max <= 0.0 {
        if liquidity.is_finite() && liquidity < usdt_min {
            return Feasible::none(format!(
                "Not enough balance — only about {} USDT is left on the book",
                fmt_amount_compact(liquidity)
            ));
        }
        return Feasible::none(format!(
            "Size mismatch — {} leg allows {}–{} USDT, {} leg needs {}–{} USDT",
            settings.fiat_a,
            fmt_amount_compact(buy_usdt.0),
            fmt_amount_compact(buy_usdt.1),
            settings.fiat_b,
            fmt_amount_compact(sell_usdt.0),
            fmt_amount_compact(sell_usdt.1)
        ));
    }

    let a_min = usdt_min * best_buy.price;
    let a_max = usdt_max * best_buy.price;
    let b_min = usdt_min * best_sell.price;
    let b_max = usdt_max * best_sell.price;

    Feasible {
        ok: true,
        reason: String::new(),
        usdt_min,
        usdt_max,
        a_min,
        a_max,
        b_min,
        b_max,
    }
}

impl Settings {
    pub fn buy_amount_band(&self) -> (f64, f64) {
        (self.buy_min_amount, self.buy_max_amount)
    }

    pub fn sell_amount_band(&self) -> (f64, f64) {
        (self.sell_min_amount, self.sell_max_amount)
    }
}

/// Human description used in the alert body.
#[allow(dead_code)]
pub fn describe_trade_size(best_buy: &Offer, best_sell: &Offer, settings: &Settings) -> String {
    let feasible = feasibility(best_buy, best_sell, settings);
    if !feasible.ok {
        return feasible.reason;
    }
    format!(
        "{}–{} {} executable",
        fmt_amount_compact(feasible.a_min),
        fmt_amount_compact(feasible.a_max),
        settings.fiat_a
    )
}

#[cfg(test)]
mod tests {
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

    fn settings() -> Settings {
        Settings {
            fiat_a: "AED".to_string(),
            fiat_b: "MAD".to_string(),
            buy_min_amount: 1_000.0,
            buy_max_amount: 50_000.0,
            sell_min_amount: 5_000.0,
            sell_max_amount: 100_000.0,
            ..Settings::default()
        }
    }

    #[test]
    fn buy_leg_sorts_ascending_sell_descending() {
        let s = settings();
        let filters = SideFilters::default();
        let buy = apply(
            vec![offer("a", 3.70, 100.0, 10_000.0), offer("b", 3.68, 100.0, 10_000.0)],
            LegSide::Buy,
            &filters,
            s.buy_amount_band(),
        );
        assert_eq!(buy[0].price, 3.68);

        let sell = apply(
            vec![offer("a", 10.80, 5_000.0, 100_000.0), offer("b", 10.90, 5_000.0, 100_000.0)],
            LegSide::Sell,
            &filters,
            s.sell_amount_band(),
        );
        assert_eq!(sell[0].price, 10.90);
    }

    #[test]
    fn rejects_offers_outside_amount_band() {
        let s = settings();
        let filters = SideFilters::default();
        // 100–200 AED cannot satisfy a 1000 AED minimum.
        let kept = apply(
            vec![offer("a", 3.68, 100.0, 200.0)],
            LegSide::Buy,
            &filters,
            s.buy_amount_band(),
        );
        assert!(kept.is_empty());
    }

    #[test]
    fn filters_by_completion_orders_and_recency() {
        let mut strict = SideFilters {
            min_completion_rate: 0.95,
            min_month_orders: 100,
            online_only: true,
            online_within_secs: 600,
            ..SideFilters::default()
        };
        let mut o = offer("a", 3.68, 1_000.0, 10_000.0);
        o.completion_rate = 0.94;
        assert!(!passes(&o, &strict, 0.0, 0.0));

        o.completion_rate = 0.99;
        o.month_orders = 10;
        assert!(!passes(&o, &strict, 0.0, 0.0));

        o.month_orders = 500;
        o.active_secs = 5_000;
        assert!(!passes(&o, &strict, 0.0, 0.0));

        strict.online_only = false;
        assert!(passes(&o, &strict, 0.0, 0.0));

        strict.pay_types = vec!["Aani".to_string()];
        assert!(!passes(&o, &strict, 0.0, 0.0));
        o.pay_types = vec!["Aani".to_string()];
        assert!(passes(&o, &strict, 0.0, 0.0));
    }

    #[test]
    fn feasibility_intersects_both_legs() {
        let s = settings();
        // Buy 1000–50000 AED at 3.69 => 271–13550 USDT.
        // Sell allows 21700–100000 MAD at 10.85 => 2000–9216 USDT.
        // The executable window is therefore 2000–9216 USDT.
        let buy = offer("buyer", 3.69, 1_000.0, 50_000.0);
        let sell = offer("seller", 10.85, 21_700.0, 100_000.0);
        let f = feasibility(&buy, &sell, &s);
        assert!(f.ok, "{}", f.reason);
        assert!((f.usdt_min - 2_000.0).abs() < 0.5, "usdt_min={}", f.usdt_min);
        assert!((f.usdt_max - 9_216.6).abs() < 1.0, "usdt_max={}", f.usdt_max);
        assert!(f.a_min > 0.0 && f.b_min > 0.0);
    }

    #[test]
    fn feasibility_reports_size_mismatch() {
        let s = settings();
        let buy = offer("buyer", 3.69, 1_000.0, 1_500.0); // max ~406 USDT
        let sell = offer("seller", 10.85, 50_000.0, 100_000.0); // min ~4608 USDT
        let f = feasibility(&buy, &sell, &s);
        assert!(!f.ok);
        assert!(f.reason.contains("Size mismatch"), "{}", f.reason);
    }

    #[test]
    fn caps_tradable_size_by_remaining_balance() {
        let s = settings();
        let mut buy = offer("buyer", 3.69, 1_000.0, 50_000.0);
        let mut sell = offer("seller", 10.85, 1_000.0, 100_000.0);
        buy.available_usdt = 50_000.0;
        sell.available_usdt = 1_200.0; // seller only has 1,200 USDT left

        let f = feasibility(&buy, &sell, &s);
        assert!(f.ok, "{}", f.reason);
        assert!(f.usdt_max <= 1_200.0 + 0.01, "usdt_max={}", f.usdt_max);

        sell.available_usdt = 10.0; // now below the buyer's minimum
        let f = feasibility(&buy, &sell, &s);
        assert!(!f.ok);
        assert!(f.reason.contains("Not enough balance"), "{}", f.reason);
    }

    #[test]
    fn dedupes_per_advertiser() {
        let filters = SideFilters::default();
        let offers = vec![
            offer("same", 3.68, 1_000.0, 10_000.0),
            offer("same", 3.69, 1_000.0, 10_000.0),
            offer("other", 3.70, 1_000.0, 10_000.0),
        ];
        let kept = apply(offers, LegSide::Buy, &filters, (0.0, 0.0));
        assert_eq!(kept.len(), 2);
        assert_eq!(kept[0].price, 3.68);
    }

    #[test]
    fn describe_size_mentions_fiat() {
        let s = settings();
        let buy = offer("buyer", 3.69, 1_000.0, 50_000.0);
        let sell = offer("seller", 10.85, 5_000.0, 100_000.0);
        let text = describe_trade_size(&buy, &sell, &s);
        assert!(text.contains("AED"), "{text}");
        assert!(text.contains("executable"), "{text}");
        assert_eq!(crate::util::fmt_rate(3.69), "3.690");
    }
}
