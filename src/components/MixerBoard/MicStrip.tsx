import { useState } from "react";
import { useMixerStore } from "../../store/mixer";
import { MAX_MIC_GAIN, MIC_LEVEL_KEY } from "../../types";
import { perceptual } from "../../lib/audio";
import { Ms } from "../Icons";
import { Fader } from "./Fader";
import { VuMeter } from "./VuMeter";

/** Mic channel strip (Phase 3): fader = chain gain, meters = processed
 * signal. Only rendered while the mic chain is enabled. */
export function MicStrip() {
  const micConfig = useMixerStore((s) => s.micConfig);
  const setMicConfig = useMixerStore((s) => s.setMicConfig);
  const level = useMixerStore((s) => s.levels[MIC_LEVEL_KEY]);
  const monitoring = useMixerStore((s) => s.monitors[MIC_LEVEL_KEY] ?? false);
  const toggleMonitor = useMixerStore((s) => s.toggleMonitor);
  const [editing, setEditing] = useState(false);
  const [draft, setDraft] = useState("");

  if (!micConfig?.enabled) return null;

  const commitRename = () => {
    setEditing(false);
    const label = draft.trim();
    if (label && label !== micConfig.output_label)
      void setMicConfig({ output_label: label });
  };

  const target = micConfig.muted ? 0 : perceptual(level?.[0] ?? 0);

  return (
      <div className={"strip input-strip" + (micConfig.muted ? " muted" : "")}>
      <div className="strip-head">
        <div className="strip-icon strip-icon-mic">
          <Ms name="mic" />
        </div>
        {editing ? (
          <input
            className="menu-input strip-name-input"
            value={draft}
            autoFocus
            maxLength={32}
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
            title="Double-click to rename - other apps see this name"
            onDoubleClick={() => {
              setDraft(micConfig.output_label);
              setEditing(true);
            }}
          >
            {micConfig.output_label}
          </div>
        )}
        <div className="strip-meta">capture</div>
      </div>

      <div className="strip-body">
        <Fader
          value={micConfig.gain_percent}
          max={MAX_MIC_GAIN}
          onChange={(v) => void setMicConfig({ gain_percent: v })}
        />
        <VuMeter target={target} />
      </div>

      <div className="strip-readout">
        {micConfig.gain_percent}
        <span style={{ fontSize: 11 }}>%</span>{" "}
        <span className="db">gain</span>
      </div>

      <div className="strip-btns">
        <button
          type="button"
          className={"sbtn" + (micConfig.muted ? " on-mute" : "")}
          onClick={() => void setMicConfig({ muted: !micConfig.muted })}
          aria-pressed={micConfig.muted}
          title={micConfig.muted ? "Unmute mic" : "Mute mic"}
        >
          <Ms name={micConfig.muted ? "mic_off" : "mic"} style={{ fontSize: 16 }} />
        </button>
        <button
          type="button"
          className={"sbtn" + (monitoring ? " on-mon" : "")}
          onClick={() => void toggleMonitor(MIC_LEVEL_KEY)}
          aria-pressed={monitoring}
          title="Sidetone - hear your processed mic on the default output"
        >
          <Ms name="headphones" style={{ fontSize: 16 }} />
        </button>
      </div>

      <div className="strip-route" />
      </div>
  );
}
