//! Core domain types shared between the polling engine, the tray and the frontend.

use serde::{Deserialize, Serialize};

/// Binance P2P asset we always bridge through.
pub const ASSET: &str = "USDT";
/// Offers requested per page from the Binance P2P search endpoint.
pub const PAGE_ROWS: u32 = 20;
/// How many offers we forward to the UI per leg.
pub const MAX_OFFERS_SHOWN: usize = 8;

/// Alert sounds that ship with macOS (`/System/Library/Sounds/<name>.aiff`).
pub const SOUNDS: &[&str] = &["Glass", "Ping", "Submarine", "Funk", "Hero", "Pop", "Purr", "Tink"];

/// Currencies we offer in the picker. Any ISO code can still be typed manually.
pub const FIATS: &[(&str, &str)] = &[
    ("AED", "UAE Dirham"),
    ("ARS", "Argentine Peso"),
    ("AUD", "Australian Dollar"),
    ("AZN", "Azerbaijani Manat"),
    ("BDT", "Bangladeshi Taka"),
    ("BHD", "Bahraini Dinar"),
    ("BRL", "Brazilian Real"),
    ("CAD", "Canadian Dollar"),
    ("CHF", "Swiss Franc"),
    ("CLP", "Chilean Peso"),
    ("CNY", "Chinese Yuan"),
    ("COP", "Colombian Peso"),
    ("CZK", "Czech Koruna"),
    ("DKK", "Danish Krone"),
    ("DZD", "Algerian Dinar"),
    ("EGP", "Egyptian Pound"),
    ("EUR", "Euro"),
    ("GBP", "British Pound"),
    ("GEL", "Georgian Lari"),
    ("GHS", "Ghanaian Cedi"),
    ("HKD", "Hong Kong Dollar"),
    ("HUF", "Hungarian Forint"),
    ("IDR", "Indonesian Rupiah"),
    ("INR", "Indian Rupee"),
    ("JOD", "Jordanian Dinar"),
    ("JPY", "Japanese Yen"),
    ("KES", "Kenyan Shilling"),
    ("KGS", "Kyrgyzstani Som"),
    ("KRW", "South Korean Won"),
    ("KWD", "Kuwaiti Dinar"),
    ("KZT", "Kazakhstani Tenge"),
    ("LKR", "Sri Lankan Rupee"),
    ("MAD", "Moroccan Dirham"),
    ("MXN", "Mexican Peso"),
    ("MYR", "Malaysian Ringgit"),
    ("NGN", "Nigerian Naira"),
    ("NOK", "Norwegian Krone"),
    ("NPR", "Nepalese Rupee"),
    ("NZD", "New Zealand Dollar"),
    ("OMR", "Omani Rial"),
    ("PEN", "Peruvian Sol"),
    ("PHP", "Philippine Peso"),
    ("PKR", "Pakistani Rupee"),
    ("PLN", "Polish Zloty"),
    ("QAR", "Qatari Riyal"),
    ("RON", "Romanian Leu"),
    ("RUB", "Russian Ruble"),
    ("SAR", "Saudi Riyal"),
    ("SEK", "Swedish Krona"),
    ("SGD", "Singapore Dollar"),
    ("THB", "Thai Baht"),
    ("TND", "Tunisian Dinar"),
    ("TRY", "Turkish Lira"),
    ("TWD", "New Taiwan Dollar"),
    ("TZS", "Tanzanian Shilling"),
    ("UAH", "Ukrainian Hryvnia"),
    ("UGX", "Ugandan Shilling"),
    ("USD", "US Dollar"),
    ("UYU", "Uruguayan Peso"),
    ("UZS", "Uzbekistani Som"),
    ("VES", "Venezuelan Bolivar"),
    ("VND", "Vietnamese Dong"),
    ("ZAR", "South African Rand"),
    ("ZMW", "Zambian Kwacha"),
];

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum LegSide {
    /// We buy USDT with fiat A (Binance `tradeType: "BUY"`).
    Buy,
    /// We sell USDT for fiat B (Binance `tradeType: "SELL"`).
    Sell,
}

impl LegSide {
    pub fn trade_type(self) -> &'static str {
        match self {
            LegSide::Buy => "BUY",
            LegSide::Sell => "SELL",
        }
    }

    pub fn parse(value: &str) -> Option<Self> {
        match value.trim().to_ascii_lowercase().as_str() {
            "buy" => Some(LegSide::Buy),
            "sell" => Some(LegSide::Sell),
            _ => None,
        }
    }

    pub fn fiat(self, settings: &Settings) -> String {
        match self {
            LegSide::Buy => settings.fiat_a.clone(),
            LegSide::Sell => settings.fiat_b.clone(),
        }
    }

    /// Amount band that applies to this leg, expressed in the leg's own fiat.
    pub fn amount_band(self, settings: &Settings) -> (f64, f64) {
        match self {
            LegSide::Buy => (settings.buy_min_amount, settings.buy_max_amount),
            LegSide::Sell => (settings.sell_min_amount, settings.sell_max_amount),
        }
    }

    pub fn filters<'a>(self, settings: &'a Settings) -> &'a SideFilters {
        match self {
            LegSide::Buy => &settings.buy_filters,
            LegSide::Sell => &settings.sell_filters,
        }
    }

    pub fn favorites<'a>(self, settings: &'a Settings) -> &'a [String] {
        match self {
            LegSide::Buy => &settings.favorite_buy_users,
            LegSide::Sell => &settings.favorite_sell_users,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum PublisherFilter {
    Any,
    Merchant,
    User,
}

impl Default for PublisherFilter {
    fn default() -> Self {
        PublisherFilter::Any
    }
}

impl PublisherFilter {
    /// Value passed to Binance's `publisherType` parameter.
    pub fn api_value(self) -> Option<&'static str> {
        match self {
            PublisherFilter::Any => None,
            PublisherFilter::Merchant => Some("merchant"),
            PublisherFilter::User => Some("user"),
        }
    }
}

/// Server- and client-side filters applied to one leg of the trade.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct SideFilters {
    pub publisher: PublisherFilter,
    /// Minimum 30-day completion rate, 0..1.
    pub min_completion_rate: f64,
    /// Minimum positive-feedback rate, 0..1.
    pub min_positive_rate: f64,
    /// Minimum number of completed 30-day orders.
    pub min_month_orders: u32,
    /// Only keep advertisers seen online within `online_within_secs`.
    pub online_only: bool,
    pub online_within_secs: u64,
    /// Empty means "any payment method".
    pub pay_types: Vec<String>,
}

impl Default for SideFilters {
    fn default() -> Self {
        Self {
            publisher: PublisherFilter::Any,
            min_completion_rate: 0.90,
            min_positive_rate: 0.0,
            min_month_orders: 20,
            online_only: true,
            online_within_secs: 900,
            pay_types: Vec::new(),
        }
    }
}

impl SideFilters {
    pub fn sanitize(&mut self) {
        self.min_completion_rate = clamp01(self.min_completion_rate);
        self.min_positive_rate = clamp01(self.min_positive_rate);
        self.min_month_orders = self.min_month_orders.min(100_000);
        self.online_within_secs = self.online_within_secs.clamp(30, 86_400);
        let mut seen: Vec<String> = Vec::new();
        for p in self.pay_types.iter().map(|p| p.trim()) {
            if !p.is_empty() && !seen.iter().any(|s| s == p) && seen.len() < 40 {
                seen.push(p.to_string());
            }
        }
        self.pay_types = seen;
    }
}

fn clamp01(v: f64) -> f64 {
    if !v.is_finite() {
        0.0
    } else {
        v.clamp(0.0, 1.0)
    }
}

/// Everything the user can configure. Persisted as JSON.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct Settings {
    pub fiat_a: String,
    pub fiat_b: String,
    /// Alert threshold R, expressed in fiat B per 1 fiat A.
    pub threshold: f64,
    pub refresh_secs: u64,
    pub buy_min_amount: f64,
    pub buy_max_amount: f64,
    pub sell_min_amount: f64,
    pub sell_max_amount: f64,
    pub buy_filters: SideFilters,
    pub sell_filters: SideFilters,
    pub favorite_buy_users: Vec<String>,
    pub favorite_sell_users: Vec<String>,
    pub alert_notification: bool,
    pub alert_sound: bool,
    pub alert_badge: bool,
    pub sound_name: String,
    /// Amount of currency A used in alert copy, e.g. "10,000 AED buys …".
    pub notify_amount: f64,
    /// Pin the mid-market rate instead of using the FX API.
    pub mid_override: Option<f64>,
    /// Flag offers deviating more than this % from the book median.
    pub outlier_pct: f64,
    pub launch_at_login: bool,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            fiat_a: "AED".to_string(),
            fiat_b: "MAD".to_string(),
            threshold: 2.75,
            refresh_secs: 15,
            buy_min_amount: 1_000.0,
            buy_max_amount: 50_000.0,
            sell_min_amount: 5_000.0,
            sell_max_amount: 100_000.0,
            buy_filters: SideFilters::default(),
            sell_filters: SideFilters::default(),
            favorite_buy_users: Vec::new(),
            favorite_sell_users: Vec::new(),
            alert_notification: true,
            alert_sound: true,
            alert_badge: true,
            sound_name: "Glass".to_string(),
            notify_amount: 10_000.0,
            mid_override: None,
            outlier_pct: 3.0,
            launch_at_login: true,
        }
    }
}

impl Settings {
    pub fn validate(mut self) -> Result<Settings, String> {
        self.fiat_a = normalize_fiat(&self.fiat_a)
            .ok_or_else(|| "Currency A must be a 3-letter code".to_string())?;
        self.fiat_b = normalize_fiat(&self.fiat_b)
            .ok_or_else(|| "Currency B must be a 3-letter code".to_string())?;
        if self.fiat_a == self.fiat_b {
            return Err("Currencies A and B must be different".to_string());
        }
        if !self.threshold.is_finite() || self.threshold <= 0.0 {
            return Err("Alert threshold must be a positive number".to_string());
        }
        self.refresh_secs = self.refresh_secs.clamp(5, 600);

        for (label, value) in [
            ("Buy minimum", self.buy_min_amount),
            ("Buy maximum", self.buy_max_amount),
            ("Sell minimum", self.sell_min_amount),
            ("Sell maximum", self.sell_max_amount),
        ] {
            if !value.is_finite() || value < 0.0 {
                return Err(format!("{label} amount must be zero or positive"));
            }
        }
        if self.buy_max_amount > 0.0 && self.buy_max_amount < self.buy_min_amount {
            return Err("Buy maximum must be greater than or equal to the minimum".to_string());
        }
        if self.sell_max_amount > 0.0 && self.sell_max_amount < self.sell_min_amount {
            return Err("Sell maximum must be greater than or equal to the minimum".to_string());
        }

        if !SOUNDS.contains(&self.sound_name.as_str()) {
            self.sound_name = "Glass".to_string();
        }
        if !self.notify_amount.is_finite() || self.notify_amount <= 0.0 {
            self.notify_amount = 10_000.0;
        }
        if !self.outlier_pct.is_finite() {
            self.outlier_pct = 3.0;
        }
        self.outlier_pct = self.outlier_pct.clamp(0.0, 50.0);
        if let Some(mid) = self.mid_override {
            if !mid.is_finite() || mid <= 0.0 {
                self.mid_override = None;
            }
        }
        self.buy_filters.sanitize();
        self.sell_filters.sanitize();
        self.favorite_buy_users.retain(|u| !u.trim().is_empty());
        self.favorite_sell_users.retain(|u| !u.trim().is_empty());
        self.favorite_buy_users.dedup();
        self.favorite_sell_users.dedup();
        Ok(self)
    }
}

pub fn normalize_fiat(raw: &str) -> Option<String> {
    let code = raw.trim().to_ascii_uppercase();
    if code.len() == 3 && code.chars().all(|c| c.is_ascii_alphabetic()) {
        Some(code)
    } else {
        None
    }
}

/// A single P2P advertisement, already normalised out of Binance's string-heavy JSON.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Offer {
    pub adv_no: String,
    pub user_no: String,
    pub nickname: String,
    /// `user` or `merchant`.
    pub user_type: String,
    pub user_identity: String,
    /// 30-day completion rate, 0..1.
    pub completion_rate: f64,
    /// 30-day positive feedback rate, 0..1.
    pub positive_rate: f64,
    pub month_orders: i64,
    /// Seconds since the advertiser was last active.
    pub active_secs: i64,
    pub price: f64,
    pub min_amount: f64,
    pub max_amount: f64,
    pub available_usdt: f64,
    pub pay_types: Vec<String>,
    pub pay_time_limit: u32,
    pub is_favorite: bool,
    /// Price is far away from the rest of the book page (likely bait/stale ad).
    pub suspicious: bool,
}

/// One side of the cross trade, as shown in the popup.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LegView {
    pub side: LegSide,
    pub fiat: String,
    pub best: Option<Offer>,
    pub offers: Vec<Offer>,
    /// Offers returned by Binance for this page.
    pub fetched: usize,
    /// Offers that survived the filters.
    pub passed: usize,
    /// Total offers Binance reports for this fiat/trade type.
    pub total: i64,
    pub error: Option<String>,
    /// Payment methods observed on this page.
    pub pay_methods: Vec<String>,
}

/// What is actually executable once both legs' limits are intersected.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Feasible {
    pub ok: bool,
    pub reason: String,
    pub usdt_min: f64,
    pub usdt_max: f64,
    pub a_min: f64,
    pub a_max: f64,
    pub b_min: f64,
    pub b_max: f64,
}

impl Feasible {
    pub fn none(reason: impl Into<String>) -> Self {
        Self {
            ok: false,
            reason: reason.into(),
            usdt_min: 0.0,
            usdt_max: 0.0,
            a_min: 0.0,
            a_max: 0.0,
            b_min: 0.0,
            b_max: 0.0,
        }
    }
}

/// A complete picture of the market at one poll, pushed to the UI and the tray.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Snapshot {
    pub ts: i64,
    pub rev: u64,
    pub fiat_a: String,
    pub fiat_b: String,
    pub buy: LegView,
    pub sell: LegView,
    /// Implied cross rate: B per A.
    pub j: Option<f64>,
    /// Runner-up cross rate (best alternative pair, not just second ads).
    pub j_second: Option<f64>,
    /// True when the alert depends on a single advertisement.
    pub fragile: bool,
    pub threshold: f64,
    pub mid: Option<f64>,
    /// `err-api`, `manual` or `unavailable`.
    pub mid_source: String,
    pub mid_age_secs: i64,
    /// (j / mid - 1) * 100.
    pub spread_pct: Option<f64>,
    pub alert_active: bool,
    pub alert_since: Option<i64>,
    pub alert_count: u32,
    pub feasible: Feasible,
    /// j >= threshold, regardless of size feasibility.
    pub rate_met: bool,
    pub stale: bool,
    pub error: Option<String>,
    pub next_poll_ms: i64,
    pub refresh_secs: u64,
    pub alert_badge: bool,
    pub alert_sound: bool,
    pub alert_notification: bool,
    pub sound_name: String,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct FiatInfo {
    pub code: String,
    pub name: String,
}

pub fn fiat_list() -> Vec<FiatInfo> {
    FIATS
        .iter()
        .map(|(code, name)| FiatInfo {
            code: (*code).to_string(),
            name: (*name).to_string(),
        })
        .collect()
}
