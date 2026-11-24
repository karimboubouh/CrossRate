// Thin wrapper over the Tauri IPC bridge.
//
// This module contains no demo data and no reference to the fixture module.
// In a plain browser (local design work only) src/lib/preview.ts patches this
// object at startup; that import is dev-only and is eliminated from production
// builds, so the shipped app can only ever talk to the real backend.

import { invoke } from "@tauri-apps/api/core";
import { listen, type UnlistenFn } from "@tauri-apps/api/event";
import type { FiatInfo, Settings, Snapshot } from "../types";

export const isTauri = typeof window !== "undefined" && "__TAURI_INTERNALS__" in window;

export const SOUNDS = ["Glass", "Ping", "Submarine", "Funk", "Hero", "Pop", "Purr", "Tink"] as const;

export const api = {
  /** Overridden by the dev-only preview layer; always false in the app. */
  isMock: false,

  async getSettings(): Promise<Settings> {
    return invoke<Settings>("get_settings");
  },

  async getSnapshot(): Promise<Snapshot | null> {
    return invoke<Snapshot | null>("get_snapshot");
  },

  async saveSettings(settings: Settings): Promise<Settings> {
    return invoke<Settings>("save_settings", { settings });
  },

  async refreshNow(): Promise<void> {
    await invoke("refresh_now");
  },

  async discoverPayMethods(fiat: string, side: "buy" | "sell"): Promise<string[]> {
    return invoke<string[]>("discover_pay_methods", { fiat, side });
  },

  async getAccent(): Promise<{ accentLight: string; accentDark: string; dark: boolean }> {
    return invoke<{ accentLight: string; accentDark: string; dark: boolean }>("get_accent");
  },

  async listFiats(): Promise<FiatInfo[]> {
    return invoke<FiatInfo[]>("list_fiats");
  },

  async openAdvertiser(userNo: string): Promise<void> {
    await invoke("open_advertiser", { userNo });
  },

  async playSound(name: string): Promise<void> {
    await invoke("play_sound_cmd", { name });
  },

  async testAlert(sound: string): Promise<void> {
    await invoke("test_alert", { sound });
  },

  async setAppearance(theme: "system" | "light" | "dark"): Promise<void> {
    await invoke("set_appearance", { theme });
  },

  async setWindowHeight(height: number): Promise<void> {
    await invoke("set_window_height", { height });
  },

  async hideWindow(): Promise<void> {
    await invoke("hide_window");
  },

  async quitApp(): Promise<void> {
    await invoke("quit_app");
  },

  onSnapshot(handler: (snapshot: Snapshot) => void): Promise<UnlistenFn> {
    return listen<Snapshot>("crossrate://snapshot", (event) => handler(event.payload));
  },

  onSettings(handler: (settings: Settings) => void): Promise<UnlistenFn> {
    return listen<Settings>("crossrate://settings", (event) => handler(event.payload));
  },

  onOpenSettings(handler: () => void): Promise<UnlistenFn> {
    return listen("crossrate://open-settings", () => handler());
  },

  onOpened(handler: () => void): Promise<UnlistenFn> {
    return listen("crossrate://opened", () => handler());
  },
};

export type Api = typeof api;
