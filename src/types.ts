// Mirrors the serde structures in src-tauri/src/model.rs (camelCase over IPC).

export type LegSide = "buy" | "sell";
export type PublisherFilter = "any" | "merchant" | "user";

export interface Offer {
  advNo: string;
  userNo: string;
  nickname: string;
  userType: string;
  userIdentity: string;
  completionRate: number;
  positiveRate: number;
  monthOrders: number;
  activeSecs: number;
  price: number;
  minAmount: number;
  maxAmount: number;
  availableUsdt: number;
  payTypes: string[];
  payTimeLimit: number;
  isFavorite: boolean;
  suspicious: boolean;
}

export interface LegView {
  side: LegSide;
  fiat: string;
  best: Offer | null;
  offers: Offer[];
  fetched: number;
  passed: number;
  total: number;
  error: string | null;
  payMethods: string[];
}

export interface Feasible {
  ok: boolean;
  reason: string;
  usdtMin: number;
  usdtMax: number;
  aMin: number;
  aMax: number;
  bMin: number;
  bMax: number;
}

export interface Snapshot {
  ts: number;
  rev: number;
  fiatA: string;
  fiatB: string;
  buy: LegView;
  sell: LegView;
  j: number | null;
  jSecond: number | null;
  fragile: boolean;
  threshold: number;
  mid: number | null;
  midSource: string;
  midAgeSecs: number;
  spreadPct: number | null;
  alertActive: boolean;
  alertSince: number | null;
  alertCount: number;
  feasible: Feasible;
  rateMet: boolean;
  stale: boolean;
  error: string | null;
  nextPollMs: number;
  refreshSecs: number;
  alertBadge: boolean;
  alertSound: boolean;
  alertNotification: boolean;
  soundName: string;
}

export interface SideFilters {
  publisher: PublisherFilter;
  minCompletionRate: number;
  minPositiveRate: number;
  minMonthOrders: number;
  onlineOnly: boolean;
  onlineWithinSecs: number;
  payTypes: string[];
}

export interface Settings {
  fiatA: string;
  fiatB: string;
  threshold: number;
  refreshSecs: number;
  buyMinAmount: number;
  buyMaxAmount: number;
  sellMinAmount: number;
  sellMaxAmount: number;
  buyFilters: SideFilters;
  sellFilters: SideFilters;
  favoriteBuyUsers: string[];
  favoriteSellUsers: string[];
  alertNotification: boolean;
  alertSound: boolean;
  alertBadge: boolean;
  soundName: string;
  notifyAmount: number;
  midOverride: number | null;
  outlierPct: number;
  launchAtLogin: boolean;
}

export interface FiatInfo {
  code: string;
  name: string;
}
