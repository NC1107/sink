import { useEffect } from "react";
import { getCurrentWindow } from "@tauri-apps/api/window";
import { useMixerStore } from "../../store/mixer";
import { busMembers, MIC_LEVEL_KEY } from "../../types";
import { channelIcon, Ms, SinkMark } from "../Icons";
import { SendRow } from "./SendRow";

const POLL_INTERVAL_MS = 2000;

/** "Recording" -> "Recording Mix"; labels already ending in "mix" stay. */
function windowTitle(label?: string): string {
  if (!label) return "Mix";
  return label.toLowerCase().endsWith("mix") ? label : `${label} Mix`;
}

/**
 * A mix's popout window (see `open_mix_fader_window`): the one home of the
 * mix's per-member send levels, meant to be left on screen while streaming
 * or in a call. Reads and nudges levels only - the main window owns the
 * audio graph's lifecycle.
 */
export function MixPopout({ busName }: Readonly<{ busName: string }>) {
  const win = getCurrentWindow();
  const bus = useMixerStore((s) => s.buses.find((b) => b.name === busName));
  const channels = useMixerStore((s) => s.channels);
  const fetchBuses = useMixerStore((s) => s.fetchBuses);
  const fetchChannels = useMixerStore((s) => s.fetchChannels);
  const setBusMemberGain = useMixerStore((s) => s.setBusMemberGain);

  // This window is its own webview, so it polls for itself - the main
  // window's store can't reach it.
  useEffect(() => {
    const poll = () => {
      void fetchBuses();
      void fetchChannels();
    };
    poll();
    const id = setInterval(poll, POLL_INTERVAL_MS);
    return () => clearInterval(id);
  }, [fetchBuses, fetchChannels]);

  const carried = bus ? busMembers(bus, channels.map((c) => c.name)) : [];

  return (
    <div className="popout">
      <header data-tauri-drag-region className="headerbar">
        <div data-tauri-drag-region className="hb-brand">
          <div className="hb-logo">
            <SinkMark />
          </div>
          <div data-tauri-drag-region className="hb-title">
            Sink
          </div>
        </div>
        <div data-tauri-drag-region className="hb-sub">
          {windowTitle(bus?.label)}
        </div>
        <div data-tauri-drag-region className="hb-spacer" />
        <div className="wctl">
          <button type="button" className="wbtn" aria-label="Minimize" onClick={() => void win.minimize()}>
            <Ms name="remove" />
          </button>
          <button
            type="button"
            className="wbtn close"
            aria-label="Close window"
            onClick={() => void win.close()}
          >
            <Ms name="close" />
          </button>
        </div>
      </header>
      <div className="popout-body">
        {bus ? (
          <>
            {bus.mic && (
              <SendRow
                icon="mic"
                label="Microphone"
                value={bus.member_gains[MIC_LEVEL_KEY] ?? 100}
                onChange={(v) => void setBusMemberGain(bus.name, MIC_LEVEL_KEY, v)}
              />
            )}
            {channels
              .filter((c) => carried.includes(c.name))
              .map((c) => (
                <SendRow
                  key={c.name}
                  icon={channelIcon(c)}
                  label={c.label}
                  value={bus.member_gains[c.name] ?? 100}
                  onChange={(v) => void setBusMemberGain(bus.name, c.name, v)}
                />
              ))}
            {!bus.mic && carried.length === 0 && (
              <p className="send-levels-hint">This mix has no members yet.</p>
            )}
          </>
        ) : (
          <p className="send-levels-hint">This mix no longer exists.</p>
        )}
      </div>
    </div>
  );
}
