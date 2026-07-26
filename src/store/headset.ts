import { create } from "zustand";
import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import { useMixerStore } from "./mixer";

// Mirrors the Rust `HeadsetStatus` (src-tauri/src/headset/protocol.rs).
// serde keeps field names as-is (snake_case), so JSON keys match 1:1.
export type AncMode = "off" | "transparent" | "on";
export type LineOutMode = "speaker" | "stream";
export type NotifyScroll = "vertical" | "horizontal";

export interface HeadsetStatus {
  present: boolean;
  power_status: string | null;
  headset_battery_percent: number | null;
  charge_slot_battery_percent: number | null;
  anc: AncMode | null;
  transparency_level: number | null;
  mic_muted: boolean | null;
  mic_led_percent: number | null;
  auto_off_minutes: number | null;
  wireless_range_mode: boolean | null;
  wireless_paired: boolean | null;
  bluetooth_connected: boolean | null;
  bluetooth_powered: boolean | null;
  volume_percent: number | null;
  chatmix_game: number | null;
  chatmix_chat: number | null;
  line_out: LineOutMode | null;
}

interface HeadsetSnapshot {
  connected: boolean;
  status: HeadsetStatus;
  video_supported: boolean;
  model: string | null;
  has_oled: boolean;
}

/** A selectable OLED live mode (mirrors Rust ModeEntry). */
export interface ModeEntry {
  id: string;
  label: string;
  category: string;
}

/** A built-in OLED animation (mirrors Rust ClipEntry). */
export interface ClipEntry {
  id: string;
  label: string;
  category: string;
}

/** A bundled hardware-EQ curve (mirrors Rust EqPreset). */
export interface EqPreset {
  name: string;
  category: string;
  bands: number[];
  description: string;
}

const emptyStatus: HeadsetStatus = {
  present: false,
  power_status: null,
  headset_battery_percent: null,
  charge_slot_battery_percent: null,
  anc: null,
  transparency_level: null,
  mic_muted: null,
  mic_led_percent: null,
  auto_off_minutes: null,
  wireless_range_mode: null,
  wireless_paired: null,
  bluetooth_connected: null,
  bluetooth_powered: null,
  volume_percent: null,
  chatmix_game: null,
  chatmix_chat: null,
  line_out: null,
};

// Auto-off timer: index -> minutes (matches the device's discrete steps).
export const AUTO_OFF_STEPS = [0, 1, 5, 10, 15, 30, 60] as const;

// Debounce device writes so a fader drag doesn't spam HID packets, mirroring
// how the audio store debounces pactl calls.
const pending = new Map<string, ReturnType<typeof setTimeout>>();
function debounced(key: string, fn: () => Promise<unknown>, ms = 120) {
  const existing = pending.get(key);
  if (existing) clearTimeout(existing);
  pending.set(
    key,
    setTimeout(() => {
      pending.delete(key);
      void fn().catch((e) => console.error(`headset ${key}:`, e));
    }, ms),
  );
}

interface HeadsetState {
  connected: boolean;
  videoSupported: boolean;
  status: HeadsetStatus;
  /** Built-in procedural OLED animations, grouped by category. */
  clips: ClipEntry[];
  /** Every live mode the display can show. */
  modes: ModeEntry[];
  /** Show one live mode. */
  oledMode: (id: string) => Promise<void>;
  /** Cycle modes every `secs` seconds; empty list stops rotating. */
  oledRotate: (ids: string[], secs: number) => Promise<void>;
  /** Let the display choose based on what the machine is doing. */
  oledAuto: () => Promise<void>;
  timerCountdown: (secs: number) => Promise<void>;
  timerStopwatch: () => Promise<void>;
  timerToggle: () => Promise<void>;
  timerReset: () => Promise<void>;
  /** WirePlumber anti-crackle headroom quirk installed? */
  alsaHeadroom: boolean;
  /** Connected model name, e.g. "Arctis Nova Pro Wireless". */
  model: string | null;
  /** False on models without a driveable display (e.g. Arctis Pro Wireless). */
  hasOled: boolean;
  /** Bundled hardware-EQ curves. */
  eqPresets: EqPreset[];
  /** Apply a bundled preset; resolves to the applied band gains. */
  applyEqPreset: (name: string) => Promise<number[]>;
  /** Guards one-time snapshot fetch + event subscription. */
  _initialized: boolean;
  /** Save-to-device is debounced after the last change settles. */
  scheduleSave: () => void;

  init: () => Promise<void>;
  setSidetone: (level: number) => void;
  setMicVolume: (level: number) => void;
  setMicLed: (level: number) => void;
  setAnc: (mode: AncMode) => void;
  setTransparency: (level: number) => void;
  setAutoOff: (idx: number) => void;
  setGainHigh: (high: boolean) => void;
  setWirelessRange: (range: boolean) => void;
  setLineOut: (mode: LineOutMode) => void;
  setLineOutVolumes: (left: number, right: number, aux: number) => void;
  setEqBands: (bands: number[]) => void;
  setEqPreset: (preset: number) => void;

  oledText: (lines: string[]) => Promise<void>;
  oledStatus: () => Promise<void>;
  oledSystem: () => Promise<void>;
  oledNowPlaying: () => Promise<void>;
  notifyMirror: boolean;
  setNotifyMirror: (enabled: boolean) => Promise<void>;
  /** How long mirrored notifications stay on the OLED (seconds). */
  notifyDurationSecs: number;
  /** How over-long notification text scrolls so all of it can be read. */
  notifyScroll: NotifyScroll;
  setNotifyDisplay: (durationSecs: number, scroll: NotifyScroll) => Promise<void>;
  oledNotify: (lines: string[], durationMs: number) => Promise<void>;
  oledMedia: (path: string, looping: boolean) => Promise<void>;
  oledClip: (name: string) => Promise<void>;
  oledBrightness: (level: number) => void;
  oledReturnUi: () => Promise<void>;

  setAlsaHeadroom: (enabled: boolean) => Promise<void>;
}

// Optimistically fold a partial status change into the store so the UI reacts
// instantly; the device's own event stream reconciles shortly after.
function patch(status: HeadsetStatus, p: Partial<HeadsetStatus>): HeadsetStatus {
  return { ...status, ...p };
}

export const useHeadset = create<HeadsetState>((set, get) => ({
  connected: false,
  videoSupported: false,
  status: emptyStatus,
  clips: [],
  modes: [],
  alsaHeadroom: false,
  model: null,
  hasOled: false,
  eqPresets: [],
  _initialized: false,

  applyEqPreset: async (name) => {
    const bands = await invoke<number[]>("headset_apply_eq_preset", { name });
    get().scheduleSave();
    return bands;
  },

  scheduleSave: () => debounced("save", () => invoke("headset_save"), 800),

  init: async () => {
    if (get()._initialized) return;
    set({ _initialized: true });
    try {
      const snap = await invoke<HeadsetSnapshot>("get_headset_status");
      set({
        connected: snap.connected,
        status: snap.status,
        videoSupported: snap.video_supported,
        model: snap.model,
        hasOled: snap.has_oled,
      });
      const clips = await invoke<ClipEntry[]>("headset_oled_clips");
      const alsaHeadroom = await invoke<boolean>("headset_get_alsa_headroom");
      const notifyMirror = await invoke<boolean>("headset_get_notify_mirror");
      const notifyDisplay = await invoke<{ duration_secs: number; scroll: NotifyScroll }>(
        "headset_get_notify_display",
      );
      const eqPresets = await invoke<EqPreset[]>("headset_eq_presets");
      const modes = await invoke<ModeEntry[]>("headset_oled_modes");
      set({
        clips,
        modes,
        alsaHeadroom,
        notifyMirror,
        notifyDurationSecs: notifyDisplay.duration_secs,
        notifyScroll: notifyDisplay.scroll,
        eqPresets,
      });
    } catch (e) {
      console.error("headset init:", e);
    }
    void listen<HeadsetStatus>("headset-status", (e) =>
      set({ status: e.payload }),
    );
    void listen<boolean>("headset-presence", (e) => {
      set((s) => ({
        connected: e.payload,
        status: e.payload ? s.status : emptyStatus,
        model: e.payload ? s.model : null,
      }));
      // A different model may have been plugged in; re-read its capabilities.
      if (e.payload) {
        void invoke<HeadsetSnapshot>("get_headset_status").then((snap) =>
          set({ model: snap.model, hasOled: snap.has_oled }),
        );
      }
    });
    // Hardware ChatMix wheel -> software mix (moves the balance channels,
    // which in turn moves the BalanceBar since it derives from their volumes).
    void listen<[number, number]>("headset-chatmix", (e) => {
      const [game, chat] = e.payload;
      useMixerStore.getState().applyChatMix(game, chat);
    });
  },

  setSidetone: (level) => {
    debounced("sidetone", () => invoke("headset_set_sidetone", { level }));
    get().scheduleSave();
  },
  setMicVolume: (level) => {
    set((s) => ({ status: patch(s.status, {}) }));
    debounced("micvol", () => invoke("headset_set_mic_volume", { level }));
    get().scheduleSave();
  },
  setMicLed: (level) => {
    debounced("micled", () => invoke("headset_set_mic_led", { level }));
    get().scheduleSave();
  },
  setAnc: (mode) => {
    set((s) => ({ status: patch(s.status, { anc: mode }) }));
    void invoke("headset_set_anc", { mode }).then(() => get().scheduleSave());
  },
  setTransparency: (level) => {
    set((s) => ({ status: patch(s.status, { transparency_level: level }) }));
    debounced("transp", () => invoke("headset_set_transparency", { level }));
    get().scheduleSave();
  },
  setAutoOff: (idx) => {
    set((s) => ({ status: patch(s.status, { auto_off_minutes: AUTO_OFF_STEPS[idx] }) }));
    void invoke("headset_set_auto_off", { idx }).then(() => get().scheduleSave());
  },
  setGainHigh: (high) => {
    void invoke("headset_set_gain_high", { high }).then(() => get().scheduleSave());
  },
  setWirelessRange: (range) => {
    set((s) => ({ status: patch(s.status, { wireless_range_mode: range }) }));
    void invoke("headset_set_wireless_range", { range }).then(() => get().scheduleSave());
  },
  setLineOut: (mode) => {
    set((s) => ({ status: patch(s.status, { line_out: mode }) }));
    void invoke("headset_set_line_out", { mode }).then(() => get().scheduleSave());
  },
  setLineOutVolumes: (left, right, aux) => {
    debounced("lineoutvol", () =>
      invoke("headset_set_line_out_volumes", { left, right, aux }),
    );
    get().scheduleSave();
  },
  setEqBands: (bands) => {
    debounced("eqbands", () => invoke("headset_set_eq_bands", { bands }));
    get().scheduleSave();
  },
  setEqPreset: (preset) => {
    void invoke("headset_set_eq_preset", { preset }).then(() => get().scheduleSave());
  },

  oledText: (lines) => invoke("headset_oled_text", { lines }),
  oledStatus: () => invoke("headset_oled_status"),
  oledSystem: () => invoke("headset_oled_system"),
  oledNowPlaying: () => invoke("headset_oled_now_playing"),
  notifyMirror: false,
  setNotifyMirror: async (enabled) => {
    set({ notifyMirror: enabled });
    try {
      await invoke("headset_set_notify_mirror", { enabled });
    } catch (e) {
      set({ notifyMirror: !enabled });
      console.error("headset notify mirror:", e);
    }
  },
  notifyDurationSecs: 5,
  notifyScroll: "vertical",
  setNotifyDisplay: async (durationSecs, scroll) => {
    const prev = { secs: get().notifyDurationSecs, scroll: get().notifyScroll };
    set({ notifyDurationSecs: durationSecs, notifyScroll: scroll });
    try {
      await invoke("headset_set_notify_display", { durationSecs, scroll });
    } catch (e) {
      set({ notifyDurationSecs: prev.secs, notifyScroll: prev.scroll });
      console.error("headset notify display:", e);
    }
  },
  oledNotify: (lines, durationMs) =>
    invoke("headset_oled_notify", { lines, durationMs }),
  oledMedia: (path, looping) => invoke("headset_oled_media", { path, looping }),
  oledClip: (name) => invoke("headset_oled_clip", { name }),
  oledMode: (id) => invoke("headset_oled_mode", { id }),
  oledRotate: (ids, secs) => invoke("headset_oled_rotate", { ids, secs }),
  oledAuto: () => invoke("headset_oled_auto"),
  timerCountdown: (secs) => invoke("headset_timer_countdown", { secs }),
  timerStopwatch: () => invoke("headset_timer_stopwatch"),
  timerToggle: () => invoke("headset_timer_toggle"),
  timerReset: () => invoke("headset_timer_reset"),
  oledBrightness: (level) =>
    debounced("brightness", () => invoke("headset_oled_brightness", { level })),
  oledReturnUi: () => invoke("headset_oled_return_ui"),

  setAlsaHeadroom: async (enabled) => {
    set({ alsaHeadroom: enabled });
    try {
      await invoke("headset_set_alsa_headroom", { enabled });
    } catch (e) {
      set({ alsaHeadroom: !enabled });
      console.error("headset alsa headroom:", e);
    }
  },
}));
