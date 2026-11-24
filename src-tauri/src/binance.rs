//! Client for Binance P2P's public advertisement search endpoint.
//!
//! Semantics worth remembering: `tradeType: "BUY"` means the *taker* buys USDT, and
//! Binance answers with advertisements where the advertiser sells USDT. `tradeType:
//! "SELL"` is the mirror image. The `advertiser` object sits next to `adv`, not inside it.

use crate::model::*;
use crate::util::median;
use serde::{Deserialize, Deserializer, Serialize};
use std::collections::HashMap;

pub const SEARCH_URL: &str = "https://p2p.binance.com/bapi/c2c/v2/friendly/c2c/adv/search";

#[derive(Debug, Clone, Copy)]
pub struct Query<'a> {
    pub fiat: &'a str,
    pub side: LegSide,
    pub page: u32,
    pub rows: u32,
    pub publisher: PublisherFilter,
    pub pay_types: &'a [String],
    pub favorites: &'a [String],
    pub outlier_pct: f64,
}

impl<'a> Query<'a> {
    pub fn new(fiat: &'a str, side: LegSide) -> Self {
        Self {
            fiat,
            side,
            page: 1,
            rows: PAGE_ROWS,
            publisher: PublisherFilter::Any,
            pay_types: &[],
            favorites: &[],
            outlier_pct: 0.0,
        }
    }
}

#[derive(Debug, Clone, Default)]
pub struct RawLeg {
    pub offers: Vec<Offer>,
    pub total: i64,
    pub pay_methods: Vec<String>,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct SearchBody {
    fiat: String,
    page: u32,
    rows: u32,
    trade_type: String,
    asset: String,
    countries: Vec<String>,
    pro_merchant_ads: bool,
    shield_merchant: bool,
    publisher_type: Option<String>,
    pay_types: Vec<String>,
}

pub async fn search(client: &reqwest::Client, q: &Query<'_>) -> Result<RawLeg, String> {
    let body = SearchBody {
        fiat: q.fiat.trim().to_ascii_uppercase(),
        page: q.page.max(1),
        rows: q.rows.clamp(1, 50),
        trade_type: q.side.trade_type().to_string(),
        asset: ASSET.to_string(),
        countries: Vec::new(),
        pro_merchant_ads: false,
        shield_merchant: false,
        publisher_type: q.publisher.api_value().map(|v| v.to_string()),
        pay_types: q.pay_types.to_vec(),
    };

    let response = client
        .post(SEARCH_URL)
        .json(&body)
        .send()
        .await
        .map_err(|e| format!("Could not reach Binance P2P: {e}"))?;

    let status = response.status();
    let text = response
        .text()
        .await
        .map_err(|e| format!("Could not read Binance P2P response: {e}"))?;

    if !status.is_success() {
        return Err(match status.as_u16() {
            429 | 418 => "Binance is rate limiting this app — try a longer refresh interval".to_string(),
            451 => "Binance P2P is not available from this region".to_string(),
            code => format!("Binance P2P returned HTTP {code}"),
        });
    }

    let (offers, total, pay_methods) = parse_items(&text, q.favorites, q.outlier_pct)?;
    Ok(RawLeg {
        offers,
        total,
        pay_methods,
    })
}

/// Discover which payment methods are actually present for a fiat/leg.
pub async fn discover_pay_methods(
    client: &reqwest::Client,
    fiat: &str,
    side: LegSide,
) -> Result<Vec<String>, String> {
    let query = Query::new(fiat, side);
    let raw = search(client, &query).await?;
    Ok(raw.pay_methods)
}

fn parse_items(
    text: &str,
    favorites: &[String],
    outlier_pct: f64,
) -> Result<(Vec<Offer>, i64, Vec<String>), String> {
    let parsed: SearchResponse = serde_json::from_str(text)
        .map_err(|e| format!("Unexpected Binance P2P response: {e}"))?;

    let code = parsed.code.unwrap_or_default();
    if code != "000000" {
        let message = parsed
            .message
            .filter(|m| !m.trim().is_empty())
            .unwrap_or_else(|| format!("Binance returned code {code}"));
        return Err(message);
    }

    let items = parsed.data.unwrap_or_default();
    let total = parsed.total.unwrap_or(items.len() as i64);

    let prices: Vec<f64> = items
        .iter()
        .map(|item| item.adv.price)
        .filter(|price| *price > 0.0)
        .collect();
    let book_median = median(&prices);

    let mut offers = Vec::with_capacity(items.len());
    for item in items {
        if let Some(offer) = item_to_offer(item, book_median, outlier_pct, favorites) {
            offers.push(offer);
        }
    }

    let pay_methods = aggregate_pay_methods(&offers);
    Ok((offers, total, pay_methods))
}

fn item_to_offer(
    item: Item,
    book_median: f64,
    outlier_pct: f64,
    favorites: &[String],
) -> Option<Offer> {
    let adv = item.adv;
    if !adv.price.is_finite() || adv.price <= 0.0 {
        return None;
    }
    if adv.max_single_trans_amount > 0.0
        && adv.max_single_trans_amount < adv.min_single_trans_amount
    {
        return None;
    }
    if matches!(adv.is_tradable, Some(false)) {
        return None;
    }

    let advertiser = item.advertiser.unwrap_or_default();
    let user_no = advertiser.user_no.trim().to_string();
    let nickname = if advertiser.nick_name.trim().is_empty() {
        "P2P user".to_string()
    } else {
        advertiser.nick_name.trim().to_string()
    };

    let mut pay_types: Vec<String> = Vec::new();
    for method in adv.trade_methods {
        let pay_type = method.pay_type.trim().to_string();
        if !pay_type.is_empty() && !pay_types.contains(&pay_type) {
            pay_types.push(pay_type);
        }
    }

    let suspicious = outlier_pct > 0.0
        && book_median > 0.0
        && ((adv.price / book_median) - 1.0).abs() * 100.0 > outlier_pct;

    Some(Offer {
        adv_no: adv.adv_no,
        is_favorite: favorites.iter().any(|favorite| favorite == &user_no),
        user_no,
        nickname,
        user_type: advertiser.user_type,
        user_identity: advertiser.user_identity,
        completion_rate: advertiser.month_finish_rate,
        positive_rate: advertiser.positive_rate,
        month_orders: advertiser.month_order_count,
        active_secs: advertiser.active_time_in_second.max(0),
        price: adv.price,
        min_amount: adv.min_single_trans_amount,
        max_amount: adv.max_single_trans_amount,
        available_usdt: adv.tradable_quantity,
        pay_types,
        pay_time_limit: adv.pay_time_limit,
        suspicious,
    })
}

fn aggregate_pay_methods(offers: &[Offer]) -> Vec<String> {
    let mut counts: HashMap<&str, usize> = HashMap::new();
    for offer in offers {
        for method in &offer.pay_types {
            *counts.entry(method.as_str()).or_insert(0) += 1;
        }
    }
    let mut pairs: Vec<(&str, usize)> = counts.into_iter().collect();
    pairs.sort_by(|a, b| b.1.cmp(&a.1).then_with(|| a.0.cmp(b.0)));
    pairs
        .into_iter()
        .take(40)
        .map(|(name, _)| name.to_string())
        .collect()
}

// ---------------------------------------------------------------------------
// JSON shapes (Binance mixes strings and numbers, so every numeric field goes
// through a tolerant deserializer).
// ---------------------------------------------------------------------------

#[derive(Deserialize)]
struct SearchResponse {
    #[serde(default, deserialize_with = "de_opt_string")]
    code: Option<String>,
    #[serde(default)]
    message: Option<String>,
    #[serde(default, deserialize_with = "de_opt_i64")]
    total: Option<i64>,
    #[serde(default)]
    data: Option<Vec<Item>>,
}

#[derive(Deserialize, Default)]
struct Item {
    #[serde(default)]
    adv: Adv,
    #[serde(default)]
    advertiser: Option<Advertiser>,
}

#[derive(Deserialize, Default)]
#[serde(rename_all = "camelCase")]
struct Adv {
    #[serde(default)]
    adv_no: String,
    #[serde(default, deserialize_with = "de_f64")]
    price: f64,
    #[serde(default, deserialize_with = "de_f64")]
    min_single_trans_amount: f64,
    #[serde(default, deserialize_with = "de_f64")]
    max_single_trans_amount: f64,
    #[serde(default, deserialize_with = "de_f64")]
    tradable_quantity: f64,
    #[serde(default)]
    trade_methods: Vec<PayMethod>,
    #[serde(default, deserialize_with = "de_u32")]
    pay_time_limit: u32,
    #[serde(default, deserialize_with = "de_opt_bool")]
    is_tradable: Option<bool>,
}

#[derive(Deserialize, Default)]
#[serde(rename_all = "camelCase")]
struct PayMethod {
    #[serde(default)]
    pay_type: String,
}

#[derive(Deserialize, Default)]
#[serde(rename_all = "camelCase")]
struct Advertiser {
    #[serde(default)]
    user_no: String,
    #[serde(default)]
    nick_name: String,
    #[serde(default)]
    user_type: String,
    #[serde(default)]
    user_identity: String,
    #[serde(default, deserialize_with = "de_f64")]
    month_finish_rate: f64,
    #[serde(default, deserialize_with = "de_i64")]
    month_order_count: i64,
    #[serde(default, deserialize_with = "de_f64")]
    positive_rate: f64,
    #[serde(default, deserialize_with = "de_i64")]
    active_time_in_second: i64,
}

fn de_f64<'de, D>(deserializer: D) -> Result<f64, D::Error>
where
    D: Deserializer<'de>,
{
    let value = serde_json::Value::deserialize(deserializer)?;
    Ok(match value {
        serde_json::Value::Number(n) => n.as_f64().unwrap_or(0.0),
        serde_json::Value::String(s) => s.trim().parse().unwrap_or(0.0),
        _ => 0.0,
    })
}

fn de_i64<'de, D>(deserializer: D) -> Result<i64, D::Error>
where
    D: Deserializer<'de>,
{
    let value = serde_json::Value::deserialize(deserializer)?;
    Ok(match value {
        serde_json::Value::Number(n) => n
            .as_i64()
            .or_else(|| n.as_f64().map(|f| f as i64))
            .unwrap_or(0),
        serde_json::Value::String(s) => s.trim().parse().unwrap_or(0),
        _ => 0,
    })
}

fn de_u32<'de, D>(deserializer: D) -> Result<u32, D::Error>
where
    D: Deserializer<'de>,
{
    Ok(de_i64(deserializer)?.clamp(0, u32::MAX as i64) as u32)
}

fn de_opt_i64<'de, D>(deserializer: D) -> Result<Option<i64>, D::Error>
where
    D: Deserializer<'de>,
{
    let value = serde_json::Value::deserialize(deserializer)?;
    Ok(match value {
        serde_json::Value::Null => None,
        serde_json::Value::Number(n) => Some(
            n.as_i64()
                .or_else(|| n.as_f64().map(|f| f as i64))
                .unwrap_or(0),
        ),
        serde_json::Value::String(s) => s.trim().parse().ok(),
        _ => None,
    })
}

fn de_opt_string<'de, D>(deserializer: D) -> Result<Option<String>, D::Error>
where
    D: Deserializer<'de>,
{
    let value = serde_json::Value::deserialize(deserializer)?;
    Ok(match value {
        serde_json::Value::String(s) => Some(s),
        serde_json::Value::Null => None,
        other => Some(other.to_string()),
    })
}

fn de_opt_bool<'de, D>(deserializer: D) -> Result<Option<bool>, D::Error>
where
    D: Deserializer<'de>,
{
    let value = serde_json::Value::deserialize(deserializer)?;
    Ok(match value {
        serde_json::Value::Bool(b) => Some(b),
        serde_json::Value::String(s) => Some(s.eq_ignore_ascii_case("true")),
        _ => None,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    const FIXTURE: &str = r#"{
      "code": "000000",
      "message": null,
      "data": [
        {
          "adv": {
            "advNo": "111",
            "tradeType": "SELL",
            "asset": "USDT",
            "fiatUnit": "AED",
            "price": "3.690",
            "minSingleTransAmount": "5815",
            "maxSingleTransAmount": "5825",
            "tradableQuantity": "1576.24",
            "payTimeLimit": 15,
            "isTradable": true,
            "tradeMethods": [{ "payType": "ADCB" }, { "payType": "Aani" }, { "payType": "ADCB" }]
          },
          "advertiser": {
            "userNo": "user-1",
            "nickName": "hishamruk",
            "userType": "user",
            "userIdentity": "",
            "monthFinishRate": 1,
            "monthOrderCount": 11,
            "positiveRate": 0.9615,
            "activeTimeInSecond": 837
          }
        },
        {
          "adv": {
            "advNo": "222",
            "tradeType": "SELL",
            "asset": "USDT",
            "fiatUnit": "AED",
            "price": "3.819",
            "minSingleTransAmount": "5800",
            "maxSingleTransAmount": "24988",
            "tradableQuantity": "48050.08",
            "payTimeLimit": 30,
            "isTradable": true,
            "tradeMethods": [{ "payType": "BANK" }]
          },
          "advertiser": {
            "userNo": "user-2",
            "nickName": "User-3d5c1",
            "userType": "user",
            "monthFinishRate": 0.742,
            "monthOrderCount": 23,
            "positiveRate": 0.6666,
            "activeTimeInSecond": 5996
          }
        },
        {
          "adv": {
            "advNo": "333",
            "tradeType": "SELL",
            "asset": "USDT",
            "fiatUnit": "AED",
            "price": "3.692",
            "minSingleTransAmount": "3655",
            "maxSingleTransAmount": "3655",
            "tradableQuantity": "990.30",
            "payTimeLimit": 15,
            "tradeMethods": [{ "payType": "BANK" }]
          },
          "advertiser": null
        }
      ],
      "total": 284,
      "success": true
    }"#;

    #[test]
    fn parses_string_and_number_fields() {
        let (offers, total, pay_methods) = parse_items(FIXTURE, &[], 3.0).unwrap();
        assert_eq!(total, 284);
        assert_eq!(offers.len(), 3);

        let first = &offers[0];
        assert_eq!(first.user_no, "user-1");
        assert_eq!(first.price, 3.690);
        assert_eq!(first.min_amount, 5815.0);
        assert_eq!(first.month_orders, 11);
        assert_eq!(first.completion_rate, 1.0);
        assert_eq!(first.active_secs, 837);
        assert_eq!(first.pay_types, vec!["ADCB".to_string(), "Aani".to_string()]);
        // 3.690 vs median 3.691 -> not suspicious
        assert!(!first.suspicious);
        // null advertiser falls back gracefully
        assert_eq!(offers[2].nickname, "P2P user");
        assert!(pay_methods.contains(&"BANK".to_string()));
    }

    #[test]
    fn flags_book_outlier() {
        let (offers, _, _) = parse_items(FIXTURE, &[], 3.0).unwrap();
        let bait = offers.iter().find(|o| o.adv_no == "222").unwrap();
        // median of [3.690, 3.819, 3.692] is 3.692, so +3.44% > 3%
        assert!(bait.suspicious);
    }

    #[test]
    fn marks_favorites() {
        let favorites = vec!["user-2".to_string()];
        let (offers, _, _) = parse_items(FIXTURE, &favorites, 0.0).unwrap();
        assert!(offers.iter().any(|o| o.is_favorite && o.user_no == "user-2"));
        assert!(!offers.iter().filter(|o| o.user_no != "user-2").any(|o| o.is_favorite));
    }

    #[test]
    fn surfaces_api_errors() {
        let err = parse_items(r#"{"code":"10001","message":"blocked"}"#, &[], 0.0).unwrap_err();
        assert_eq!(err, "blocked");
    }
}

/// Hits the real Binance P2P endpoint. Run with:
/// `cargo test --lib live_p2p -- --ignored --nocapture`
#[cfg(test)]
mod live_tests {
    use super::*;

    fn client() -> reqwest::Client {
        reqwest::Client::builder()
            .user_agent("CrossRate/0.1 (integration test)")
            .timeout(std::time::Duration::from_secs(25))
            .build()
            .expect("client")
    }

    #[tokio::test]
    #[ignore = "requires network access to p2p.binance.com"]
    async fn live_p2p_aed_to_mad_round_trip() {
        let http = client();

        let buy = search(&http, &Query::new("AED", LegSide::Buy))
            .await
            .expect("AED buy book");
        let sell = search(&http, &Query::new("MAD", LegSide::Sell))
            .await
            .expect("MAD sell book");

        assert!(!buy.offers.is_empty(), "no AED offers returned");
        assert!(!sell.offers.is_empty(), "no MAD offers returned");

        let mut buy_sorted = buy.offers.clone();
        let mut sell_sorted = sell.offers.clone();
        crate::filters::sort(&mut buy_sorted, LegSide::Buy);
        crate::filters::sort(&mut sell_sorted, LegSide::Sell);

        let best_buy = &buy_sorted[0];
        let best_sell = &sell_sorted[0];
        let j = best_sell.price / best_buy.price;

        println!(
            "AED buy {:.3} ({} ads, {} parsed) · MAD sell {:.3} ({} ads, {} parsed) · j = {:.4} MAD/AED",
            best_buy.price, buy.total, buy.offers.len(), best_sell.price, sell.total, sell.offers.len(), j
        );
        println!(
            "  top buy:  {} [{:?}] finish {:.1}% orders {}",
            best_buy.nickname,
            best_buy.user_type,
            best_buy.completion_rate * 100.0,
            best_buy.month_orders
        );
        println!(
            "  top sell: {} [{:?}] finish {:.1}% orders {}",
            best_sell.nickname,
            best_sell.user_type,
            best_sell.completion_rate * 100.0,
            best_sell.month_orders
        );

        // Sanity bounds: AED/USDT ~3.67 and MAD/USDT ~10.8 at the time of writing.
        assert!(
            best_buy.price > 2.0 && best_buy.price < 6.0,
            "AED price looks wrong: {}",
            best_buy.price
        );
        assert!(
            best_sell.price > 5.0 && best_sell.price < 30.0,
            "MAD price looks wrong: {}",
            best_sell.price
        );
        assert!(j > 1.0 && j < 10.0, "cross rate looks wrong: {j}");
    }

    #[tokio::test]
    #[ignore = "requires network access to p2p.binance.com"]
    async fn live_pay_method_discovery() {
        let http = client();
        let methods = discover_pay_methods(&http, "MAD", LegSide::Buy)
            .await
            .expect("MAD pay methods");
        println!("MAD buy pay methods: {:?}", methods);
        assert!(!methods.is_empty(), "no payment methods discovered");
    }
}
