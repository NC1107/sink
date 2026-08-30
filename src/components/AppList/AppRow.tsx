import { useState } from "react";
import { useMixerStore } from "../../store/mixer";
import type { AppStream } from "../../types";
import { IconButton } from "../IconButton";
import { AppIcon } from "./AppIcon";
import { ChannelSelect } from "./ChannelSelect";
import { HSlider } from "./HSlider";

interface AppRowProps {
  /** Every live stream of one app on one channel, lowest index first. */
  streams: AppStream[];
}

export function AppRow({ streams }: Readonly<AppRowProps>) {
  const routeApp = useMixerStore((s) => s.routeApp);
  const setAppVolume = useMixerStore((s) => s.setAppVolume);
  const renameApp = useMixerStore((s) => s.renameApp);
  const setAppIgnored = useMixerStore((s) => s.setAppIgnored);

  const [editing, setEditing] = useState(false);
  const [draft, setDraft] = useState("");

  // The oldest stream stands for the app; volume applies to all below.
  const stream = streams[0];
  const displayName = stream.alias ?? stream.app_name;
  const active = streams.some((s) => s.active);

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
              className={"eq" + (active ? " on" : "")}
              title={active ? "Playing audio" : "Silent"}
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
            <IconButton
              reveal
              size={14}
              icon="edit"
              title="Rename"
              label={`Rename ${displayName}`}
              onClick={startEdit}
            />
            <IconButton
              reveal
              size={14}
              icon="visibility_off"
              title="Ignore - hide this app from Sink"
              label={`Ignore ${displayName}`}
              onClick={() => void setAppIgnored(stream, true)}
            />
          </div>
        )}
        {streams.length > 1 && <div className="rsub">{streams.length} streams</div>}
      </div>
      <div className="rtrail">
        <HSlider
          value={stream.volume_percent}
          max={100}
          onChange={(v) => {
            for (const s of streams) void setAppVolume(s.index, v);
          }}
        />
        <ChannelSelect
          value={stream.assigned_sink}
          onChange={(sinkName) => void routeApp(stream.index, sinkName)}
        />
      </div>
    </div>
  );
}
