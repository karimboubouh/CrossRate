# CrossRate — work handoff / resume note

**Status: PAUSED by user (macOS restart to apply permissions). Nothing running, no code written yet.**
Resume by saying "resume CrossRate" or "continue".

## What CrossRate is
macOS menu bar app (Tauri v2 + React/TS) that polls Binance P2P and alerts when
`j = best_sell_rate(fiat B) / best_buy_rate(fiat A)` reaches threshold `R` (B per A).

## Locked decisions (from user Q&A)
| Topic | Decision |
|---|---|
| Stack | Tauri v2 + React + TypeScript (Rust backend does HTTP → no CORS) |
| Platform | macOS only |
| Default pair | **AED → MAD**, any A/B pair must be selectable |
| Threshold R | **Absolute rate in B per A** + show mid-market for reference |
| Mid-market | Auto-fetch from free keyless FX API (`open.er-api.com/v6/latest/AED`, MAD=2.726018, verified) |
| Alert cadence | Once per crossing (j crosses above R), quiet until it drops below |
| Menu bar badge | Show live `j` next to tray icon; icon green when active (idle = template/monochrome) |
| Amount filters | Separate bands: buy leg in fiat A, sell leg in fiat B |
| Extra filters | (1) payment method per currency — server-side `payTypes` (2) online/verified merchants only (3) min 30d order count |
| Favorites | Highlight only — do **not** bypass filters (user did not select bypass) |
| Sound | Choose from macOS system sounds (Glass/Ping/Submarine/Funk/…) via `afplay` |
| Launch | Launch at login + auto-start polling |
| Pair scope | Single active pair |

## Verified API facts (Binance P2P)
- `POST https://p2p.binance.com/bapi/c2c/v2/friendly/c2c/adv/search`
  body: `{fiat,page,rows,tradeType,asset:"USDT",countries:[],proMerchantAds:false,shieldMerchant:false,publisherType:null,payTypes:[]}`
- `tradeType:"BUY"` = taker buys USDT (response ads are `adv.tradeType:"SELL"`); `"SELL"` = taker sells USDT.
- **`advertiser` is at item level**, sibling of `adv` — NOT `adv.advertiser` (easy parser bug).
- `adv` fields used: `price`, `minSingleTransAmount`, `maxSingleTransAmount`, `tradableQuantity`, `tradeMethods[].payType`, `payTimeLimit`, `isTradable`.
- `advertiser` fields: `userNo`, `nickName`, `userType` ("user"|"merchant"), `userIdentity` ("MASS_MERCHANT"), `monthFinishRate` (0..1), `monthOrderCount`, `positiveRate`, `activeTimeInSecond` (≈ online signal), `userGrade`.
- `payTypes` filters server-side (MAD book 205 → 98 total with `["cihmorocco"]`).
- No pay-method-list endpoint (`filter-options` 404) → **discover payTypes by aggregating `tradeMethods` from a rows=20 unfiltered page**.
- Live sample: AED buy 3.690/3.692/3.693 (limits e.g. 5815–5825); AED sell top ad 3.819 is **bait** (74% finish, 23 orders, idle 5996s) → filters must reject it. MAD buy 10.87 (205 ads), MAD sell 10.85 (340 ads) → j≈2.94 vs mid 2.726 = +7.9% genuine premium (MAD USD shortage). So a real alert is plausible.
- Advertiser link that works: `https://p2p.binance.com/en/advertiserDetail?advertiser_no=<userNo>` (HTTP 200).

## Engine logic (to implement)
- Poll both legs concurrently, rows=20, page 2 only if all rows filtered out.
- Filters per leg: amount-band intersection, min completion rate, min positive rate, min 30d orders, online (activeTimeInSecond ≤ N), publisher type, payTypes, favorites highlight.
- Suspicion badge: price deviates > `outlier_pct` (default 3%) from page median → ⚠ (flags bait ads; the AED 3.819 is +3.8%).
- `j = sell_best / buy_best`; `j_second` from runner-ups → `fragile` flag when alert depends on a single offer.
- Alert active = `j >= R` **and** executable size exists (buy band ∩ buy offer, sell band ∩ sell offer, intersect in USDT).
- Mid-market cache TTL 1h; optional manual override.
- Tray title = j (4dp); icon alert/idle; native notification + `afplay`; deep link opens advertiser page.
- Settings persisted as JSON in app config dir; `CROSSRATE_CONFIG_DIR` env override for tests.

## Build/environment notes (important)
- **Real workspace path:** `/Users/karimboubouh/Workspace/D/CrossRate` (runtime summary typo'd it as `karimbouh`).
- Sandbox is workspace-write: must relocate caches inside workspace →
  `CARGO_HOME=<ws>/.cargo-home`, pnpm via `.npmrc` `store-dir`, npm `--cache`.
- `~/.npm` is root-owned → never use plain `npm`; use `pnpm` with cache override.
- Versions: tauri 2.12.7, tauri-build 2.7.6, CLI 2.12.6, api 2.12.5,
  plugin-notification 2.8.1, plugin-autostart 2.7.0, react 19.3.1, vite 8.3.8, TS 7.0.2.
- `TrayIconEvent::Click { position: PhysicalPosition<f64>, rect: Rect, .. }` (verified on docs.rs 2.12.7) — use `position.x - width/2`, `position.y + 10`.
- Allow ~6–10 min first `cargo build` (fresh deps in workspace CARGO_HOME).
- `screencapture` is **denied** → verify UI visually via Chrome headless:
  `/Applications/Google Chrome.app/Contents/MacOS/Google Chrome --headless=new --user-data-dir=/tmp/chrome-prof --screenshot=... --window-size=408,720 <url>`
  → build a browser mock mode in the frontend (`window.__TAURI_INTERNALS__` absent ⇒ mock data).
- Icons already generated: `src-tauri/icons/` (icon.png 1024, icns, ico, sizes 32–512, tray-idle.png 44, tray-alert.png 44).
- Empty dirs already created: `src/`, `dist/`, `scripts/`, `src-tauri/src/`, `src-tauri/capabilities/`.
- `dist/` must contain something before `cargo build` (tauri-build validates frontendDist).

## Next steps on resume (in order)
1. Write `package.json`, `.npmrc`, `tsconfig`, `vite.config.ts`, `index.html`, `dist/index.html` placeholder.
2. Write `src-tauri/Cargo.toml`, `build.rs`, `tauri.conf.json`, `capabilities/default.json` + minimal compilable `lib.rs`/`main.rs`; start `cargo build` in **background** to warm deps.
3. Write Rust modules: model, settings, binance, filters, fx, engine, tray, alerts, commands + unit tests (fixtures from probes).
4. Write React UI: dashboard (pair chips, hero j/R/mid gauge, leg cards, profit preview, offer rows), settings tabs, fiat picker, pay-method chips, favorites, toggles; browser mock mode.
5. `pnpm install`, `vite build`, Chrome-headless screenshots → iterate visual design.
6. Rust tests + `pnpm tauri build` → .app + .dmg; launch, verify tray title + alert; write README.
7. Present artifacts with usage instructions (grant notification permission on first alert).

## Still to confirm after restart
- Notification permission prompt appears for the unsigned dev app (fallback: `osascript display notification`).
- Whether `tauri.conf.json` accepts `app.macOSPrivateApi` + `tauri` feature `macos-private-api` for the rounded transparent popup (if not, fall back to non-transparent decorated-less window).
