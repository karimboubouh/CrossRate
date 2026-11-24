# CrossRate

A native macOS menu bar app that watches Binance P2P and tells you when it is worth moving money from one fiat currency to another through USDT.

CrossRate polls both sides of the USDT market (buy USDT with **currency A**, sell USDT for **currency B**), computes the implied cross rate, and raises an alert when the rate clears your threshold — but only if a real, executable trade size exists on both legs.

Built with **Tauri v2** (Rust backend) and **React 19 + TypeScript** frontend. No account, no API keys, no cloud service: it talks directly to Binance's public P2P search endpoint and a free FX rate API.

| | |
|---|---|
| **Platform** | macOS 11 or newer (Apple Silicon and Intel) |
| **Status** | Early development (`0.1.0`), unsigned build, not distributed on the App Store |
| **Bridge asset** | USDT |
| **Default pair** | AED → MAD |
| **Default threshold** | `2.75` MAD per AED |

---

## What it does

1. Every N seconds, fetch the best **buy USDT** offers priced in currency A and the best **sell USDT** offers priced in currency B.
2. Rank the offers, apply your filters, and compute the implied cross rate `j`.
3. Compare `j` with your threshold `R` and with the mid-market FX rate.
4. If `j >= R` **and** a trade size exists that both counterparties will accept, activate the alert: green menu bar icon, live `j` beside it, a native notification and a macOS system sound.
5. Stay quiet until the opportunity disappears (`j < R` or the size becomes infeasible), so you get one alert per crossing, not one per poll.

## The cross-rate formula

Rates from Binance are quoted as fiat per USDT, so they are not directly comparable across currencies. CrossRate converts them into a single number:

```text
j = best sell rate (B per USDT) / best buy rate (A per USDT)
```

`j` is therefore expressed in **B per 1 A** — the same unit as the threshold `R` and as a normal FX quote.

Worked example, with the AED → MAD defaults:

| Leg | Binance trade type | Best rate | Meaning |
|---|---|---|---|
| Buy USDT with AED | `BUY` | `3.690` AED/USDT | you pay 3.690 AED per USDT |
| Sell USDT for MAD | `SELL` | `10.850` MAD/USDT | you receive 10.850 MAD per USDT |
| **Cross rate** | | **`j = 10.850 / 3.690 ≈ 2.9404`** | **2.9404 MAD per AED** |

With mid-market at `2.7260` MAD per AED, that is a `+7.9%` premium — a plausible real signal rather than a rounding artefact, which is exactly why the app also shows the spread versus mid.

### Alert state

```text
alert_active = (j >= R) AND executable size exists
```

- **Fires once** when `alert_active` transitions from `false` to `true` (notification + sound + menu bar badge).
- **Clears** when `j < R`, or when the executable size disappears — even if the rate is still attractive.
- The alert counter reports how many times the opportunity has re-appeared since launch.

### What is "executable size"?

Each advertisement has its own `min`/`max` transaction limits in its local fiat. A rate is meaningless if one side will only trade 200 AED while the other needs 50,000. CrossRate converts both legs' limits to USDT, intersects them, and exposes the resulting window:

```text
buy leg:   1000–50000 AED  @ 3.69  →  271–13,550 USDT
sell leg: 21700–100000 MAD @ 10.85 → 2000–9,216 USDT
executable:                            2000–9,216 USDT
```

Only if that intersection is non-empty does the alert activate. The popup shows the exact USDT and fiat amounts that can be traded end to end.

## Why the filters matter

Binance P2P is an open marketplace, so the top of the book is not always a real offer:

- **Bait ads.** An advert can sit at an attractive price while its advertiser is offline, has a poor completion rate, or rarely finishes orders. A real observed case: an AED sell ad at `3.819` against a `3.69` book median, from an advertiser with a **74% completion rate**, 23 monthly orders and no activity for ~100 minutes. CrossRate's default filters (≥90% completion, ≥20 monthly orders, online within 15 minutes) reject it, and any advert priced more than `3%` away from the page median is additionally flagged **⚠ suspicious** in the UI.
- **Fragility.** An alert that depends on a single advert can vanish the moment that advert is taken down. CrossRate recomputes the cross rate with the runner-up offer swapped in; if that alternative falls below `R`, the alert is marked **fragile**. When there is only one advert per leg there is no evidence either way, so the app does not claim fragility.
- **Strictness trade-off.** Filters also throw away legitimate but small or low-volume advertisers. That is deliberate: the app is tuned for offers you would actually accept, not for headline rates.

## Menu bar behaviour

- **Live `j` next to the tray icon** — formatted for the magnitude of the rate (up to 4 decimal places, e.g. `2.9404`).
- **Icon** — monochrome template icon when idle; **green** when an alert is active (unless the menu bar badge is disabled in settings).
- **Tooltip** — pair, current `j`, and `ALERT` when active.
- **Left click** — opens a 408×724 popup anchored under the icon; it hides automatically when it loses focus, like a native popover.
- **Right click** — menu with *Open CrossRate*, *Refresh now*, *Settings…*, *Quit CrossRate*.
- Running as a menu bar accessory app: no Dock icon and no app switcher entry; quit from the tray menu.

### What an alert says

The notification leads with the **rate you can trade at**, not with internal variables:

> **AED → MAD · 2.931**
> 10,000 AED buys 29,315 MAD. Bank rate 2.726 — +2,054 MAD (+7.5%) better.

The first line is `currency A → currency B · P2P rate`, and the body quotes the money at
the configurable **alert trade size** (default 10,000 of currency A) against the
mid-market rate. The same wording is used by **Settings → Alerts → Test alert**, so you
can judge the copy without waiting for a real opportunity.

### Which offer pair is used

The cheapest buy ad and the most expensive sell ad are not always tradable at the same
size. CrossRate therefore evaluates the filtered offers as **pairs** and headlines the
best pair whose limits actually overlap — so the price you see is one you can execute.
If no pair overlaps, the rate is still shown but the alert stays quiet and the popup
explains why ("Rate is above your target but not tradable").

Inside the popup: the current pair, a hero reading of the P2P rate against your target and the bank rate, the spread versus the bank, both leg cards with their best offer, the executable size, up to 8 ranked offers per leg, and the settings screen (pair picker, threshold, refresh, amount bands, per-leg filters, favourites, alert options, launch at login).

## Configuration

Settings are saved as JSON and applied on the next poll (a save wakes the polling loop immediately).

| Setting | Default | Notes |
|---|---|---|
| Currency A → Currency B | AED → MAD | 64 common currencies in the picker; the backend accepts any 3-letter ISO code |
| Alert threshold `R` | `2.75` B per A | Must be positive |
| Alert trade size | `10,000` A | Amount quoted in the notification and profit line (e.g. “10,000 AED buys 29,315 MAD”) |
| Refresh interval | `15 s` | Clamped to 5–600 s |
| Buy-leg amount band | `1,000–50,000` A | Ads outside the band are filtered out |
| Sell-leg amount band | `5,000–100,000` B | Same, in currency B |
| Min completion rate (per leg) | `90%` | 30-day completion rate |
| Min positive-feedback rate (per leg) | `0%` | 30-day positive rate |
| Min 30-day orders (per leg) | `20` | Excludes brand-new advertisers |
| Online only (per leg) | on, within `900 s` | Based on Binance's last-active signal |
| Publisher type (per leg) | any | any / merchant / P2P user |
| Payment methods (per leg) | any | Discovered from the live book, applied server-side via `payTypes` |
| Outlier flag | `3%` | Deviation from page median before an offer is marked ⚠ |
| Favourites | none | Highlighted only — they never bypass filters |
| Notifications | on | Native notification centre |
| Alert sound | on, `Glass` | Any of Glass, Ping, Submarine, Funk, Hero, Pop, Purr, Tink (`afplay`) |
| Menu bar badge | on | Green icon while alerting |
| Launch at login | on | macOS LaunchAgent |
| Mid-market override | off | Auto from the FX API; can be pinned manually |

The mid-market rate comes from `open.er-api.com` (keyless, ECB-derived), cached for 30 minutes with a 2-minute retry backoff after failures. Spread versus mid is `(j / mid - 1) × 100`.

## Build and run

### Prerequisites

- macOS 11+
- Xcode Command Line Tools (`xcode-select --install`)
- Node.js 20+ and pnpm
- Rust stable 1.77.2+ — Tauri compiles the backend with `cargo`

### Commands

```bash
pnpm install       # install JS dependencies (store is configured in .npmrc)
pnpm app:dev       # Tauri dev: Vite + Rust, hot reload
pnpm app:build     # release bundle: .app and .dmg
```

`pnpm app:build` writes the bundles to `src-tauri/target/release/bundle/` (`macos/CrossRate.app` and `dmg/CrossRate_0.1.0_*.dmg`). The first `cargo build` can take 6–10 minutes while dependencies compile; later builds are incremental.

Other useful scripts: `pnpm build` type-checks and builds the frontend only, `pnpm dev` runs the Vite dev server on its own (the UI then runs in a browser mock mode).

### Sandbox / custom cache locations

If you are building inside a sandbox that cannot write to `~/.cargo` or the default pnpm store, point the caches at the workspace (both are gitignored):

```bash
export CARGO_HOME="$PWD/.cargo-home"
pnpm install   # .npmrc already sets store-dir=.pnpm-store
CARGO_HOME="$PWD/.cargo-home" pnpm app:build
```

There is no signing or notarisation configured. Locally built apps run directly; if you distribute the `.dmg`, recipients may need **System Settings → Privacy & Security → Open Anyway**, or you can supply your own Developer ID signing identity.

## No API keys

Everything CrossRate uses is public and keyless:

- `POST https://p2p.binance.com/bapi/c2c/v2/friendly/c2c/adv/search` — Binance P2P advertisement search
- `GET https://open.er-api.com/v6/latest/{CURRENCY}` — mid-market reference rates

No Binance account, API key, secret, or login is required, and no data is sent anywhere except those two hosts. Settings stay on your Mac.

## How it works

```text
┌─────────────────────────── Tauri process (Rust) ───────────────────────────┐
│                                                                            │
│  engine.rs ── polling loop (refresh interval, failure backoff)             │
│     │  every tick:                                                         │
│     ├── fx.rs      → mid-market rate (30 min cache, manual override)       │
│     ├── binance.rs → search both legs concurrently (tokio::join!)          │
│     │     BUY  currency A, rows=20, page 2/3 if everything was filtered    │
│     │     SELL currency B, same                                            │
│     ├── filters.rs → amount-band intersection, quality filters, ranking,   │
│     │                one advert per advertiser, bait/outlier flag          │
│     ├── compose()  → j, j_second, fragile, executable size, spread         │
│     └── alerts / tray → notification + sound + live menu bar title/icon    │
│                                                                            │
│  Snapshot (serde, camelCase) ──event──▶ React webview                       │
│  Settings JSON ◀──commands──▶ React settings screen                        │
└────────────────────────────────────────────────────────────────────────────┘
```

- `src-tauri/src/model.rs` — domain types, settings defaults, snapshot schema, currency list.
- `src-tauri/src/binance.rs` — HTTP client and tolerant parser for Binance's mixed string/number JSON (the `advertiser` object sits next to `adv`, not inside it).
- `src-tauri/src/filters.rs` — per-leg filtering, price ranking, dedupe per advertiser, and the size-feasibility intersection.
- `src-tauri/src/engine.rs` — polling loop, cross-rate and alert state machine, fragility calculation, snapshot emission.
- `src-tauri/src/fx.rs` — keyless mid-market fetch with cache and retry backoff.
- `src-tauri/src/tray.rs`, `alerts.rs`, `settings.rs`, `commands.rs` — tray item and menu, notifications/sounds, persisted settings, IPC surface.
- `src/` — React 19 dashboard and settings UI. Without the Tauri IPC bridge (plain `pnpm dev`) the UI serves mock snapshots so it can be previewed in a browser.

Design notes: the Rust side performs all HTTP, so there are no CORS issues and no browser networking; polls of the two legs run concurrently; settings are written atomically (temp file + rename); consecutive poll failures are backed off up to 120 s; and the native notification path falls back to `osascript` on unsigned builds.

## Troubleshooting

**No notifications appear.** macOS requires permission per app: **System Settings → Notifications → CrossRate → Allow Notifications**. Unsigned development builds can fail the native notification call; CrossRate then falls back to an AppleScript notification, which uses the Script Editor permission instead. You can trigger a sample alert from the settings screen (`test_alert` command) to check sound and notification together.

**Rate-limited by Binance.** A `429` (or `418`) response means the public endpoint is throttling you. Increase the refresh interval (30–60 s is plenty for this use case). The app also backs off automatically after consecutive failures, up to 120 s.

**"No offers found" / a leg shows nothing.** This almost always means the filters are too strict, not that the market is empty. Check the leg counters (fetched / passed / total): if `total` is large but `passed` is 0, relax your criteria — lower the minimum completion rate or monthly order count, widen the online window or turn *online only* off, clear payment-method filters, or widen the amount band. Region blocks surface separately as "Binance P2P is not available from this region" (HTTP 451).

**The rate shows but no alert fires.** Two distinct conditions gate the alert: `j >= R` (rate met) and a non-empty executable size. The popup reports which one failed — often the buy and sell amount bands do not overlap once converted to USDT.

**No mid-market rate / no spread shown.** The FX API may be unreachable. The last good rate is kept and labelled as stale; if none exists, set a manual mid-market override in settings. The alert itself only needs Binance data.

**The app has no Dock icon.** By design: CrossRate is a menu bar accessory app. Use the tray menu to quit.

**The menu bar shows no rate.** It appears once both legs return usable offers. A tooltip of "waiting for data" plus the error text in the popup points at what is failing.

**Where is my configuration?** `~/Library/Application Support/com.crossrate.desktop/settings.json` (atomic writes, readable JSON). Override the directory with `CROSSRATE_CONFIG_DIR` for testing. Set `CROSSRATE_NO_AUTOHIDE=1` if you do not want the popup to hide when it loses focus.

## Limitations

- macOS only, single active pair at a time, USDT as the bridge asset.
- Binance P2P's search endpoint is public but unofficial; its response shape or rate limits can change without notice.
- The filters intentionally exclude low-quality and low-volume adverts, so a real opportunity may be invisible if the only offers are poor counterparts.
- Rates shown are indicative marketplace quotes, not executable orders. Nothing here is financial advice; P2P transfers carry counterparty and payment-method risk.

## License

[MIT](LICENSE) © 2026 Karim Boubouh
