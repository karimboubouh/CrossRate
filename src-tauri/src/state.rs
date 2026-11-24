//! Process-wide state shared by the polling engine and the IPC commands.

use crate::fx::FxState;
use crate::model::{Settings, Snapshot};
use std::collections::HashMap;
use std::sync::atomic::AtomicU64;
use std::time::Duration;
use tokio::sync::{Mutex, Notify, RwLock};

pub const USER_AGENT: &str = "Mozilla/5.0 (Macintosh; Intel Mac OS X 10_15_7) AppleWebKit/605.1.15 (KHTML, like Gecko) Version/17.0 Safari/605.1.15";

/// Alert bookkeeping so we notify once per crossing.
#[derive(Debug, Default)]
pub struct AlertRuntime {
    pub active: bool,
    pub since: Option<i64>,
    pub count: u32,
}

pub struct AppState {
    pub settings: RwLock<Settings>,
    pub settings_rev: AtomicU64,
    pub snapshot: RwLock<Option<Snapshot>>,
    pub http: reqwest::Client,
    /// Wakes the polling loop immediately (manual refresh / settings change).
    pub wake: Notify,
    pub fx: RwLock<FxState>,
    pub alert: Mutex<AlertRuntime>,
    /// Cached payment-method discovery: key -> (fetched_at_ms, methods).
    pub pay_cache: Mutex<HashMap<String, (i64, Vec<String>)>>,
}

impl AppState {
    pub fn new(settings: Settings) -> Self {
        let mut headers = reqwest::header::HeaderMap::new();
        headers.insert(
            reqwest::header::ORIGIN,
            reqwest::header::HeaderValue::from_static("https://p2p.binance.com"),
        );
        headers.insert(
            reqwest::header::ACCEPT,
            reqwest::header::HeaderValue::from_static("application/json"),
        );

        let http = reqwest::Client::builder()
            .timeout(Duration::from_secs(20))
            .connect_timeout(Duration::from_secs(10))
            .user_agent(USER_AGENT)
            .default_headers(headers)
            .build()
            .expect("could not build the HTTP client");

        Self {
            settings: RwLock::new(settings),
            settings_rev: AtomicU64::new(1),
            snapshot: RwLock::new(None),
            http,
            wake: Notify::new(),
            fx: RwLock::new(FxState::default()),
            alert: Mutex::new(AlertRuntime::default()),
            pay_cache: Mutex::new(HashMap::new()),
        }
    }
}
