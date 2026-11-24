import type { ReactNode } from "react";

export function Toggle({ on, onChange }: { on: boolean; onChange: (next: boolean) => void }) {
  return (
    <button
      type="button"
      className={`toggle${on ? " on" : ""}`}
      role="switch"
      aria-checked={on}
      onClick={() => onChange(!on)}
    />
  );
}

export function Field({
  label,
  hint,
  children,
}: {
  label: string;
  hint?: string;
  children: ReactNode;
}) {
  return (
    <div className="field">
      <div>
        <div className="field-label">{label}</div>
        {hint ? <div className="field-hint">{hint}</div> : null}
      </div>
      <div className="field-body">{children}</div>
    </div>
  );
}

export function Section({ title, children }: { title: string; children: ReactNode }) {
  return (
    <section className="section">
      <h3 className="section-title">{title}</h3>
      {children}
    </section>
  );
}

export function Segmented<T extends string>({
  value,
  options,
  onChange,
}: {
  value: T;
  options: { value: T; label: string }[];
  onChange: (next: T) => void;
}) {
  return (
    <div className="segmented">
      {options.map((option) => (
        <button
          key={option.value}
          type="button"
          className={option.value === value ? "active" : ""}
          onClick={() => onChange(option.value)}
        >
          {option.label}
        </button>
      ))}
    </div>
  );
}

/**
 * Numeric input that keeps a local string so typing "1." or clearing the field
 * does not fight the parent state.
 */
export function NumField({
  value,
  onChange,
  suffix,
  step,
  min,
  max,
  wide,
  pct,
}: {
  value: number;
  onChange: (next: number) => void;
  suffix?: string;
  step?: number;
  min?: number;
  max?: number;
  wide?: boolean;
  pct?: boolean;
}) {
  const display = Number.isFinite(value) ? String(value) : "";
  return (
    <input
      className={`input${wide ? " wide" : ""}${pct ? " pct" : ""}`}
      type="number"
      inputMode="decimal"
      value={display}
      step={step ?? "any"}
      min={min}
      max={max}
      placeholder={suffix}
      onChange={(event) => {
        const raw = event.target.value;
        if (raw === "") {
          onChange(0);
          return;
        }
        const parsed = Number(raw);
        if (Number.isFinite(parsed)) onChange(parsed);
      }}
    />
  );
}
