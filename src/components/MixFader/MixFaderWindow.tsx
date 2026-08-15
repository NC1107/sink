import { useEffect, useRef } from "react";
import { listen } from "@tauri-apps/api/event";
import { useMixerStore, type Levels } from "../../store/mixer";
import { busMembers, MAX_VOLUME, MIC_LEVEL_KEY } from "../../types";
import { perceptual, volToDb } from "../../lib/audio";
import { channelIcon, Ms } from "../Icons";
import { Fader } from "../MixerBoard/Fader";
import { VuMeter } from "../MixerBoard/VuMeter";
import { MixFaderTitleBar } from "./MixFaderTitleBar";

const POLL_INTERVAL_MS = 2000;

/**
 * Standalone popout window (see `open_mix_fader_window`): independent send
 * levels for one mix's members, styled as the same channel-strip cards the
 * main mixer board uses. A fader here only changes what this mix's
 * recorders/listeners hear - your own headphones stay at each channel's
 * normal volume. Meant to be left open on screen while streaming/in a call.
 */
export function MixFaderWindow({ busName }: Readonly<{ busName: string }>) {
  const bus = useMixerStore((s) => s.buses.find((b) => b.name === busName));
  const channels = useMixerStore((s) => s.channels);
  const micEnabled = useMixerStore((s) => s.micConfig?.enabled ?? false);
  const fetchBuses = useMixerStore((s) => s.fetchBuses);
  const fetchChannels = useMixerStore((s) => s.fetchChannels);
  const fetchMic = useMixerStore((s) => s.fetchMic);
  const setLevels = useMixerStore((s) => s.setLevels);
  const setBusMemberGain = useMixerStore((s) => s.setBusMemberGain);

  useEffect(() => {
    // No init/teardown here - the main window already owns the audio
    // graph's lifecycle. This window only ever reads and nudges levels.
    const poll = () => {
      void fetchBuses();
      void fetchChannels();
      void fetchMic();
    };
    poll();
    const id = setInterval(poll, POLL_INTERVAL_MS);
    return () => clearInterval(id);
  }, [fetchBuses, fetchChannels, fetchMic]);

  useEffect(() => {
    const unlisten = listen<Levels>("levels", (event) => setLevels(event.payload));
    return () => {
      void unlisten.then((fn) => fn());
    };
  }, [setLevels]);

  if (!bus) {
    return (
      <div className="window">
        <MixFaderTitleBar label="Mix" />
        <div className="mix-fader-body mix-fader-empty">
          <Ms name="tune" style={{ fontSize: 28 }} />
          <p>This mix no longer exists. You can close this window.</p>
        </div>
      </div>
    );
  }

  const allNames = channels.map((c) => c.name);
  const memberNames = busMembers(bus, allNames);
  const memberChannels = channels.filter((c) => memberNames.includes(c.name));

  return (
    <div className="window">
      <MixFaderTitleBar label={bus.label} />
      <div className="mix-fader-body">
        <p className="mix-fader-hint">
          Independent levels for this mix - your own listening volume is unaffected.
        </p>

        <div className="mix-fader-rows">
          {bus.mic && (
            <MixFaderRow
              label="Microphone"
              icon="mic"
              meterKey={MIC_LEVEL_KEY}
              disabled={!micEnabled}
              value={bus.member_gains[MIC_LEVEL_KEY] ?? 100}
              onChange={(v) => void setBusMemberGain(bus.name, MIC_LEVEL_KEY, v)}
            />
          )}
          {memberChannels.map((c) => (
            <MixFaderRow
              key={c.name}
              label={c.label}
              icon={channelIcon(c)}
              meterKey={c.name}
              value={bus.member_gains[c.name] ?? 100}
              onChange={(v) => void setBusMemberGain(bus.name, c.name, v)}
            />
          ))}
          {!bus.mic && memberChannels.length === 0 && (
            <p className="mix-fader-hint">This mix has no members yet.</p>
          )}
        </div>
      </div>
    </div>
  );
}

function MixFaderRow({
  label,
  icon,
  meterKey,
  value,
  disabled,
  onChange,
}: Readonly<{
  label: string;
  icon: string;
  meterKey: string;
  value: number;
  disabled?: boolean;
  onChange: (value: number) => void;
}>) {
  const level = useMixerStore((s) => s.levels[meterKey]);
  const amplitude = Math.max(level?.[0] ?? 0, level?.[1] ?? 0);
  const muted = value === 0;
  // Remembers the level a mute should restore, mirroring the main strips'
  // mute behavior - but scoped to this mix only (see setBusMemberGain).
  const preMute = useRef(100);

  const toggleMuteInMix = () => {
    if (muted) {
      onChange(preMute.current || 100);
    } else {
      preMute.current = value;
      onChange(0);
    }
  };

  return (
    <div className={"strip" + (muted ? " muted" : "")}>
      <div className="strip-head">
        <div className="strip-icon" title={disabled ? "Mic is currently disabled (Mic tab)" : undefined}>
          <Ms name={icon} />
        </div>
        <div className="strip-name">{label}</div>
      </div>

      <div className="strip-body">
        <Fader value={value} max={MAX_VOLUME} onChange={onChange} />
        <VuMeter target={muted ? 0 : perceptual(amplitude)} />
      </div>

      <div className="strip-readout">
        {value}
        <span style={{ fontSize: 11 }}>%</span> <span className="db">{volToDb(value)}</span>
      </div>

      <div className="strip-btns">
        <button
          type="button"
          className={"sbtn" + (muted ? " on-mute" : "")}
          onClick={toggleMuteInMix}
          aria-pressed={muted}
          title={muted ? "Unmute in this mix" : "Mute in this mix (recorders/listeners of this mix only)"}
        >
          <Ms name={muted ? "volume_off" : "volume_up"} style={{ fontSize: 16 }} />
        </button>
        <button
          type="button"
          className="sbtn"
          disabled={value === 100}
          onClick={() => onChange(100)}
          title="Reset to 100% (this mix's normal level)"
        >
          <Ms name="restart_alt" style={{ fontSize: 16 }} />
        </button>
      </div>
    </div>
  );
}
