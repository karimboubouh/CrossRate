import { useMemo, useState } from "react";
import type { FiatInfo } from "../types";

interface Props {
  title: string;
  fiats: FiatInfo[];
  selected: string;
  /** Hide the other side's currency so A and B can never collide. */
  exclude?: string;
  onPick: (code: string) => void;
  onClose: () => void;
}

export function FiatPicker({ title, fiats, selected, exclude, onPick, onClose }: Props) {
  const [query, setQuery] = useState("");
  const filtered = useMemo(() => {
    const needle = query.trim().toLowerCase();
    return fiats
      .filter((fiat) => fiat.code !== exclude)
      .filter(
        (fiat) =>
          needle === "" ||
          fiat.code.toLowerCase().includes(needle) ||
          fiat.name.toLowerCase().includes(needle),
      );
  }, [fiats, query, exclude]);

  return (
    <div className="overlay" onClick={onClose}>
      <div className="picker" onClick={(event) => event.stopPropagation()}>
        <div className="picker-head">
          <input
            className="picker-search"
            placeholder={`${title} — search code or name`}
            value={query}
            autoFocus
            onChange={(event) => setQuery(event.target.value)}
            onKeyDown={(event) => {
              if (event.key === "Escape") onClose();
              if (event.key === "Enter" && filtered.length > 0) onPick(filtered[0].code);
            }}
          />
        </div>
        <div className="picker-list">
          {filtered.length === 0 ? (
            <div className="empty-block">No currency matches “{query}”.</div>
          ) : (
            filtered.map((fiat) => (
              <button
                key={fiat.code}
                type="button"
                className={`picker-item${fiat.code === selected ? " sel" : ""}`}
                onClick={() => onPick(fiat.code)}
              >
                <span className="code">{fiat.code}</span>
                <span className="name">{fiat.name}</span>
              </button>
            ))
          )}
        </div>
      </div>
    </div>
  );
}
