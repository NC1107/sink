import { useState } from "react";
import { useMixerStore } from "../../store/mixer";
import type { BusDef } from "../../types";
import { busMembers, MASTER_BUS, MAX_VOLUME } from "../../types";
import { perceptual, volToDb } from "../../lib/audio";
import { Ms } from "../Icons";
import { Modal } from "../Modal";
import { Popover } from "../Popover";
import { Fader } from "./Fader";
import { VuMeter } from "./VuMeter";

/**
 * A mix (record bus): aggregates the chosen channels into a capturable
 * source. The label is exactly the device name recorders display - rename
 * it and OBS sees the new name. Volume/mute shape what recorders hear,
 * not what you hear.
 */
/** Compact "what this mix carries" label for the membership button. */
function memberLabel(exclude: boolean, carried: number, all: number): string {
  if (!exclude) return `${carried} ${carried === 1 ? "channel" : "channels"}`;
  if (carried === all) return "all channels";
  return `all but ${all - carried}`;
}

export function BusStrip({ bus }: Readonly<{ bus: BusDef }>) {
  const channels = useMixerStore((s) => s.channels);
  const setBusMembers = useMixerStore((s) => s.setBusMembers);
  const setBusExclude = useMixerStore((s) => s.setBusExclude);
  const renameBus = useMixerStore((s) => s.renameBus);
  const removeBus = useMixerStore((s) => s.removeBus);
  const level = useMixerStore((s) => s.levels[bus.name]);
  const monitoring = useMixerStore((s) => s.monitors[bus.name] ?? false);
  const toggleMonitor = useMixerStore((s) => s.toggleMonitor);
  const setBusVolume = useMixerStore((s) => s.setBusVolume);
  const setBusMute = useMixerStore((s) => s.setBusMute);

  const [managing, setManaging] = useState(false);
  const [editing, setEditing] = useState(false);
  const [draft, setDraft] = useState("");
  const [confirmingDelete, setConfirmingDelete] = useState(false);

  // The master mix always exists and carries every channel.
  const isMaster = bus.name === MASTER_BUS;

  // Volume/mute live on the persisted bus, so they survive remounts, profile
  // switches, and restarts (the backend re-applies them to the fresh node).
  const volume = bus.volume_percent;
  const muted = bus.muted;

  const amplitude = Math.max(level?.[0] ?? 0, level?.[1] ?? 0);

  const applyVolume = (v: number) => void setBusVolume(bus.name, v);
  const toggleMute = () => void setBusMute(bus.name, !muted);
  const commitRename = () => {
    setEditing(false);
    const label = draft.trim();
    if (label && label !== bus.label) void renameBus(bus.name, label);
  };
  // What this mix actually carries (mode-aware).
  const allNames = channels.map((c) => c.name);
  const carried = busMembers(bus, allNames);

  const toggleMember = (channelName: string) => {
    const next = carried.includes(channelName)
      ? carried.filter((c) => c !== channelName)
      : [...carried, channelName];
    void setBusMembers(bus.name, next);
  };

  return (
    <div className={"strip bus-strip" + (muted ? " muted" : "")}>
      {!isMaster && (
        <button
          className="strip-x"
          aria-label={`Delete mix ${bus.label}`}
          title="Delete mix"
          onClick={() => setConfirmingDelete(true)}
        >
          <Ms name="close" />
        </button>
      )}

      <div className="strip-head">
        <div className="strip-icon strip-icon-bus">
          <Ms name="radio_button_checked" />
        </div>
        {editing ? (
          <input
            className="menu-input strip-name-input"
            value={draft}
            autoFocus
            maxLength={24}
            onChange={(e) => setDraft(e.target.value)}
            onBlur={commitRename}
            onKeyDown={(e) => {
              if (e.key === "Enter") commitRename();
              if (e.key === "Escape") setEditing(false);
            }}
          />
        ) : (
          <div
            className="strip-name strip-name-editable"
            title='Double-click to rename - recorders see this name'
            onDoubleClick={() => {
              setDraft(bus.label);
              setEditing(true);
            }}
          >
            {bus.label}
          </div>
        )}
        {isMaster ? (
          <div className="strip-meta" title="The master mix always carries every channel">
            all channels
          </div>
        ) : (
          <div style={{ position: "relative" }}>
            <button
              className="strip-meta strip-meta-btn"
              title="Choose which channels this mix carries"
              onClick={() => setManaging(true)}
            >
              {memberLabel(bus.exclude, carried.length, allNames.length)}
              <Ms name="expand_more" style={{ fontSize: 13 }} />
            </button>
            <Popover
              open={managing}
              onClose={() => setManaging(false)}
              side="bottom"
              align="center"
              style={{ minWidth: 220 }}
            >
              {channels.map((c) => {
                const checked = carried.includes(c.name);
                return (
                  <div key={c.name} className="menu-item" onClick={() => toggleMember(c.name)}>
                    <Ms
                      name={checked ? "check_box" : "check_box_outline_blank"}
                      style={checked ? { color: "var(--accent-hover)" } : undefined}
                    />
                    <span>{c.label}</span>
                  </div>
                );
              })}
              <div className="menu-div" />
              <div
                className="menu-item"
                title="New channels join this mix automatically - keep the ones you don't want unchecked"
                onClick={() => void setBusExclude(bus.name, !bus.exclude)}
              >
                <Ms
                  name={bus.exclude ? "check_box" : "check_box_outline_blank"}
                  style={bus.exclude ? { color: "var(--accent-hover)" } : undefined}
                />
                <span>Auto-include new channels</span>
              </div>
            </Popover>
          </div>
        )}
      </div>

      <div className="strip-body">
        <Fader value={volume} max={MAX_VOLUME} onChange={applyVolume} />
        <VuMeter target={muted ? 0 : perceptual(amplitude)} />
      </div>

      <div className="strip-readout">
        {volume}
        <span style={{ fontSize: 11 }}>%</span> <span className="db">{volToDb(volume)}</span>
      </div>

      <div className="strip-btns">
        <button
          className={"sbtn" + (muted ? " on-mute" : "")}
          onClick={toggleMute}
          aria-pressed={muted}
          title={muted ? "Unmute this mix" : "Mute this mix (recorders hear silence)"}
        >
          <Ms name={muted ? "volume_off" : "volume_up"} style={{ fontSize: 16 }} />
        </button>
        <button
          className={"sbtn" + (monitoring ? " on-mon" : "")}
          onClick={() => void toggleMonitor(bus.name)}
          aria-pressed={monitoring}
          title="Monitor - hear what this mix carries on the default output"
        >
          <Ms name="headphones" style={{ fontSize: 16 }} />
        </button>
      </div>

      <div
        className="strip-route"
        title={`Select "${bus.label}" as an audio source in OBS or any recorder`}
      />

      <Modal
        open={confirmingDelete}
        onClose={() => setConfirmingDelete(false)}
        title={`Delete mix "${bus.label}"?`}
      >
        <p className="modal-text">
          Recorders capturing "{bus.label}" will go silent. Channels are unaffected.
        </p>
        <div className="modal-btns">
          <button
            className="modal-btn danger"
            onClick={() => {
              setConfirmingDelete(false);
              void removeBus(bus.name);
            }}
          >
            Delete mix
          </button>
          <button className="modal-btn" onClick={() => setConfirmingDelete(false)}>
            Cancel
          </button>
        </div>
      </Modal>
    </div>
  );
}
