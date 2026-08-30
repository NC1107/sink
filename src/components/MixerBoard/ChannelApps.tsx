import { useMixerStore } from "../../store/mixer";
import type { VirtualSink } from "../../types";
import { AppIcon } from "../AppList/AppIcon";
import { MenuCheckItem } from "../MenuItem";
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

  // One entry per app, not per stream - they route together.
  const live = new Map<string, Entry>();
  for (const s of appStreams) {
    const key = `${s.match_prop}\0${s.match_value}`;
    const existing = live.get(key);
    if (existing) {
      existing.checked ||= s.assigned_sink === channel.name;
      existing.active ||= s.active;
      continue;
    }
    live.set(key, {
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
  const entries: Entry[] = [...live.values()];
  for (const a of seenApps) {
    const key = `${a.match_prop}\0${a.match_value}`;
    if (a.ignored || live.has(key)) continue;
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
        <div className="menu-item static muted">No apps discovered yet</div>
      )}
      {entries.map((entry) => (
        <MenuCheckItem key={entry.key} checked={entry.checked} onClick={() => toggle(entry)}>
          <span className="channel-apps-icon">
            <AppIcon iconPath={entry.iconPath} />
          </span>
          <span className="channel-apps-name">{entry.name}</span>
          {entry.active ? (
            <span className="eq on channel-apps-eq" aria-hidden="true">
              <i />
              <i />
              <i />
            </span>
          ) : (
            entry.streamIndex === null && <span className="channel-apps-off">off</span>
          )}
        </MenuCheckItem>
      ))}
    </Popover>
  );
}
