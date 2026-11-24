// Number formatting shared by the dashboard and settings.

/** Rates span 0.27 (USD per AED) to 30,000 (IDR per AED), so scale the decimals. */
export function fmtRate(value: number | null | undefined): string {
  if (value == null || !isFinite(value) || value <= 0) return "–";
  if (value >= 10000) return value.toFixed(0);
  if (value >= 1000) return value.toFixed(1);
  if (value >= 100) return value.toFixed(2);
  if (value >= 10) return value.toFixed(3);
  return value.toFixed(4);
}

export function fmtAmount(value: number | null | undefined): string {
  if (value == null || !isFinite(value)) return "–";
  const abs = Math.abs(value);
  if (abs >= 1_000_000) return `${(value / 1_000_000).toFixed(2)}M`;
  if (abs >= 100_000) return `${(value / 1000).toFixed(0)}k`;
  if (abs >= 10_000) return `${(value / 1000).toFixed(1)}k`;
  if (abs >= 1000) return value.toLocaleString("en-US", { maximumFractionDigits: 0 });
  return value.toFixed(0);
}

export function fmtMoney(value: number | null | undefined, fiat: string): string {
  if (value == null || !isFinite(value)) return "–";
  return `${fmtAmount(value)} ${fiat}`;
}

export function fmtPct(value: number | null | undefined, digits = 1): string {
  if (value == null || !isFinite(value)) return "–";
  const sign = value > 0 ? "+" : "";
  return `${sign}${value.toFixed(digits)}%`;
}

export function fmtRatePct(value: number | null | undefined, digits = 1): string {
  if (value == null || !isFinite(value)) return "–";
  return `${(value * 100).toFixed(digits)}%`;
}

export function fmtAge(secs: number | null | undefined): string {
  if (secs == null || !isFinite(secs)) return "–";
  if (secs < 60) return `${Math.max(0, Math.round(secs))}s`;
  if (secs < 3600) return `${Math.round(secs / 60)}m`;
  if (secs < 86400) return `${(secs / 3600).toFixed(1)}h`;
  return `${Math.round(secs / 86400)}d`;
}

export function fmtClock(ts: number | null | undefined): string {
  if (!ts) return "–";
  return new Date(ts).toLocaleTimeString([], {
    hour: "2-digit",
    minute: "2-digit",
    second: "2-digit",
  });
}

/** "1.2k", "48" — order counts. */
export function fmtCount(value: number | null | undefined): string {
  if (value == null || !isFinite(value)) return "–";
  if (value >= 10_000) return `${Math.round(value / 1000)}k`;
  if (value >= 1000) return `${(value / 1000).toFixed(1)}k`;
  return String(Math.round(value));
}

export function prettyPayType(raw: string): string {
  const map: Record<string, string> = {
    cihmorocco: "CIH Bank",
    AttijariwafaNational: "Attijariwafa",
    CreditBanquePopu: "Banque Populaire",
    BMCEBank: "BMCE Bank",
    CreditduMaroc: "Crédit du Maroc",
    aibaridbank: "Al Barid Bank",
    sahamBank: "Saham Bank",
    simple: "SIMPLE",
    CashPlus: "CashPlus",
    BANK: "Bank transfer",
    ADCB: "ADCB",
    ADIB: "ADIB",
    EmiratesNBD: "Emirates NBD",
    Aani: "Aani",
    CashDeposit: "Cash deposit",
    SpecificBank: "Specific bank",
    BankTransferMena: "Bank transfer",
  };
  return map[raw] ?? raw;
}
