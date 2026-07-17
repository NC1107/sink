import { useMixerStore } from "../../store/mixer";
import type { VirtualSink } from "../../types";
import { Ms } from "../Icons";
import { AppIcon } from "../AppList/AppIcon";
import { Popover } from "../Popover";

interface Entry {
  key: string;
  name: string;
  iconPath: string | null;
  checked: boolean;
  active: boolean;
  /** Live stream index, when the app is currently playing. */
  streamIndex: number | null;
  matchProp: string;
  matchValue: string;
}

/**
 * Channel membership editor: every known app (live and not running) with a
 * checkbox. Checking moves/assigns the app to this channel; unchecking
 * sends it back to the default output.
 */
export function ChannelApps({
  channel,
  open,
  onClose,
}: Readonly<{
  channel: VirtualSink;
  open: boolean;
  onClose: () => void;
}>) {
  const appStreams = useMixerStore((s) => s.appStreams);
  const seenApps = useMixerStore((s) => s.seenApps);
  const routeApp = useMixerStore((s) => s.routeApp);
  const setAppAssignment = useMixerStore((s) => s.setAppAssignment);

  const entries: Entry[] = [];
  const seenKeys = new Set<string>();
  for (const s of appStreams) {
    const key = `${s.match_prop}\0${s.match_value}`;
    seenKeys.add(key);
    entries.push({
      key,
      name: s.alias ?? s.app_name,
      iconPath: s.icon_path,
      checked: s.assigned_sink === channel.name,
      active: s.active,
      streamIndex: s.index,
      matchProp: s.match_prop,
      matchValue: s.match_value,
    });
  }
  for (const a of seenApps) {
    const key = `${a.match_prop}\0${a.match_value}`;
    if (a.ignored || seenKeys.has(key)) continue;
    entries.push({
      key,
      name: a.alias ?? a.display_name,
      iconPath: a.icon_path,
      checked: a.assigned_sink === channel.name,
      active: false,
      streamIndex: null,
      matchProp: a.match_prop,
      matchValue: a.match_value,
    });
  }
  entries.sort((a, b) => Number(b.checked) - Number(a.checked) || a.name.localeCompare(b.name));

  const toggle = (entry: Entry) => {
    if (entry.streamIndex !== null) {
      void routeApp(entry.streamIndex, entry.checked ? "" : channel.name);
    } else {
      void setAppAssignment(
        { match_prop: entry.matchProp, match_value: entry.matchValue },
        entry.checked ? null : channel.name,
      );
    }
  };

  return (
    <Popover open={open} onClose={onClose} side="bottom" align="center" style={{ minWidth: 250 }}>
      {entries.length === 0 && (
        <div className="menu-item" style={{ cursor: "default", color: "var(--fg-muted)" }}>
          No apps discovered yet
        </div>
      )}
      {entries.map((entry) => (
        <div key={entry.key} className="menu-item" onClick={() => toggle(entry)}>
          <Ms
            name={entry.checked ? "check_box" : "check_box_outline_blank"}
            style={entry.checked ? { color: "var(--accent-hover)" } : undefined}
          />
          <span className="channel-apps-icon">
            <AppIcon iconPath={entry.iconPath} />
          </span>
          <span className="channel-apps-name">{entry.name}</span>
          {entry.active ? (
            <span className="eq on" style={{ marginLeft: "auto" }} aria-hidden="true">
              <i />
              <i />
              <i />
            </span>
          ) : (
            entry.streamIndex === null && (
              <span className="channel-apps-off">off</span>
            )
          )}
        </div>
      ))}
    </Popover>
  );
}
