import { useCallback, useEffect, useRef, useState } from "react";
import type { FiatInfo, LegSide, Settings, Snapshot } from "./types";
import { api } from "./lib/api";
import { Dashboard } from "./components/Dashboard";
import { SettingsPanel } from "./components/SettingsPanel";

function sleep(ms: number): Promise<void> {
  return new Promise((resolve) => window.setTimeout(resolve, ms));
}

export default function App() {
  // Dev-only preview hooks (?scenario=settings&tab=filters&theme=dark).
  const previewParams =
    import.meta.env.DEV && typeof location !== "undefined"
      ? new URLSearchParams(location.search)
      : null;

  const [settings, setSettings] = useState<Settings | null>(null);
  const [snapshot, setSnapshot] = useState<Snapshot | null>(null);
  const [fiats, setFiats] = useState<FiatInfo[]>([]);
  const [version, setVersion] = useState<string>("");
  const [view, setView] = useState<"dashboard" | "settings">(
    previewParams?.get("scenario") === "settings" ? "settings" : "dashboard",
  );
  const [saving, setSaving] = useState(false);
  const [refreshing, setRefreshing] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [loadError, setLoadError] = useState<string | null>(null);
  const [reloadToken, setReloadToken] = useState(0);
  const [toast, setToast] = useState<{ text: string; kind: "ok" | "err" } | null>(null);

  const toastTimer = useRef<number | null>(null);
  const settingsRef = useRef<Settings | null>(null);
  const snapshotRef = useRef<Snapshot | null>(null);

  const showToast = useCallback((text: string, kind: "ok" | "err" = "ok") => {
    setToast({ text, kind });
    if (toastTimer.current) window.clearTimeout(toastTimer.current);
    toastTimer.current = window.setTimeout(() => setToast(null), 2600);
  }, []);

  const adoptSettings = useCallback((next: Settings) => {
    settingsRef.current = next;
    setSettings(next);

    // A pair change invalidates the current market: never render the old
    // pair's numbers under the new pair's labels.
    const current = snapshotRef.current;
    if (current && (current.fiatA !== next.fiatA || current.fiatB !== next.fiatB)) {
      snapshotRef.current = null;
      setSnapshot(null);
    }
  }, []);

  const adoptSnapshot = useCallback((next: Snapshot | null) => {
    snapshotRef.current = next;
    setSnapshot(next);
  }, []);

  // Adopt the real macOS accent colour so controls match System Settings.
  useEffect(() => {
    void api.getAccent()
      .then((appearance) => {
        const root = document.documentElement;
        root.style.setProperty("--accent-light", appearance.accentLight);
        root.style.setProperty("--accent-dark", appearance.accentDark);
      })
      .catch(() => {
        /* accent is cosmetic; system defaults remain */
      });
  }, []);

  // Theme: follow the system unless the user pins light/dark. Mirrored to
  // localStorage so the correct appearance paints before React mounts.
  useEffect(() => {
    if (!settings) return; // don't clobber the pre-paint value while loading
    const mode = settings.theme ?? "system";
    // Keep the native material and the webview's own appearance in sync with
    // the pin, otherwise light-on-dark (or dark-on-light) becomes unreadable.
    void api.setAppearance(mode).catch(() => {});
    const root = document.documentElement;
    if (mode === "light" || mode === "dark") {
      root.dataset.theme = mode;
      try {
        window.localStorage.setItem("crossrate.theme", mode);
      } catch {
        /* storage unavailable */
      }
    } else {
      delete root.dataset.theme;
      try {
        window.localStorage.removeItem("crossrate.theme");
      } catch {
        /* ignore */
      }
    }
  }, [settings]);

  // Initial load + live wiring, with a retry path if the bridge is not ready.
  useEffect(() => {
    let disposed = false;
    const unlisteners: (() => void)[] = [];
    const track = (off: () => void) => {
      if (disposed) off();
      else unlisteners.push(off);
    };

    setLoadError(null);
    void Promise.all([
      api.getSettings(),
      api.getSnapshot(),
      api.listFiats(),
      api.getVersion().catch(() => ""),
    ])
      .then(([nextSettings, nextSnapshot, nextFiats, nextVersion]) => {
        if (disposed) return;
        adoptSettings(nextSettings);
        if (nextSnapshot) adoptSnapshot(nextSnapshot);
        setFiats(nextFiats);
        setVersion(nextVersion);
      })
      .catch((err) => {
        if (disposed) return;
        setLoadError(err instanceof Error ? err.message : String(err));
      });

    void api.onSnapshot((next) => adoptSnapshot(next)).then(track).catch(() => {});
    void api.onSettings((next) => adoptSettings(next)).then(track).catch(() => {});
    void api.onOpenSettings(() => setView("settings")).then(track).catch(() => {});
    void api.onOpened(() => {
      setView("dashboard");
      setError(null);
    }).then(track).catch(() => {});

    return () => {
      disposed = true;
      unlisteners.forEach((off) => off());
    };
  }, [adoptSettings, adoptSnapshot, reloadToken]);

  // Re-render the footer's "updated Xs ago" without polling the backend.
  const [, forceTick] = useState(0);
  useEffect(() => {
    const timer = window.setInterval(() => forceTick((value) => value + 1), 1000);
    return () => window.clearInterval(timer);
  }, []);

  // Size the native popover to its content. ResizeObserver catches tab
  // switches and error lines that the previous dependency-array timer missed.
  const lastHeight = useRef(0);
  useEffect(() => {
    if (api.isMock) return;
    const panel = document.querySelector(".app");
    if (!panel) return;

    let timer: number | null = null;
    const measure = () => {
      if (timer) window.clearTimeout(timer);
      timer = window.setTimeout(() => {
        const height = Math.ceil(panel.getBoundingClientRect().height);
        if (Number.isFinite(height) && height > 0 && Math.abs(height - lastHeight.current) > 2) {
          lastHeight.current = height;
          void api.setWindowHeight(height);
        }
      }, 70);
    };

    const observer = new ResizeObserver(measure);
    observer.observe(panel);
    measure();
    return () => {
      observer.disconnect();
      if (timer) window.clearTimeout(timer);
    };
  }, [settings !== null]);

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
        setError(err instanceof Error ? err.message : String(err));
      } finally {
        setSaving(false);
      }
    },
    [adoptSettings, showToast],
  );

  const handleRefresh = useCallback(async () => {
    setRefreshing(true);
    const before = snapshotRef.current?.ts ?? 0;
    try {
      await api.refreshNow();
      for (let attempt = 0; attempt < 14; attempt += 1) {
        await sleep(150);
        const fresh = await api.getSnapshot();
        if (fresh && fresh.ts !== before) {
          adoptSnapshot(fresh);
          break;
        }
      }
    } catch (err) {
      showToast(err instanceof Error ? err.message : "Refresh failed", "err");
    } finally {
      setRefreshing(false);
    }
  }, [adoptSnapshot, showToast]);

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

      adoptSettings(next);
      try {
        const saved = await api.saveSettings(next);
        adoptSettings(saved);
        showToast(nextList.includes(userNo) ? "Added to favorites" : "Removed from favorites");
      } catch (err) {
        // Revert only this user, so a later successful toggle is not clobbered.
        const latest = settingsRef.current;
        if (latest) {
          const list = latest[key];
          adoptSettings({
            ...latest,
            [key]: current.includes(userNo)
              ? (list.includes(userNo) ? list : [...list, userNo])
              : list.filter((item) => item !== userNo),
          });
        }
        showToast(err instanceof Error ? err.message : String(err), "err");
      }
    },
    [adoptSettings, showToast],
  );

  if (loadError) {
    return (
      <div className="app">
        <div className="empty-block" style={{ marginTop: 120 }}>
          <div>Could not reach the CrossRate engine.</div>
          <div style={{ marginTop: 6, fontSize: 10.5 }}>{loadError}</div>
          <button
            type="button"
            className="btn ghost small"
            style={{ marginTop: 12 }}
            onClick={() => setReloadToken((token) => token + 1)}
          >
            Try again
          </button>
        </div>
      </div>
    );
  }

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
          version={version}
          initialTab={previewParams?.get("tab") ?? undefined}
          onEdit={() => setError(null)}
          onSave={(next) => void handleSave(next)}
          onClose={() => {
            setError(null);
            setView("dashboard");
          }}
          onTest={(sound) => {
            void api
              .testAlert(sound)
              .catch((err) =>
                showToast(err instanceof Error ? err.message : "Test failed", "err"),
              );
          }}
        />
      )}

      {toast ? (
        <div className={`toast${toast.kind === "err" ? " err" : ""}`}>{toast.text}</div>
      ) : null}
    </div>
  );
}
