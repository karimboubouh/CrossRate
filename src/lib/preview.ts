// Development-only preview layer (browser, not the Tauri app).
//
// main.tsx imports this through a DEV-guarded dynamic import, so neither this
// file nor ./mock is part of a production build.

import type { Api } from "./api";
import { mockFiats, mockSettings, mockSnapshot } from "./mock";

export function installPreview(api: Api): void {
  const scenario =
    typeof location === "undefined"
      ? "alert"
      : new URLSearchParams(location.search).get("scenario") ?? "alert";

  let settings = mockSettings();
  let tick = 0;

  const target = api as unknown as Record<string, unknown>;
  target.isMock = true;

  target.getSettings = async () => settings;
  target.getSnapshot = async () => {
    tick += 1;
    const jitter = tick % 2 === 0 ? 0.0006 : -0.0004;
    const name = scenario === "empty" ? "empty" : scenario === "idle" ? "idle" : "alert";
    return mockSnapshot(name, jitter);
  };
  target.saveSettings = async (next: unknown) => {
    settings = next as typeof settings;
    return settings;
  };
  target.refreshNow = async () => {};
  target.discoverPayMethods = async () => mockSnapshot("alert").buy.payMethods;
  target.getAccent = async () => ({
    accentLight: "#007aff",
    accentDark: "#0a84ff",
    dark: false,
  });
  target.listFiats = async () => mockFiats;
  target.openAdvertiser = async () => {};
  target.playSound = async () => {};
  target.testAlert = async () => {};
  target.setAppearance = async () => {};
  target.setWindowHeight = async () => {};
  target.hideWindow = async () => {};
  target.quitApp = async () => {};
  target.onSnapshot = async () => () => {};
  target.onSettings = async () => () => {};
  target.onOpenSettings = async () => () => {};
  target.onOpened = async () => () => {};
}
