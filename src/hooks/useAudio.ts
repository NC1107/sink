import { useEffect, useRef } from "react";
import { listen } from "@tauri-apps/api/event";
import {
  useMixerStore,
  type HardwareChatMixEvent,
  type Levels,
} from "../store/mixer";

const POLL_INTERVAL_MS = 2000;

/**
 * Boots the audio layer: creates the virtual sinks on mount, polls the app
 * stream list + device list every 2s while the window is on screen,
 * subscribes to live VU level events, and auto-loads profiles bound to
 * newly connected devices (Phase 5).
 */
export function useAudio() {
  const initialize = useMixerStore((s) => s.initialize);
  const fetchAppStreams = useMixerStore((s) => s.fetchAppStreams);
  const fetchOutputs = useMixerStore((s) => s.fetchOutputs);
  const fetchSeenApps = useMixerStore((s) => s.fetchSeenApps);
  const setLevels = useMixerStore((s) => s.setLevels);
  const applyHardwareChatMix = useMixerStore((s) => s.applyHardwareChatMix);
  const outputDevices = useMixerStore((s) => s.outputDevices);
  const profiles = useMixerStore((s) => s.profiles);
  const loadProfile = useMixerStore((s) => s.loadProfile);

  useEffect(() => {
    void initialize();
    let id: ReturnType<typeof setInterval> | undefined;
    const poll = () => {
      void fetchAppStreams();
      void fetchOutputs();
      void fetchSeenApps();
    };
    const start = () => {
      if (id === undefined) {
        poll(); // refresh immediately so a returning window isn't stale
        id = setInterval(poll, POLL_INTERVAL_MS);
      }
    };
    const stop = () => {
      if (id !== undefined) {
        clearInterval(id);
        id = undefined;
      }
    };
    // Pause the 4-IPC poll while hidden in the tray - the product's dominant
    // idle state - instead of round-tripping every 2s forever (TD-009). This
    // only stops the UI refreshing: auto-routing is enforced by a backend
    // ticker, so assignments hold while the window is away.
    const onVisibility = () => (document.hidden ? stop() : start());
    if (!document.hidden) start();
    document.addEventListener("visibilitychange", onVisibility);
    return () => {
      stop();
      document.removeEventListener("visibilitychange", onVisibility);
    };
  }, [initialize, fetchAppStreams, fetchOutputs, fetchSeenApps]);

  useEffect(() => {
    const unlisten = listen<Levels>("levels", (event) => setLevels(event.payload));
    return () => {
      void unlisten.then((fn) => fn());
    };
  }, [setLevels]);

  // Hardware reports already changed PipeWire and Rust's in-memory model.
  // Mirror them locally without calling setChannelVolume, which would send
  // the same update back through IPC and create redundant work.
  useEffect(() => {
    const unlisten = listen<HardwareChatMixEvent>("arctis-chatmix", (event) => {
      applyHardwareChatMix(event.payload);
    });
    return () => {
      void unlisten.then((fn) => fn());
    };
  }, [applyHardwareChatMix]);

  // Profile switched from the tray menu - sync the whole UI.
  const onProfileChanged = useMixerStore((s) => s.onProfileChanged);
  useEffect(() => {
    const unlisten = listen<string>("profile-changed", (event) => {
      void onProfileChanged(event.payload);
    });
    return () => {
      void unlisten.then((fn) => fn());
    };
  }, [onProfileChanged]);

  // Hardware profile auto-switch: when a device with a bound profile
  // appears, load that profile (Sonar-style).
  const seenDevices = useRef<Set<string> | null>(null);
  useEffect(() => {
    const names = new Set(outputDevices.map((d) => d.name));
    if (seenDevices.current === null) {
      // First sample: just learn the current device set.
      if (names.size > 0) seenDevices.current = names;
      return;
    }
    for (const name of names) {
      if (!seenDevices.current.has(name)) {
        const bound = profiles.find((p) => p.trigger_device === name);
        if (bound) void loadProfile(bound.name);
      }
    }
    seenDevices.current = names;
  }, [outputDevices, profiles, loadProfile]);
}
