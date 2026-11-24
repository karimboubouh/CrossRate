import { useCallback, useEffect, useRef, useState } from "react";
import type { FiatInfo, LegSide, Settings, Snapshot } from "./types";
import { api } from "./lib/api";
import { Dashboard } from "./components/Dashboard";
import { SettingsPanel } from "./components/SettingsPanel";

/** Resolves after `ms`; used to wait for the engine's next snapshot. */
function sleep(ms: number): Promise<void> {
  return new Promise((resolve) => window.setTimeout(resolve, ms));
}

export default function App() {
  // Dev-only preview hooks (?scenario=settings&tab=filters&theme=dark). The
  // whole expression is dead code in a production build.
  const previewParams =
    import.meta.env.DEV && typeof location !== "undefined"
      ? new URLSearchParams(location.search)
      : null;

  const [settings, setSettings] = useState<Settings | null>(null);
  const [snapshot, setSnapshot] = useState<Snapshot | null>(null);
  const [fiats, setFiats] = useState<FiatInfo[]>([]);
  const [view, setView] = useState<"dashboard" | "settings">(
    previewParams?.get("scenario") === "settings" ? "settings" : "dashboard",
  );
  const [saving, setSaving] = useState(false);
  const [refreshing, setRefreshing] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [toast, setToast] = useState<{ text: string; kind: "ok" | "err" } | null>(null);

  const toastTimer = useRef<number | null>(null);
  /** Latest settings, read synchronously so rapid edits cannot clobber each other. */
  const settingsRef = useRef<Settings | null>(null);

  const showToast = useCallback((text: string, kind: "ok" | "err" = "ok") => {
    setToast({ text, kind });
    if (toastTimer.current) window.clearTimeout(toastTimer.current);
    toastTimer.current = window.setTimeout(() => setToast(null), 2600);
  }, []);

  const adoptSettings = useCallback((next: Settings) => {
    settingsRef.current = next;
    setSettings(next);
  }, []);

  // Adopt the real macOS accent colour so controls match System Settings.
  useEffect(() => {
    void api.getAccent().then((appearance) => {
      const root = document.documentElement;
      root.style.setProperty("--accent-light", appearance.accentLight);
      root.style.setProperty("--accent-dark", appearance.accentDark);
    });
  }, []);

  // Theme: follow the system unless the user pins light/dark. The value is
  // mirrored to localStorage so the correct appearance paints before React
  // hydrates (main.tsx reads it synchronously).
  useEffect(() => {
    const root = document.documentElement;
    const mode = previewParams?.get("theme") ?? settings?.theme ?? "system";
    if (mode === "light" || mode === "dark") {
      root.dataset.theme = mode;
      try {
        window.localStorage.setItem("crossrate.theme", mode);
      } catch {
        /* storage may be unavailable; not fatal */
      }
    } else {
      delete root.dataset.theme;
      try {
        window.localStorage.removeItem("crossrate.theme");
      } catch {
        /* ignore */
      }
    }
  }, [settings?.theme, previewParams]);

  // Initial load + live wiring. Every listener is tracked so nothing leaks if
  // the bridge resolves after unmount.
  useEffect(() => {
    let disposed = false;
    const unlisteners: (() => void)[] = [];
    const track = (off: () => void) => {
      if (disposed) off();
      else unlisteners.push(off);
    };

    void api.getSettings().then((next) => {
      if (!disposed) adoptSettings(next);
    });
    void api.getSnapshot().then((next) => {
      if (!disposed) setSnapshot(next);
    });
    void api.listFiats().then((list) => {
      if (!disposed) setFiats(list);
    });

    void api.onSnapshot((next) => setSnapshot(next)).then(track);
    void api.onSettings((next) => adoptSettings(next)).then(track);
    void api.onOpenSettings(() => setView("settings")).then(track);
    // Reopening the popup (tray click, notification click, app activation)
    // always lands on the dashboard.
    void api.onOpened(() => {
      setView("dashboard");
      setError(null);
    }).then(track);

    return () => {
      disposed = true;
      unlisteners.forEach((off) => off());
    };
  }, [adoptSettings]);

  // Re-render the footer's "updated Xs ago" without polling the backend.
  const [, forceTick] = useState(0);
  useEffect(() => {
    const timer = window.setInterval(() => forceTick((value) => value + 1), 1000);
    return () => window.clearInterval(timer);
  }, []);

  // Size the native popover to its content, like a real macOS popover.
  const lastHeight = useRef(0);
  useEffect(() => {
    if (api.isMock) return;
    const timer = window.setTimeout(() => {
      const panel = document.querySelector(".app");
      if (!panel) return;
      const height = Math.ceil(panel.getBoundingClientRect().height);
      if (Number.isFinite(height) && height > 0 && Math.abs(height - lastHeight.current) > 2) {
        lastHeight.current = height;
        void api.setWindowHeight(height);
      }
    }, 80);
    return () => window.clearTimeout(timer);
  }, [snapshot, view, settings]);

  const handleSave = useCallback(
    async (next: Settings) => {
      setSaving(true);
      setError(null);
      try {
        const saved = await api.saveSettings(next);
        adoptSettings(saved);
        setView("dashboard");
        showToast("Settings saved");
      } catch (err) {
        // Keep the sheet open with the draft intact so the user can fix it.
        setError(err instanceof Error ? err.message : String(err));
      } finally {
        setSaving(false);
      }
    },
    [adoptSettings, showToast],
  );

  const handleRefresh = useCallback(async () => {
    setRefreshing(true);
    const before = snapshot?.ts ?? 0;
    try {
      await api.refreshNow();
      // The engine polls in the background; wait for a genuinely newer reading
      // instead of immediately re-reading the old snapshot.
      for (let attempt = 0; attempt < 14; attempt += 1) {
        await sleep(150);
        const fresh = await api.getSnapshot();
        if (fresh && fresh.ts !== before) {
          setSnapshot(fresh);
          break;
        }
      }
    } finally {
      setRefreshing(false);
    }
  }, [snapshot?.ts]);

  const handleToggleFavorite = useCallback(
    async (side: LegSide, userNo: string) => {
      const base = settingsRef.current;
      if (!base) return;

      const key = side === "buy" ? "favoriteBuyUsers" : "favoriteSellUsers";
      const current = base[key];
      const nextList = current.includes(userNo)
        ? current.filter((item) => item !== userNo)
        : [...current, userNo];
      const next: Settings = { ...base, [key]: nextList };

      // Optimistic and synchronous: two quick taps compose instead of racing.
      adoptSettings(next);
      try {
        const saved = await api.saveSettings(next);
        adoptSettings(saved);
        showToast(nextList.includes(userNo) ? "Added to favorites" : "Removed from favorites");
      } catch (err) {
        adoptSettings(base);
        showToast(err instanceof Error ? err.message : String(err), "err");
      }
    },
    [adoptSettings, showToast],
  );

  if (!settings) {
    return (
      <div className="app">
        <div className="empty-block" style={{ marginTop: 120 }}>
          Loading CrossRate…
        </div>
      </div>
    );
  }

  return (
    <div className="app">
      {view === "dashboard" ? (
        <Dashboard
          snapshot={snapshot}
          settings={settings}
          onOpenSettings={() => setView("settings")}
          onRefresh={() => void handleRefresh()}
          refreshing={refreshing}
          onToggleFavorite={(side, userNo) => void handleToggleFavorite(side, userNo)}
        />
      ) : (
        <SettingsPanel
          settings={settings}
          fiats={fiats}
          saving={saving}
          error={error}
          initialTab={previewParams?.get("tab") ?? undefined}
          onEdit={() => setError(null)}
          onSave={(next) => void handleSave(next)}
          onClose={() => {
            setError(null);
            setView("dashboard");
          }}
          onTest={(sound) => void api.testAlert(sound)}
        />
      )}

      {toast ? (
        <div className={`toast${toast.kind === "err" ? " err" : ""}`}>{toast.text}</div>
      ) : null}
    </div>
  );
}
