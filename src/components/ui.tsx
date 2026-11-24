import { useEffect, useState, type ReactNode } from "react";

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
 * Numeric input.
 *
 * The visible text is local state, so the user can clear the field and type
 * freely without the parent clamping a half-typed number (that used to make
 * "60" become "560"). A parsed value is reported on every keystroke, but the
 * display is only normalised from the model on blur / when not focused.
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
  invalid,
}: {
  value: number;
  onChange: (next: number) => void;
  suffix?: string;
  step?: number;
  min?: number;
  max?: number;
  wide?: boolean;
  pct?: boolean;
  invalid?: boolean;
}) {
  const format = (input: number) => (Number.isFinite(input) ? String(input) : "");
  const [text, setText] = useState(() => format(value));
  const [editing, setEditing] = useState(false);

  // Adopt external changes (backend round-trip, clamping) unless the user is
  // mid-edit.
  useEffect(() => {
    if (!editing) setText(format(value));
  }, [value, editing]);

  return (
    <input
      className={`input${wide ? " wide" : ""}${pct ? " pct" : ""}${invalid ? " invalid" : ""}`}
      type="text"
      inputMode="decimal"
      value={text}
      step={step}
      min={min}
      max={max}
      placeholder={suffix}
      aria-invalid={invalid ? true : undefined}
      onFocus={() => setEditing(true)}
      onBlur={() => {
        setEditing(false);
        const parsed = Number(text.replace(",", "."));
        if (text.trim() === "" || !Number.isFinite(parsed)) {
          setText(format(value));
          return;
        }
        onChange(parsed);
        setText(format(parsed));
      }}
      onChange={(event) => {
        const raw = event.target.value;
        setText(raw);
        const parsed = Number(raw.replace(",", "."));
        // Report only real numbers; an empty field means "not committed yet".
        if (raw.trim() !== "" && Number.isFinite(parsed)) onChange(parsed);
      }}
    />
  );
}
