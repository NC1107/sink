import { useState } from "react";
import { useMixerStore } from "../../store/mixer";
import type { VirtualSink } from "../../types";
import { MAX_VOLUME } from "../../types";
import { channelIcon, Ms, ICON_CHOICES } from "../Icons";
import { Modal } from "../Modal";
import { Popover } from "../Popover";
import { perceptual, volToDb } from "../../lib/audio";
import { EqModal } from "../Eq/EqModal";
import { ChannelApps } from "./ChannelApps";
import { Fader } from "./Fader";
import { OutputSelect } from "./OutputSelect";
import { VuMeter } from "./VuMeter";

interface ChannelStripProps {
  channel: VirtualSink;
  appCount: number;
  /** Drag-reorder wiring (owned by MixerBoard). */
  dragging: boolean;
  onGripDragStart: (e: React.DragEvent) => void;
  onGripDragEnd: () => void;
  onStripDragOver: (e: React.DragEvent) => void;
}

export function ChannelStrip({
  channel,
  appCount,
  dragging,
  onGripDragStart,
  onGripDragEnd,
  onStripDragOver,
}: ChannelStripProps) {
  const setChannelVolume = useMixerStore((s) => s.setChannelVolume);
  const toggleMute = useMixerStore((s) => s.toggleMute);
  const level = useMixerStore((s) => s.levels[channel.name]);
  const output = useMixerStore((s) => s.channelOutputs[channel.name] ?? null);
  const resolvedOutput = useMixerStore((s) => s.resolvedOutputs[channel.name] ?? null);
  const failover = useMixerStore((s) => s.channelFailover[channel.name] ?? true);
  const setChannelOutput = useMixerStore((s) => s.setChannelOutput);
  const setChannelFailover = useMixerStore((s) => s.setChannelFailover);
  const renameChannel = useMixerStore((s) => s.renameChannel);
  const removeChannel = useMixerStore((s) => s.removeChannel);
  const setChannelIcon = useMixerStore((s) => s.setChannelIcon);
  const channelCount = useMixerStore((s) => s.channels.length);
  const monitoring = useMixerStore((s) => s.monitors[channel.name] ?? false);
  const toggleMonitor = useMixerStore((s) => s.toggleMonitor);

  const eqEnabled = useMixerStore((s) => s.eqConfigs[channel.name]?.enabled ?? false);

  const [editing, setEditing] = useState(false);
  const [draft, setDraft] = useState("");
  const [confirmingDelete, setConfirmingDelete] = useState(false);
  const [pickingIcon, setPickingIcon] = useState(false);
  const [managingApps, setManagingApps] = useState(false);
  const [editingEq, setEditingEq] = useState(false);

  const commitRename = () => {
    setEditing(false);
    const label = draft.trim();
    if (label && label !== channel.label) {
      void renameChannel(channel.name, label);
    }
  };

  // Mono meter: show the louder of L/R.
  const amplitude = Math.max(level?.[0] ?? 0, level?.[1] ?? 0);

  return (
    <div
      className={"strip" + (channel.muted ? " muted" : "") + (dragging ? " dragging" : "")}
      onDragOver={onStripDragOver}
      onDrop={(e) => e.preventDefault()}
    >
      {channelCount > 1 && (
        <span
          className="strip-grip"
          draggable
          title="Drag to reorder"
          onDragStart={onGripDragStart}
          onDragEnd={onGripDragEnd}
        >
          <Ms name="drag_indicator" />
        </span>
      )}
      {channelCount > 1 && (
        <button
          className="strip-x"
          aria-label={`Delete channel ${channel.label}`}
          title="Delete channel"
          onClick={() => setConfirmingDelete(true)}
        >
          <Ms name="close" />
        </button>
      )}

      <div className="strip-head">
        <div style={{ position: "relative" }}>
          <button
            className="strip-icon strip-icon-btn"
            title="Change icon"
            aria-label={`Change icon for ${channel.label}`}
            onClick={() => setPickingIcon(true)}
          >
            <Ms name={channelIcon(channel)} />
          </button>
          <Popover
            open={pickingIcon}
            onClose={() => setPickingIcon(false)}
            side="bottom"
            align="center"
            style={{ minWidth: 196 }}
          >
            <div className="icon-grid">
              {ICON_CHOICES.map((icon) => (
                <button
                  key={icon}
                  className={"icon-cell" + (channelIcon(channel) === icon ? " sel" : "")}
                  onClick={() => {
                    setPickingIcon(false);
                    void setChannelIcon(channel.name, icon);
                  }}
                >
                  <Ms name={icon} />
                </button>
              ))}
            </div>
          </Popover>
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
            title="Double-click to rename"
            onDoubleClick={() => {
              setDraft(channel.label);
              setEditing(true);
            }}
          >
            {channel.label}
          </div>
        )}
        <div style={{ position: "relative" }}>
          <button
            className="strip-meta strip-meta-btn"
            title="Choose which apps play through this channel"
            onClick={() => setManagingApps(true)}
          >
            {appCount} {appCount === 1 ? "app" : "apps"}
            <Ms name="expand_more" style={{ fontSize: 13 }} />
          </button>
          <ChannelApps
            channel={channel}
            open={managingApps}
            onClose={() => setManagingApps(false)}
          />
        </div>
      </div>

      <div className="strip-body">
        <Fader
          value={channel.volume_percent}
          max={MAX_VOLUME}
          onChange={(v) => void setChannelVolume(channel.name, v)}
        />
        <VuMeter target={channel.muted ? 0 : perceptual(amplitude)} />
      </div>

      <div className="strip-readout">
        {channel.volume_percent}
        <span style={{ fontSize: 11 }}>%</span>{" "}
        <span className="db">{volToDb(channel.volume_percent)}</span>
      </div>

      <div className="strip-btns">
        <button
          className={"sbtn" + (channel.muted ? " on-mute" : "")}
          onClick={() => void toggleMute(channel.name, !channel.muted)}
          aria-pressed={channel.muted}
          title={channel.muted ? "Unmute" : "Mute"}
        >
          <Ms name={channel.muted ? "volume_off" : "volume_up"} style={{ fontSize: 16 }} />
        </button>
        <button
          className={"sbtn" + (monitoring ? " on-mon" : "")}
          onClick={() => void toggleMonitor(channel.name)}
          aria-pressed={monitoring}
          title="Monitor — listen to this channel on the default output"
        >
          <Ms name="headphones" style={{ fontSize: 16 }} />
        </button>
        <button
          className={"sbtn" + (eqEnabled ? " on-eq" : "")}
          onClick={() => setEditingEq(true)}
          aria-pressed={eqEnabled}
          title="Equalizer"
        >
          <Ms name="tune" style={{ fontSize: 16 }} />
        </button>
      </div>

      <EqModal channel={channel} open={editingEq} onClose={() => setEditingEq(false)} />

      <OutputSelect
        compact
        value={output}
        resolved={resolvedOutput}
        failover={failover}
        onFailoverChange={(enabled) => void setChannelFailover(channel.name, enabled)}
        onChange={(o) => void setChannelOutput(channel.name, o)}
      />

      <Modal
        open={confirmingDelete}
        onClose={() => setConfirmingDelete(false)}
        title={`Delete "${channel.label}"?`}
      >
        <p className="modal-text">
          Apps routed to this channel return to the default output. Its saved
          routing is removed.
        </p>
        <div className="modal-btns">
          <button
            className="modal-btn danger"
            onClick={() => {
              setConfirmingDelete(false);
              void removeChannel(channel.name);
            }}
          >
            Delete channel
          </button>
          <button className="modal-btn" onClick={() => setConfirmingDelete(false)}>
            Cancel
          </button>
        </div>
      </Modal>
    </div>
  );
}
