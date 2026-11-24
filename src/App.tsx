import { useCallback, useEffect, useRef, useState } from "react";
import type { FiatInfo, LegSide, Settings, Snapshot } from "./types";
import { api } from "./lib/api";
import { Dashboard } from "./components/Dashboard";
import { SettingsPanel } from "./components/SettingsPanel";

export default function App() {
  const preview = new URLSearchParams(
    typeof location === "undefined" ? "" : location.search,
  );
  const [settings, setSettings] = useState<Settings | null>(null);
  const [snapshot, setSnapshot] = useState<Snapshot | null>(null);
  const [fiats, setFiats] = useState<FiatInfo[]>([]);
  const [view, setView] = useState<"dashboard" | "settings">(
    preview.get("scenario") === "settings" ? "settings" : "dashboard",
  );
  const [saving, setSaving] = useState(false);
  const [refreshing, setRefreshing] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [toast, setToast] = useState<{ text: string; kind: "ok" | "err" } | null>(null);
  const toastTimer = useRef<number | null>(null);

  const showToast = useCallback((text: string, kind: "ok" | "err" = "ok") => {
    setToast({ text, kind });
    if (toastTimer.current) window.clearTimeout(toastTimer.current);
    toastTimer.current = window.setTimeout(() => setToast(null), 2600);
  }, []);

  // Initial load + live wiring.
  useEffect(() => {
    void api.getSettings().then(setSettings);
    void api.getSnapshot().then(setSnapshot);
    void api.listFiats().then(setFiats);

    const unlisteners: (() => void)[] = [];
    void api.onSnapshot(setSnapshot).then((off) => unlisteners.push(off));
    void api.onSettings(setSettings).then((off) => unlisteners.push(off));
    void api.onOpenSettings(() => setView("settings")).then((off) => unlisteners.push(off));

    return () => unlisteners.forEach((off) => off());
  }, []);

  // Browser preview: keep the mock alive so screenshots show fresh timestamps.
  useEffect(() => {
    if (api.isMock !== true) return;
    const timer = window.setInterval(() => {
      void api.getSnapshot().then(setSnapshot);
    }, 2500);
    return () => window.clearInterval(timer);
  }, []);

  // Re-render the footer's "updated Xs ago" without polling the backend.
  const [, forceTick] = useState(0);
  useEffect(() => {
    const timer = window.setInterval(() => forceTick((value) => value + 1), 1000);
    return () => window.clearInterval(timer);
  }, []);

  const handleSave = useCallback(
    async (next: Settings) => {
      setSaving(true);
      setError(null);
      try {
        const saved = await api.saveSettings(next);
        setSettings(saved);
        setView("dashboard");
        showToast("Settings saved");
      } catch (err) {
        setError(String(err));
      } finally {
        setSaving(false);
      }
    },
    [showToast],
  );

  const handleRefresh = useCallback(async () => {
    setRefreshing(true);
    try {
      await api.refreshNow();
      const fresh = await api.getSnapshot();
      if (fresh) setSnapshot(fresh);
    } finally {
      window.setTimeout(() => setRefreshing(false), 600);
    }
  }, []);

  const handleToggleFavorite = useCallback(
    async (side: LegSide, userNo: string) => {
      if (!settings) return;
      const key = side === "buy" ? "favoriteBuyUsers" : "favoriteSellUsers";
      const current = settings[key];
      const nextList = current.includes(userNo)
        ? current.filter((item) => item !== userNo)
        : [...current, userNo];
      const next: Settings = { ...settings, [key]: nextList };
      // Optimistic update so the star reacts instantly.
      setSettings(next);
      try {
        const saved = await api.saveSettings(next);
        setSettings(saved);
        showToast(nextList.includes(userNo) ? "Added to favorites" : "Removed from favorites");
      } catch (err) {
        setSettings(settings);
        showToast(String(err), "err");
      }
    },
    [settings, showToast],
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

  const alert = snapshot?.alertActive ?? false;

  return (
    <div className={`app${alert ? " is-alert" : ""}`}>
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
          initialTab={preview.get("tab") ?? undefined}
          onSave={(next) => void handleSave(next)}
          onClose={() => {
            setError(null);
            setView("dashboard");
          }}
          onTest={(sound) => void api.testAlert(sound)}
        />
      )}

      {toast ? <div className={`toast${toast.kind === "err" ? " err" : ""}`}>{toast.text}</div> : null}
    </div>
  );
}
