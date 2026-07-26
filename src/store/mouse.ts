import { create } from "zustand";
import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";

// Mirrors the Rust structs in src-tauri/src/mouse/.
export interface MouseStatus {
  present: boolean;
  model: string | null;
  wireless: boolean;
  battery_percent: number | null;
  charging: boolean;
}

export interface MouseConfig {
  dpi_presets: number[];
  dpi_selected: number;
  polling_hz: number;
  zones: [number, number, number][];
  rainbow: boolean;
  reactive: [number, number, number] | null;
  sleep_minutes: number;
  dim_seconds: number;
}

interface MouseSnapshot {
  connected: boolean;
  status: MouseStatus;
  config: MouseConfig;
  dpi_steps: number[];
  polling_rates: number[];
}

const emptyStatus: MouseStatus = {
  present: false,
  model: null,
  wireless: false,
  battery_percent: null,
  charging: false,
};

const defaultConfig: MouseConfig = {
  dpi_presets: [400, 800, 1200, 2400, 3200],
  dpi_selected: 0,
  polling_hz: 1000,
  zones: [
    [255, 0, 0],
    [0, 255, 0],
    [0, 0, 255],
  ],
  rainbow: true,
  reactive: null,
  sleep_minutes: 5,
  dim_seconds: 30,
};

/** Zone labels in device order. */
export const ZONE_LABELS = ["Top", "Middle", "Bottom"] as const;

interface MouseState {
  connected: boolean;
  status: MouseStatus;
  config: MouseConfig;
  dpiSteps: number[];
  pollingRates: number[];
  error: string | null;
  _initialized: boolean;

  init: () => Promise<void>;
  refresh: () => Promise<void>;
  setDpi: (presets: number[], selected: number) => Promise<void>;
  setPolling: (hz: number) => Promise<void>;
  setZoneColor: (zone: number, rgb: [number, number, number]) => Promise<void>;
  setRainbow: () => Promise<void>;
  setReactive: (rgb: [number, number, number] | null) => Promise<void>;
  setSleep: (minutes: number) => Promise<void>;
  setDim: (seconds: number) => Promise<void>;
}

export const useMouse = create<MouseState>((set, get) => {
  // Device writes are slow (the mouse needs ~50 ms between commands), so a
  // colour-picker drag must not fire one invoke per pixel.
  const pending = new Map<string, ReturnType<typeof setTimeout>>();
  const debounced = (key: string, fn: () => Promise<unknown>, ms = 200) => {
    const prev = pending.get(key);
    if (prev) clearTimeout(prev);
    pending.set(
      key,
      setTimeout(() => {
        pending.delete(key);
        void fn().catch((e) => set({ error: String(e) }));
      }, ms),
    );
  };

  return {
    connected: false,
    status: emptyStatus,
    config: defaultConfig,
    dpiSteps: [],
    pollingRates: [125, 250, 500, 1000],
    error: null,
    _initialized: false,

    refresh: async () => {
      const snap = await invoke<MouseSnapshot>("get_mouse_status");
      set({
        connected: snap.connected,
        status: snap.status,
        config: snap.config,
        dpiSteps: snap.dpi_steps,
        pollingRates: snap.polling_rates,
      });
    },

    init: async () => {
      if (get()._initialized) return;
      set({ _initialized: true });
      try {
        await get().refresh();
      } catch (e) {
        console.error("mouse init:", e);
      }
      void listen<MouseStatus>("mouse-status", (e) => set({ status: e.payload }));
      void listen<boolean>("mouse-presence", (e) => {
        set((s) => ({
          connected: e.payload,
          status: e.payload ? s.status : emptyStatus,
        }));
        if (e.payload) void get().refresh();
      });
    },

    setDpi: async (presets, selected) => {
      set((s) => ({
        config: { ...s.config, dpi_presets: presets, dpi_selected: selected },
      }));
      debounced("dpi", () => invoke("mouse_set_dpi", { presets, selected }));
    },
    setPolling: async (hz) => {
      set((s) => ({ config: { ...s.config, polling_hz: hz } }));
      await invoke("mouse_set_polling", { hz });
    },
    setZoneColor: async (zone, rgb) => {
      set((s) => {
        const zones = s.config.zones.map((z, i) => (i === zone ? rgb : z));
        return { config: { ...s.config, zones, rainbow: false } };
      });
      debounced(`zone:${zone}`, () =>
        invoke("mouse_set_zone_color", { zone, r: rgb[0], g: rgb[1], b: rgb[2] }),
      );
    },
    setRainbow: async () => {
      set((s) => ({ config: { ...s.config, rainbow: true } }));
      await invoke("mouse_set_rainbow");
    },
    setReactive: async (rgb) => {
      set((s) => ({ config: { ...s.config, reactive: rgb } }));
      await invoke("mouse_set_reactive", { rgb });
    },
    setSleep: async (minutes) => {
      set((s) => ({ config: { ...s.config, sleep_minutes: minutes } }));
      debounced("sleep", () => invoke("mouse_set_sleep", { minutes }));
    },
    setDim: async (seconds) => {
      set((s) => ({ config: { ...s.config, dim_seconds: seconds } }));
      debounced("dim", () => invoke("mouse_set_dim", { seconds }));
    },
  };
});
