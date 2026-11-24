// Thin wrapper over the Tauri IPC bridge with a browser-only mock fallback so the
// UI can be previewed and screenshotted in Chrome without the Rust shell.

import { invoke } from "@tauri-apps/api/core";
import { listen, type UnlistenFn } from "@tauri-apps/api/event";
import type { FiatInfo, Settings, Snapshot } from "../types";
import { mockFiats, mockSettings, mockSnapshot, type Scenario } from "./mock";

export const isTauri = typeof window !== "undefined" && "__TAURI_INTERNALS__" in window;

export const SOUNDS = ["Glass", "Ping", "Submarine", "Funk", "Hero", "Pop", "Purr", "Tink"] as const;

const params = new URLSearchParams(typeof location === "undefined" ? "" : location.search);
const scenario = (params.get("scenario") as Scenario) || "alert";

let mockState: Settings = mockSettings();
let mockTick = 0;

export const api = {
  isMock: !isTauri,

  async getSettings(): Promise<Settings> {
    if (!isTauri) return mockState;
    return invoke<Settings>("get_settings");
  },

  async getSnapshot(): Promise<Snapshot | null> {
    if (!isTauri) {
      mockTick += 1;
      const jitter = mockTick % 2 === 0 ? 0.0006 : -0.0004;
      return mockSnapshot(scenario === "empty" ? "empty" : scenario === "idle" ? "idle" : "alert", jitter);
    }
    return invoke<Snapshot | null>("get_snapshot");
  },

  async saveSettings(settings: Settings): Promise<Settings> {
    if (!isTauri) {
      mockState = settings;
      return settings;
    }
    return invoke<Settings>("save_settings", { settings });
  },

  async refreshNow(): Promise<void> {
    if (!isTauri) return;
    await invoke("refresh_now");
  },

  async discoverPayMethods(fiat: string, side: "buy" | "sell"): Promise<string[]> {
    if (!isTauri) return mockSnapshot("alert").buy.payMethods;
    return invoke<string[]>("discover_pay_methods", { fiat, side });
  },

  async getAccent(): Promise<{ accentLight: string; accentDark: string; dark: boolean }> {
    if (!isTauri) {
      return { accentLight: "#007aff", accentDark: "#0a84ff", dark: false };
    }
    return invoke<{ accentLight: string; accentDark: string; dark: boolean }>("get_accent");
  },

  async listFiats(): Promise<FiatInfo[]> {
    if (!isTauri) return mockFiats;
    return invoke<FiatInfo[]>("list_fiats");
  },

  async openAdvertiser(userNo: string): Promise<void> {
    if (!isTauri) {
      console.info("open advertiser", userNo);
      return;
    }
    await invoke("open_advertiser", { userNo });
  },

  async playSound(name: string): Promise<void> {
    if (!isTauri) return;
    await invoke("play_sound_cmd", { name });
  },

  async testAlert(sound: string): Promise<void> {
    if (!isTauri) {
      console.info("test alert", sound);
      return;
    }
    await invoke("test_alert", { sound });
  },

  async hideWindow(): Promise<void> {
    if (!isTauri) return;
    await invoke("hide_window");
  },

  async quitApp(): Promise<void> {
    if (!isTauri) return;
    await invoke("quit_app");
  },

  onSnapshot(handler: (snapshot: Snapshot) => void): Promise<UnlistenFn> {
    if (!isTauri) return Promise.resolve(() => {});
    return listen<Snapshot>("crossrate://snapshot", (event) => handler(event.payload));
  },

  onSettings(handler: (settings: Settings) => void): Promise<UnlistenFn> {
    if (!isTauri) return Promise.resolve(() => {});
    return listen<Settings>("crossrate://settings", (event) => handler(event.payload));
  },

  onOpenSettings(handler: () => void): Promise<UnlistenFn> {
    if (!isTauri) return Promise.resolve(() => {});
    return listen("crossrate://open-settings", () => handler());
  },

  onOpened(handler: () => void): Promise<UnlistenFn> {
    if (!isTauri) return Promise.resolve(() => {});
    return listen("crossrate://opened", () => handler());
  },
};
