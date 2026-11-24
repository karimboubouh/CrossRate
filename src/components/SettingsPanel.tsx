import { useEffect, useMemo, useState } from "react";
import type { FiatInfo, LegSide, PublisherFilter, Settings, SideFilters } from "../types";
import { api, SOUNDS } from "../lib/api";
import { prettyPayType } from "../lib/format";
import { FiatPicker } from "./FiatPicker";
import { Field, NumField, Section, Segmented, Toggle } from "./ui";
import { ArrowRightIcon, BackIcon, SwapIcon } from "./icons";

type Tab = "pair" | "trade" | "filters" | "alerts";

const TABS: { id: Tab; label: string }[] = [
  { id: "pair", label: "Pair" },
  { id: "trade", label: "Trade" },
  { id: "filters", label: "Filters" },
  { id: "alerts", label: "Alerts" },
];

const PUBLISHER_OPTIONS: { value: PublisherFilter; label: string }[] = [
  { value: "any", label: "Any" },
  { value: "merchant", label: "Pro" },
  { value: "user", label: "User" },
];

function sideKey(side: LegSide): "buyFilters" | "sellFilters" {
  return side === "buy" ? "buyFilters" : "sellFilters";
}

function favKey(side: LegSide): "favoriteBuyUsers" | "favoriteSellUsers" {
  return side === "buy" ? "favoriteBuyUsers" : "favoriteSellUsers";
}

function FilterBlock({
  side,
  fiat,
  filters,
  payMethods,
  onChange,
}: {
  side: LegSide;
  fiat: string;
  filters: SideFilters;
  payMethods: string[];
  onChange: (patch: Partial<SideFilters>) => void;
}) {
  const label = side === "buy" ? `Buy leg · ${fiat}` : `Sell leg · ${fiat}`;

  const togglePay = (method: string) => {
    const next = filters.payTypes.includes(method)
      ? filters.payTypes.filter((item) => item !== method)
      : [...filters.payTypes, method];
    onChange({ payTypes: next });
  };

  return (
    <div className="section">
      <h3 className="section-title">{label}</h3>

      <Field label="Publisher" hint="Merchant = Binance-verified pro merchant">
        <Segmented
          value={filters.publisher}
          options={PUBLISHER_OPTIONS}
          onChange={(publisher) => onChange({ publisher })}
        />
      </Field>

      <Field label="Min completion rate" hint="30-day order completion">
        <NumField
          value={Math.round(filters.minCompletionRate * 100)}
          onChange={(percent) => onChange({ minCompletionRate: Math.min(100, Math.max(0, percent)) / 100 })}
          step={1}
          min={0}
          max={100}
          pct
        />
      </Field>

      <Field label="Min positive feedback" hint="Leave 0 to ignore this filter">
        <NumField
          value={Math.round(filters.minPositiveRate * 100)}
          onChange={(percent) => onChange({ minPositiveRate: Math.min(100, Math.max(0, percent)) / 100 })}
          step={1}
          min={0}
          max={100}
          pct
        />
      </Field>

      <Field label="Min 30-day orders">
        <NumField
          value={filters.minMonthOrders}
          onChange={(minMonthOrders) => onChange({ minMonthOrders: Math.max(0, Math.round(minMonthOrders)) })}
          step={5}
          min={0}
        />
      </Field>

      <Field label="Online only" hint={`Active within ${Math.round(filters.onlineWithinSecs / 60)} min`}>
        <Toggle on={filters.onlineOnly} onChange={(onlineOnly) => onChange({ onlineOnly })} />
      </Field>

      {filters.onlineOnly ? (
        <Field label="Online window" hint="Minutes since the advertiser was last active">
          <NumField
            value={Math.round(filters.onlineWithinSecs / 60)}
            onChange={(minutes) =>
              onChange({ onlineWithinSecs: Math.min(1440, Math.max(1, Math.round(minutes))) * 60 })
            }
            step={5}
            min={1}
            max={1440}
            pct
          />
        </Field>
      ) : null}

      <div className="field" style={{ alignItems: "flex-start" }}>
        <div>
          <div className="field-label">Payment methods</div>
          <div className="field-hint">
            {filters.payTypes.length === 0
              ? "Any method accepted"
              : `${filters.payTypes.length} selected`}
          </div>
        </div>
      </div>
      <div className="chips">
        {payMethods.length === 0 ? (
          <span className="field-hint">Loading methods…</span>
        ) : (
          payMethods.map((method) => (
            <button
              key={method}
              type="button"
              className={`chip${filters.payTypes.includes(method) ? " on" : ""}`}
              onClick={() => togglePay(method)}
            >
              {prettyPayType(method)}
            </button>
          ))
        )}
        {filters.payTypes.length > 0 ? (
          <button type="button" className="chip add" onClick={() => onChange({ payTypes: [] })}>
            clear
          </button>
        ) : null}
      </div>
    </div>
  );
}

export function SettingsPanel({
  settings,
  fiats,
  onSave,
  onClose,
  onTest,
  saving,
  error,
  initialTab,
}: {
  settings: Settings;
  fiats: FiatInfo[];
  onSave: (next: Settings) => void;
  onClose: () => void;
  onTest: (sound: string) => void;
  saving: boolean;
  error: string | null;
  initialTab?: string;
}) {
  const startTab: Tab = TABS.some((item) => item.id === initialTab)
    ? (initialTab as Tab)
    : "pair";
  const [tab, setTab] = useState<Tab>(startTab);
  const [draft, setDraft] = useState<Settings>(settings);
  const [picking, setPicking] = useState<LegSide | null>(null);
  const [payMethods, setPayMethods] = useState<{ buy: string[]; sell: string[] }>({
    buy: [],
    sell: [],
  });

  useEffect(() => {
    setDraft(settings);
  }, [settings]);

  // Discover real payment methods for the selected currencies.
  useEffect(() => {
    if (tab !== "filters") return;
    let cancelled = false;
    void (async () => {
      const [buy, sell] = await Promise.all([
        api.discoverPayMethods(draft.fiatA, "buy").catch(() => []),
        api.discoverPayMethods(draft.fiatB, "sell").catch(() => []),
      ]);
      if (!cancelled) setPayMethods({ buy, sell });
    })();
    return () => {
      cancelled = true;
    };
  }, [tab, draft.fiatA, draft.fiatB]);

  const patch = (next: Partial<Settings>) => setDraft((prev) => ({ ...prev, ...next }));

  const patchFilters = (side: LegSide, next: Partial<SideFilters>) =>
    setDraft((prev) => ({
      ...prev,
      [sideKey(side)]: { ...prev[sideKey(side)], ...next },
    }));

  const fiatName = (code: string) => fiats.find((fiat) => fiat.code === code)?.name ?? "";

  const dirty = useMemo(() => JSON.stringify(draft) !== JSON.stringify(settings), [draft, settings]);

  const clearFavorites = (side: LegSide) =>
    setDraft((prev) => ({ ...prev, [favKey(side)]: [] }));

  return (
    <div className="settings">
      <div className="header">
        <button type="button" className="icon-btn" title="Back" onClick={onClose}>
          <BackIcon />
        </button>
        <span className="brand" style={{ marginLeft: 2 }}>
          Settings
        </span>
        <span className="header-spacer" />
        <div className="tabs">
          {TABS.map((item) => (
            <button
              key={item.id}
              type="button"
              className={`tab${tab === item.id ? " active" : ""}`}
              onClick={() => setTab(item.id)}
            >
              {item.label}
            </button>
          ))}
        </div>
      </div>

      <div className="settings-scroll">
        {tab === "pair" ? (
          <>
            <Section title="Currency pair">
              <div className="pair-fields">
                <button type="button" className="currency-btn" onClick={() => setPicking("buy")}>
                  <span className="code">{draft.fiatA}</span>
                  <span className="name">{fiatName(draft.fiatA) || "Currency A"}</span>
                </button>
                <button
                  type="button"
                  className="swap-btn"
                  title="Swap currencies"
                  onClick={() => patch({ fiatA: draft.fiatB, fiatB: draft.fiatA })}
                >
                  <SwapIcon />
                </button>
                <button type="button" className="currency-btn" onClick={() => setPicking("sell")}>
                  <span className="code">{draft.fiatB}</span>
                  <span className="name">{fiatName(draft.fiatB) || "Currency B"}</span>
                </button>
              </div>
              <div className="field-hint" style={{ display: "flex", alignItems: "center", gap: 5 }}>
                Buy USDT with {draft.fiatA}
                <ArrowRightIcon size={10} />
                sell USDT for {draft.fiatB}
              </div>
            </Section>

            <Section title="Alert threshold">
              <Field
                label="Threshold R"
                hint={`Alert when 1 ${draft.fiatA} buys at least this many ${draft.fiatB}`}
              >
                <NumField
                  value={draft.threshold}
                  onChange={(threshold) => patch({ threshold })}
                  step={0.001}
                />
              </Field>
              <Field
                label="Mid-market override"
                hint="0 = auto-fetch the reference rate"
              >
                <NumField
                  value={draft.midOverride ?? 0}
                  onChange={(value) => patch({ midOverride: value > 0 ? value : null })}
                  step={0.0001}
                />
              </Field>
              <Field
                label="Outlier warning"
                hint="Flag offers this far from the book median"
              >
                <NumField
                  value={draft.outlierPct}
                  onChange={(outlierPct) => patch({ outlierPct })}
                  step={0.5}
                  min={0}
                  max={50}
                  pct
                />
              </Field>
            </Section>

            <Section title="Refresh">
              <Field label="Poll interval" hint="Seconds between Binance P2P checks">
                <NumField
                  value={draft.refreshSecs}
                  onChange={(refreshSecs) =>
                    patch({ refreshSecs: Math.min(600, Math.max(5, Math.round(refreshSecs))) })
                  }
                  step={5}
                  min={5}
                  max={600}
                  pct
                />
              </Field>
            </Section>
          </>
        ) : null}

        {tab === "trade" ? (
          <>
            <Section title={`Buy leg · amounts in ${draft.fiatA}`}>
              <Field label="Minimum trade">
                <NumField
                  value={draft.buyMinAmount}
                  onChange={(buyMinAmount) => patch({ buyMinAmount })}
                  step={100}
                />
              </Field>
              <Field label="Maximum trade" hint="0 = no upper limit">
                <NumField
                  value={draft.buyMaxAmount}
                  onChange={(buyMaxAmount) => patch({ buyMaxAmount })}
                  step={100}
                />
              </Field>
            </Section>

            <Section title={`Sell leg · amounts in ${draft.fiatB}`}>
              <Field label="Minimum trade">
                <NumField
                  value={draft.sellMinAmount}
                  onChange={(sellMinAmount) => patch({ sellMinAmount })}
                  step={100}
                />
              </Field>
              <Field label="Maximum trade" hint="0 = no upper limit">
                <NumField
                  value={draft.sellMaxAmount}
                  onChange={(sellMaxAmount) => patch({ sellMaxAmount })}
                  step={100}
                />
              </Field>
            </Section>

            <div className="banner warn" style={{ margin: 0 }}>
              <span className="banner-icon">!</span>
              <span>
                The app intersects both bands in USDT. An alert only fires when a size
                exists that satisfies the buy leg and the sell leg at once.
              </span>
            </div>

            <Section title="Favorite users">
              <Field label={`Buy-side favorites · ${draft.fiatA}`} hint="Star an offer in the popup to add it">
                <button type="button" className="btn ghost small" onClick={() => clearFavorites("buy")}>
                  {draft.favoriteBuyUsers.length} · clear
                </button>
              </Field>
              <Field label={`Sell-side favorites · ${draft.fiatB}`} hint="Highlighted, not filter-bypassing">
                <button type="button" className="btn ghost small" onClick={() => clearFavorites("sell")}>
                  {draft.favoriteSellUsers.length} · clear
                </button>
              </Field>
            </Section>
          </>
        ) : null}

        {tab === "filters" ? (
          <>
            <FilterBlock
              side="buy"
              fiat={draft.fiatA}
              filters={draft.buyFilters}
              payMethods={payMethods.buy}
              onChange={(next) => patchFilters("buy", next)}
            />
            <FilterBlock
              side="sell"
              fiat={draft.fiatB}
              filters={draft.sellFilters}
              payMethods={payMethods.sell}
              onChange={(next) => patchFilters("sell", next)}
            />
          </>
        ) : null}

        {tab === "alerts" ? (
          <>
            <Section title="Alert type">
              <Field label="System notification">
                <Toggle
                  on={draft.alertNotification}
                  onChange={(alertNotification) => patch({ alertNotification })}
                />
              </Field>
              <Field label="Sound">
                <Toggle on={draft.alertSound} onChange={(alertSound) => patch({ alertSound })} />
              </Field>
              <Field label="Menu bar badge" hint="Green icon + live rate in the bar">
                <Toggle on={draft.alertBadge} onChange={(alertBadge) => patch({ alertBadge })} />
              </Field>
            </Section>

            <Section title="Alert trade size">
              <Field
                label={`Amount in ${draft.fiatA}`}
                hint="Used in the notification, e.g. “10,000 AED buys …”"
              >
                <NumField
                  value={draft.notifyAmount}
                  onChange={(notifyAmount) => patch({ notifyAmount })}
                  step={1000}
                  min={1}
                />
              </Field>
            </Section>

            <Section title="Sound">
              <Field label="Alert sound">
                <select
                  className="input wide"
                  value={draft.soundName}
                  onChange={(event) => patch({ soundName: event.target.value })}
                  style={{ textAlign: "left" }}
                >
                  {SOUNDS.map((sound) => (
                    <option key={sound} value={sound}>
                      {sound}
                    </option>
                  ))}
                </select>
              </Field>
              <Field label="Preview" hint="Plays the selected sound">
                <button
                  type="button"
                  className="btn ghost small"
                  onClick={() => onTest(draft.soundName)}
                >
                  Test alert
                </button>
              </Field>
            </Section>

            <Section title="Behaviour">
              <Field label="Launch at login" hint="Start monitoring automatically">
                <Toggle
                  on={draft.launchAtLogin}
                  onChange={(launchAtLogin) => patch({ launchAtLogin })}
                />
              </Field>
              <div className="field-hint" style={{ paddingTop: 6 }}>
                Alerts fire once when the cross rate crosses above R, then stay quiet
                until it drops below R again.
              </div>
            </Section>
          </>
        ) : null}

        {error ? <div className="error-text">{error}</div> : null}
      </div>

      <div className="settings-footer">
        <button type="button" className="btn ghost" onClick={onClose}>
          Cancel
        </button>
        <span className="spacer" />
        <button
          type="button"
          className="btn primary"
          disabled={saving || !dirty}
          onClick={() => onSave(draft)}
        >
          {saving ? "Saving…" : dirty ? "Save changes" : "Saved"}
        </button>
      </div>

      {picking ? (
        <FiatPicker
          title={picking === "buy" ? "Currency A" : "Currency B"}
          fiats={fiats}
          selected={picking === "buy" ? draft.fiatA : draft.fiatB}
          exclude={picking === "buy" ? draft.fiatB : draft.fiatA}
          onPick={(code) => {
            patch(picking === "buy" ? { fiatA: code } : { fiatB: code });
            setPicking(null);
          }}
          onClose={() => setPicking(null)}
        />
      ) : null}
    </div>
  );
}
