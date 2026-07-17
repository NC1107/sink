import { useState } from "react";
import { useMixerStore } from "../../store/mixer";
import type { AppStream } from "../../types";
import { Ms } from "../Icons";
import { AppIcon } from "./AppIcon";
import { ChannelSelect } from "./ChannelSelect";
import { HSlider } from "./HSlider";

interface AppRowProps {
  stream: AppStream;
}

export function AppRow({ stream }: Readonly<AppRowProps>) {
  const routeApp = useMixerStore((s) => s.routeApp);
  const setAppVolume = useMixerStore((s) => s.setAppVolume);
  const renameApp = useMixerStore((s) => s.renameApp);
  const setAppIgnored = useMixerStore((s) => s.setAppIgnored);

  const [editing, setEditing] = useState(false);
  const [draft, setDraft] = useState("");

  const displayName = stream.alias ?? stream.app_name;

  const startEdit = () => {
    setDraft(displayName);
    setEditing(true);
  };
  const commit = () => {
    setEditing(false);
    const next = draft.trim();
    // Re-entering the discovered name clears the alias.
    void renameApp(stream, next === stream.app_name ? "" : next);
  };

  return (
    <div className="row">
      <div className="ricon">
        <AppIcon iconPath={stream.icon_path} />
      </div>
      <div className="rmain">
        {editing ? (
          <input
            className="menu-input"
            style={{ width: "100%", maxWidth: 260 }}
            value={draft}
            autoFocus
            maxLength={64}
            onChange={(e) => setDraft(e.target.value)}
            onBlur={commit}
            onKeyDown={(e) => {
              if (e.key === "Enter") commit();
              if (e.key === "Escape") setEditing(false);
            }}
          />
        ) : (
          <div className="rtitle" title={stream.app_name}>
            <span
              className={"eq" + (stream.active ? " on" : "")}
              title={stream.active ? "Playing audio" : "Silent"}
              aria-hidden="true"
            >
              <i />
              <i />
              <i />
            </span>
            <span className="rname">{displayName}</span>
            {stream.alias && (
              <span className="tag" title={`Discovered as "${stream.app_name}"`}>
                {stream.app_name}
              </span>
            )}
            <button className="rename-btn" aria-label={`Rename ${displayName}`} onClick={startEdit}>
              <Ms name="edit" style={{ fontSize: 14 }} />
            </button>
            <button
              className="rename-btn"
              title="Ignore - hide this app from Sink"
              aria-label={`Ignore ${displayName}`}
              onClick={() => void setAppIgnored(stream, true)}
            >
              <Ms name="visibility_off" style={{ fontSize: 14 }} />
            </button>
          </div>
        )}
        <div className="rsub">stream #{stream.index}</div>
      </div>
      <div className="rtrail">
        <HSlider
          value={stream.volume_percent}
          max={100}
          onChange={(v) => void setAppVolume(stream.index, v)}
        />
        <ChannelSelect
          value={stream.assigned_sink}
          onChange={(sinkName) => void routeApp(stream.index, sinkName)}
        />
      </div>
    </div>
  );
}
