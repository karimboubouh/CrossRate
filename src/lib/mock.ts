// Realistic fixtures for the browser-only preview (?scenario=alert|idle|empty|settings).

import type { Offer, Settings, Snapshot, LegView, FiatInfo } from "../types";

export type Scenario = "alert" | "idle" | "empty" | "settings";

function offer(partial: Partial<Offer> & { nickname: string; price: number }): Offer {
  return {
    advNo: `adv-${partial.nickname}`,
    userNo: `s${partial.nickname.toLowerCase().replace(/[^a-z0-9]/g, "")}`,
    userType: "merchant",
    userIdentity: "MASS_MERCHANT",
    completionRate: 0.99,
    positiveRate: 0.98,
    monthOrders: 640,
    activeSecs: 40,
    minAmount: 1000,
    maxAmount: 50000,
    availableUsdt: 24500,
    payTypes: ["BANK"],
    payTimeLimit: 15,
    isFavorite: false,
    suspicious: false,
    ...partial,
  };
}

const buyOffers: Offer[] = [
  offer({
    nickname: "Trusty-Change",
    price: 3.69,
    completionRate: 1,
    monthOrders: 180,
    activeSecs: 12,
    minAmount: 10000,
    maxAmount: 24000,
    payTypes: ["cihmorocco", "BANK"],
    isFavorite: true,
  }),
  offer({
    nickname: "Lid_Oua",
    price: 3.692,
    userType: "user",
    userIdentity: "",
    completionRate: 0.989,
    monthOrders: 176,
    activeSecs: 90,
    minAmount: 10000,
    maxAmount: 12000,
    payTypes: ["BANK"],
  }),
  offer({
    nickname: "VIP_Trusted_P2P",
    price: 3.697,
    userType: "user",
    userIdentity: "",
    completionRate: 0.972,
    monthOrders: 34,
    activeSecs: 240,
    minAmount: 15600,
    maxAmount: 24000,
    payTypes: ["ADCB", "Aani"],
  }),
  offer({
    nickname: "hishamruk",
    price: 3.702,
    userType: "user",
    userIdentity: "",
    completionRate: 1,
    monthOrders: 11,
    activeSecs: 837,
    minAmount: 5815,
    maxAmount: 5825,
    payTypes: ["ADCB"],
    suspicious: true,
  }),
  offer({
    nickname: "GulfExchange",
    price: 3.712,
    completionRate: 0.995,
    monthOrders: 1420,
    activeSecs: 20,
    minAmount: 2000,
    maxAmount: 60000,
    payTypes: ["EmiratesNBD", "BANK"],
  }),
];

const sellOffers: Offer[] = [
  offer({
    nickname: "MBebars",
    price: 10.87,
    completionRate: 0.961,
    positiveRate: 0.985,
    monthOrders: 1007,
    activeSecs: 78,
    minAmount: 11000,
    maxAmount: 100000,
    payTypes: ["cihmorocco"],
    isFavorite: true,
  }),
  offer({
    nickname: "AtlasCapital",
    price: 10.85,
    completionRate: 0.994,
    monthOrders: 2300,
    activeSecs: 15,
    minAmount: 5000,
    maxAmount: 80000,
    payTypes: ["AttijariwafaNational", "BANK"],
  }),
  offer({
    nickname: "Kingkong_linglong",
    price: 10.84,
    userType: "user",
    userIdentity: "",
    completionRate: 0.791,
    monthOrders: 49,
    activeSecs: 1200,
    minAmount: 5000,
    maxAmount: 24000,
    payTypes: ["CashPlus"],
    suspicious: true,
  }),
  offer({
    nickname: "BilalExchangeUSDT",
    price: 10.83,
    completionRate: 0.917,
    monthOrders: 66,
    activeSecs: 400,
    minAmount: 5000,
    maxAmount: 100000,
    payTypes: ["simple", "AttijariwafaNational"],
  }),
  offer({
    nickname: "CasaP2P",
    price: 10.81,
    userType: "user",
    userIdentity: "",
    completionRate: 0.968,
    monthOrders: 310,
    activeSecs: 210,
    minAmount: 3000,
    maxAmount: 45000,
    payTypes: ["CreditBanquePopu"],
  }),
];

function leg(
  side: "buy" | "sell",
  fiat: string,
  offers: Offer[],
  total: number,
): LegView {
  return {
    side,
    fiat,
    best: offers[0] ?? null,
    offers,
    fetched: 20,
    passed: offers.length,
    total,
    error: null,
    payMethods: ["cihmorocco", "BANK", "AttijariwafaNational", "CashPlus", "simple"],
  };
}

export function mockSettings(): Settings {
  return {
    fiatA: "AED",
    fiatB: "MAD",
    threshold: 2.75,
    refreshSecs: 15,
    buyMinAmount: 1000,
    buyMaxAmount: 50000,
    sellMinAmount: 5000,
    sellMaxAmount: 100000,
    buyFilters: {
      publisher: "any",
      minCompletionRate: 0.9,
      minPositiveRate: 0,
      minMonthOrders: 20,
      onlineOnly: true,
      onlineWithinSecs: 900,
      payTypes: [],
    },
    sellFilters: {
      publisher: "any",
      minCompletionRate: 0.9,
      minPositiveRate: 0,
      minMonthOrders: 20,
      onlineOnly: true,
      onlineWithinSecs: 900,
      payTypes: [],
    },
    favoriteBuyUsers: ["sTrustyChange"],
    favoriteSellUsers: ["sMBebars"],
    alertNotification: true,
    alertSound: true,
    alertBadge: true,
    soundName: "Glass",
    notifyAmount: 10000,
    midOverride: null,
    outlierPct: 3,
    launchAtLogin: true,
  };
}

export function mockSnapshot(scenario: Scenario = "alert", jitter = 0): Snapshot {
  const settings = mockSettings();
  const empty = scenario === "empty";
  const idle = scenario === "idle";

  const buy = empty
    ? { ...leg("buy", "AED", [], 0), passed: 0, fetched: 0 }
    : leg("buy", "AED", buyOffers, 205);
  const sell = empty
    ? { ...leg("sell", "MAD", [], 0), passed: 0, fetched: 0 }
    : leg("sell", "MAD", sellOffers, 340);

  const j = empty ? null : idle ? 2.7124 + jitter : 2.9458 + jitter;
  const mid = 2.726018;
  const threshold = settings.threshold;
  const rateMet = j != null && j >= threshold;
  const alertActive = !empty && !idle && rateMet;
  const spreadPct = j == null ? null : (j / mid - 1) * 100;

  return {
    ts: Date.now(),
    rev: 7,
    fiatA: settings.fiatA,
    fiatB: settings.fiatB,
    buy,
    sell,
    j,
    jSecond: empty ? null : idle ? 2.6 : 2.7412,
    fragile: false,
    threshold,
    mid,
    midSource: "ecb-er-api",
    midAgeSecs: 412,
    spreadPct,
    alertActive,
    alertSince: alertActive ? Date.now() - 42_000 : null,
    alertCount: alertActive ? 3 : 0,
    feasible: empty
      ? { ok: false, reason: "Waiting for matching offers on both legs", usdtMin: 0, usdtMax: 0, aMin: 0, aMax: 0, bMin: 0, bMax: 0 }
      : {
          ok: true,
          reason: "",
          usdtMin: 2710,
          usdtMax: 8650,
          aMin: 10000,
          aMax: 31919,
          bMin: 29458,
          bMax: 94026,
        },
    rateMet,
    stale: false,
    error: null,
    nextPollMs: Date.now() + settings.refreshSecs * 1000,
    refreshSecs: settings.refreshSecs,
    alertBadge: settings.alertBadge,
    alertSound: settings.alertSound,
    alertNotification: settings.alertNotification,
    soundName: settings.soundName,
  };
}

export const mockFiats: FiatInfo[] = [
  { code: "AED", name: "UAE Dirham" },
  { code: "MAD", name: "Moroccan Dirham" },
  { code: "USD", name: "US Dollar" },
  { code: "EUR", name: "Euro" },
  { code: "GBP", name: "British Pound" },
  { code: "SAR", name: "Saudi Riyal" },
  { code: "QAR", name: "Qatari Riyal" },
  { code: "EGP", name: "Egyptian Pound" },
  { code: "INR", name: "Indian Rupee" },
  { code: "PKR", name: "Pakistani Rupee" },
  { code: "BDT", name: "Bangladeshi Taka" },
  { code: "NGN", name: "Nigerian Naira" },
  { code: "TRY", name: "Turkish Lira" },
  { code: "TND", name: "Tunisian Dinar" },
  { code: "DZD", name: "Algerian Dinar" },
  { code: "KWD", name: "Kuwaiti Dinar" },
  { code: "BHD", name: "Bahraini Dinar" },
  { code: "OMR", name: "Omani Rial" },
  { code: "JOD", name: "Jordanian Dinar" },
  { code: "CHF", name: "Swiss Franc" },
  { code: "CAD", name: "Canadian Dollar" },
  { code: "AUD", name: "Australian Dollar" },
  { code: "JPY", name: "Japanese Yen" },
  { code: "CNY", name: "Chinese Yuan" },
  { code: "THB", name: "Thai Baht" },
  { code: "IDR", name: "Indonesian Rupiah" },
  { code: "MYR", name: "Malaysian Ringgit" },
  { code: "PHP", name: "Philippine Peso" },
  { code: "VND", name: "Vietnamese Dong" },
  { code: "ZAR", name: "South African Rand" },
  { code: "KES", name: "Kenyan Shilling" },
  { code: "RUB", name: "Russian Ruble" },
  { code: "UAH", name: "Ukrainian Hryvnia" },
  { code: "BRL", name: "Brazilian Real" },
  { code: "MXN", name: "Mexican Peso" },
  { code: "ARS", name: "Argentine Peso" },
];
