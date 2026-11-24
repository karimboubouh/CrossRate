import { useState } from "react";
import type { LegView, Offer, Settings, Snapshot } from "../types";
import { api } from "../lib/api";
import {
  fmtAge,
  fmtAmount,
  fmtCount,
  fmtGrouped,
  fmtPct,
  fmtRate,
  fmtRatePct,
  prettyPayType,
} from "../lib/format";
import { GearIcon, Logo, RefreshIcon, StarIcon, WarnIcon } from "./icons";

/* ------------------------------------------------------------------ gauge -- */

function RateGauge({ j, threshold, mid }: { j: number | null; threshold: number; mid: number | null }) {
  const points = [j, threshold, mid].filter((value): value is number => value != null && isFinite(value) && value > 0);
  const low = Math.min(...points);
  const high = Math.max(...points);
  const span = high - low || Math.max(high * 0.02, 1e-6);
  const floor = low - span * 0.35;
  const ceiling = high + span * 0.35;
  const pos = (value: number) => ((value - floor) / (ceiling - floor)) * 100;

  return (
    <div className="gauge">
      <div className="gauge-track">
        {j != null ? <div className="gauge-fill" style={{ width: `${Math.max(1.5, Math.min(100, pos(j)))}%` }} /> : null}
        {mid != null ? (
          <div
            className="gauge-mark mid"
            style={{ left: `${pos(mid)}%` }}
            title={`Bank / mid-market ${fmtRate(mid)}`}
          />
        ) : null}
        <div
          className="gauge-mark threshold"
          style={{ left: `${pos(threshold)}%` }}
          title={`Your target ${fmtRate(threshold)}`}
        />
      </div>
      <div className="gauge-legend">
        <span className="item">
          <span className="swatch" style={{ background: "var(--label-3)" }} />
          bank {fmtRate(mid)}
        </span>
        <span className="item">
          <span className="swatch" style={{ background: "var(--label)" }} />
          target {fmtRate(threshold)}
        </span>
      </div>
    </div>
  );
}

/* -------------------------------------------------------------- leg card -- */

function LegCard({ leg }: { leg: LegView }) {
  const isBuy = leg.side === "buy";
  const best = leg.best;
  const noOffers = leg.passed === 0 && leg.offers.length === 0;

  return (
    <div className="leg">
      <div className="leg-head">
        <span className={`leg-tag ${isBuy ? "buy" : "sell"}`}>
          {isBuy ? `BUY ${leg.fiat}` : `SELL ${leg.fiat}`}
        </span>
        <span className="leg-count">{leg.passed > 0 ? `${leg.passed} offers` : "none"}</span>
      </div>
      <div className="leg-rate num">
        {best ? fmtRate(best.price) : "–"}
        <small>{isBuy ? `${leg.fiat}/USDT` : `${leg.fiat}/USDT`}</small>
      </div>
      {best ? (
        <>
          <div className="leg-user">
            {best.isFavorite ? <span className="star">★</span> : null}
            <span className="name">{best.nickname}</span>
          </div>
          <div className="leg-meta num">
            {fmtRatePct(best.completionRate, 0)} · {fmtCount(best.monthOrders)} orders
          </div>
        </>
      ) : (
        <div className="leg-meta" style={{ whiteSpace: "normal" }}>
          {noOffers ? "No offer passes your filters" : "No offers found"}
        </div>
      )}
    </div>
  );
}

/* ------------------------------------------------------------ offer rows -- */

function OfferRow({
  offer,
  rank,
  isBest,
  onToggleFavorite,
}: {
  offer: Offer;
  rank: number;
  isBest: boolean;
  onToggleFavorite: () => void;
}) {
  const classes = [
    "offer",
    isBest ? "best" : "",
    offer.isFavorite ? "fav" : "",
  ]
    .filter(Boolean)
    .join(" ");

  const payPreview = offer.payTypes.slice(0, 2).map(prettyPayType).join(", ");
  const morePays = Math.max(0, offer.payTypes.length - 2);

  return (
    <div
      className={classes}
      role="button"
      tabIndex={0}
      title={`Open ${offer.nickname} on Binance P2P`}
      onClick={() => void api.openAdvertiser(offer.userNo)}
      onKeyDown={(event) => {
        if (event.key === "Enter" || event.key === " ") void api.openAdvertiser(offer.userNo);
      }}
    >
      <span className="offer-rank num">{rank}</span>
      <span className="offer-main">
        <span className="offer-name-row">
          <span className="offer-name">{offer.nickname}</span>
          {offer.userType === "merchant" ? (
            <span className="badge merchant">
              {offer.userIdentity === "MASS_MERCHANT" ? "PRO" : "MERCHANT"}
            </span>
          ) : null}
          {offer.suspicious ? (
            <span
              className="badge risk"
              title="This price is far from the rest of the book — likely bait or stale"
            >
              <WarnIcon size={8} />
            </span>
          ) : null}
        </span>
        <span className="offer-sub num">
          <span>
            {fmtAmount(offer.minAmount)}–{fmtAmount(offer.maxAmount)}
          </span>
          <span className="sep">·</span>
          <span>
            {payPreview}
            {morePays > 0 ? ` +${morePays}` : ""}
          </span>
        </span>
      </span>
      <span className="offer-right">
        <span className="offer-price num">{fmtRate(offer.price)}</span>
        <span className="offer-meta num">
          {fmtRatePct(offer.completionRate, 0)} · {fmtAge(offer.activeSecs)} ago
        </span>
      </span>
      <button
        type="button"
        className={`fav-btn${offer.isFavorite ? " on" : ""}`}
        title={offer.isFavorite ? "Remove from favorites" : "Add to favorites"}
        onClick={(event) => {
          event.stopPropagation();
          onToggleFavorite();
        }}
      >
        <StarIcon size={10} />
      </button>
    </div>
  );
}

function OfferList({
  leg,
  onToggleFavorite,
}: {
  leg: LegView;
  onToggleFavorite: (userNo: string) => void;
}) {
  if (leg.error) {
    return <div className="empty-block">{leg.error}</div>;
  }
  if (leg.offers.length === 0) {
    return (
      <div className="empty-block">
        {leg.fetched > 0
          ? `All ${leg.fetched} offers were filtered out. Try loosening completion rate, order count, or the amount band.`
          : `No ${leg.fiat} advertisements right now.`}
      </div>
    );
  }
  return (
    <>
      {leg.offers.slice(0, 8).map((offer, index) => (
        <OfferRow
          key={offer.advNo}
          offer={offer}
          rank={index + 1}
          isBest={offer.advNo === leg.best?.advNo}
          onToggleFavorite={() => onToggleFavorite(offer.userNo)}
        />
      ))}
    </>
  );
}

/* -------------------------------------------------------------- dashboard -- */

export function Dashboard({
  snapshot,
  settings,
  onOpenSettings,
  onRefresh,
  refreshing,
  onToggleFavorite,
}: {
  snapshot: Snapshot | null;
  settings: Settings;
  onOpenSettings: () => void;
  onRefresh: () => void;
  refreshing: boolean;
  onToggleFavorite: (side: "buy" | "sell", userNo: string) => void;
}) {
  const [tab, setTab] = useState<"buy" | "sell">("buy");

  const stale = snapshot?.stale ?? false;
  const alert = (snapshot?.alertActive ?? false) && !stale;
  const rateMet = snapshot?.rateMet ?? false;
  const noSize = rateMet && snapshot?.feasible && !snapshot.feasible.ok;

  const statusClass = stale
    ? "status warn"
    : alert
      ? "status active"
      : noSize
        ? "status warn"
        : "status";
  const statusText = stale
    ? "Stale data"
    : alert
      ? "Alert active"
      : noSize
        ? "Rate met · no size"
        : snapshot?.j == null
          ? "No data"
          : "Below threshold";

  // Quote the configured size, but never more than is actually tradable now.
  const wanted = settings.notifyAmount;
  const refSize =
    snapshot?.feasible.ok === true
      ? Math.min(Math.max(wanted, snapshot.feasible.aMin), snapshot.feasible.aMax)
      : wanted;
  const sizeQuote =
    snapshot?.feasible.ok !== true || Math.abs(refSize - wanted) <= 0.5
      ? "exact"
      : refSize > wanted
        ? "capped"
        : "floored";
  const gain =
    snapshot && snapshot.j != null && snapshot.mid != null
      ? refSize * (snapshot.j - snapshot.mid)
      : null;

  const buyLeg = snapshot?.buy;
  const sellLeg = snapshot?.sell;

  return (
    <>
      <div className="header">
        <span className="brand">
          <Logo size={15} />
          CrossRate
        </span>
        <span className="header-spacer" />
        <button type="button" className="icon-btn" title="Refresh now" onClick={onRefresh}>
          <RefreshIcon />
        </button>
        <button
          type="button"
          className="icon-btn"
          title="Settings"
          onClick={onOpenSettings}
        >
          <GearIcon />
        </button>
      </div>

      <div className="hero">
        <div className="hero-top">
          <div>
            <div className="hero-label">
              {settings.fiatA} → {settings.fiatB} · P2P rate
            </div>
            <div className="hero-value">
              <span className={`hero-j num${snapshot?.j == null ? " none" : ""}`}>
                {fmtRate(snapshot?.j ?? null)}
              </span>
              <span className="hero-unit">{settings.fiatB}/{settings.fiatA}</span>
            </div>
          </div>
          <span className={statusClass}>
            <span className="dot" />
            {statusText}
          </span>
        </div>

        <RateGauge
          j={snapshot?.j ?? null}
          threshold={snapshot?.threshold ?? settings.threshold}
          mid={snapshot?.mid ?? null}
        />

        <div className="hero-facts">
          <div className="fact">
            <div className="fact-k">vs bank</div>
            <div className={`fact-v num ${(snapshot?.spreadPct ?? 0) >= 0 ? "green" : "amber"}`}>
              {fmtPct(snapshot?.spreadPct)}
            </div>
          </div>
          <div className="fact">
            <div className="fact-k">Your target</div>
            <div className="fact-v num">{fmtRate(snapshot?.threshold ?? settings.threshold)}</div>
          </div>
          <div className="fact">
            <div className="fact-k">Tradable size</div>
            <div className={`fact-v num ${snapshot?.feasible.ok ? "green" : ""}`}>
              {snapshot?.feasible.ok
                ? `${fmtAmount(snapshot.feasible.aMin)}–${fmtAmount(snapshot.feasible.aMax)} ${settings.fiatA}`
                : "–"}
            </div>
          </div>
        </div>
      </div>

      {snapshot?.stale ? (
        <div className="banner warn">
          <span className="banner-icon">!</span>
          <span>
            <b>Showing the last good reading</b>
            {snapshot.error ? ` — ${snapshot.error}` : ""}. Prices below may be out
            of date.
          </span>
        </div>
      ) : snapshot?.error ? (
        <div className="banner err">
          <span className="banner-icon">!</span>
          <span>{snapshot.error}</span>
        </div>
      ) : null}

      {snapshot?.fragile ? (
        <div className="banner warn">
          <span className="banner-icon">!</span>
          <span>
            <b>Fragile alert</b> — only one offer pair is this good. If either ad
            disappears, it closes.
          </span>
        </div>
      ) : null}

      {noSize && snapshot?.feasible.reason ? (
        <div className="banner warn">
          <span className="banner-icon">!</span>
          <span>
            <b>Rate is above your target but not tradable.</b> {snapshot.feasible.reason}
          </span>
        </div>
      ) : null}

      {gain != null && refSize != null ? (
        <div className="banner green">
          <span className="banner-icon">↗</span>
          <span>
            <b>
              {sizeQuote === "capped" ? "Up to " : sizeQuote === "floored" ? "Minimum " : ""}
              {fmtGrouped(refSize)} {settings.fiatA}
            </b>{" "}
            buys{" "}
            <b>
              {fmtGrouped(refSize * (snapshot!.j as number))} {settings.fiatB}
            </b>{" "}
            at the P2P rate — <b>{fmtGrouped(gain)} {settings.fiatB}</b> more than the bank.
          </span>
        </div>
      ) : null}

      {buyLeg && sellLeg ? (
        <div className="legs">
          <LegCard leg={buyLeg} />
          <LegCard leg={sellLeg} />
        </div>
      ) : null}

      <div className="offers">
        <div className="offers-head">
          <div className="tabs">
            <button
              type="button"
              className={`tab${tab === "buy" ? " active" : ""}`}
              onClick={() => setTab("buy")}
            >
              Buy {settings.fiatA}
            </button>
            <button
              type="button"
              className={`tab${tab === "sell" ? " active" : ""}`}
              onClick={() => setTab("sell")}
            >
              Sell {settings.fiatB}
            </button>
          </div>
          <span className="offers-note">
            {tab === "buy"
              ? `${buyLeg?.passed ?? 0}/${buyLeg?.total ?? 0} ads`
              : `${sellLeg?.passed ?? 0}/${sellLeg?.total ?? 0} ads`}
          </span>
        </div>
        <div className="offer-scroll">
          {tab === "buy" && buyLeg ? (
            <OfferList leg={buyLeg} onToggleFavorite={(userNo) => onToggleFavorite("buy", userNo)} />
          ) : null}
          {tab === "sell" && sellLeg ? (
            <OfferList leg={sellLeg} onToggleFavorite={(userNo) => onToggleFavorite("sell", userNo)} />
          ) : null}
          <div className="list-hint">
            Prices refresh every {snapshot?.refreshSecs ?? settings.refreshSecs}s · tap an offer to
            open it on Binance
          </div>
        </div>
      </div>

      <div className="footer">
        <span
          className={`live-dot${snapshot?.error ? " err" : snapshot?.j != null ? "" : " off"}`}
          title={snapshot?.error ? "Polling error" : "Live"}
        />
        <span>
          {snapshot ? `updated ${fmtAge((Date.now() - snapshot.ts) / 1000)} ago` : "starting…"}
        </span>
        <span className="spacer" />
        <span>{refreshing ? "refreshing…" : `every ${snapshot?.refreshSecs ?? settings.refreshSecs}s`}</span>
      </div>
    </>
  );
}
