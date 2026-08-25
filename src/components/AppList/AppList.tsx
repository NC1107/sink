import { useState } from "react";
import { useMixerStore } from "../../store/mixer";
import type { AppStream } from "../../types";
import { Ms } from "../Icons";
import { AppRow } from "./AppRow";
import { InactiveRow } from "./InactiveRow";

/** Apps screen: live apps grouped by channel, previously-seen apps below
 * (pre-routable while closed), ignored apps tucked away at the bottom. */
export function AppList() {
  const appStreams = useMixerStore((s) => s.appStreams);
  const channels = useMixerStore((s) => s.channels);
  const seenApps = useMixerStore((s) => s.seenApps);
  const [showIgnored, setShowIgnored] = useState(false);

  const identity = (s: AppStream) => `${s.match_prop}\0${s.match_value}`;
  const byName = (a: AppStream[], b: AppStream[]) =>
    (a[0].alias ?? a[0].app_name).localeCompare(b[0].alias ?? b[0].app_name);

  /** One entry per app, holding all of its streams on this channel: an app
   *  playing several streams at once (a browser tab per video) is one row,
   *  not one row per stream. */
  const byApp = (streams: AppStream[]): AppStream[][] => {
    const apps = new Map<string, AppStream[]>();
    for (const s of streams) {
      const group = apps.get(identity(s));
      if (group) group.push(s);
      else apps.set(identity(s), [s]);
    }
    for (const group of apps.values()) group.sort((a, b) => a.index - b.index);
    return [...apps.values()].sort(byName);
  };

  const groups = [
    ...channels.map((c) => ({
      key: c.name,
      label: c.label,
      apps: byApp(appStreams.filter((s) => s.assigned_sink === c.name)),
    })),
    {
      key: "unrouted",
      label: "Unrouted",
      apps: byApp(appStreams.filter((s) => !s.assigned_sink)),
    },
  ].filter((g) => g.apps.length > 0);

  const liveIdentity = new Set(appStreams.map(identity));
  const inactive = seenApps
    .filter((a) => !a.ignored && !liveIdentity.has(`${a.match_prop}\0${a.match_value}`))
    .sort((a, b) => b.last_seen - a.last_seen);
  const ignored = seenApps.filter((a) => a.ignored);

  return (
    <div className="content">
      <div className="screen-head">
        <h1>Applications</h1>
        <div className="sub">Route each app's audio to a channel</div>
        <div className="screen-head-actions">
          <span className="tag">
            <Ms name="graphic_eq" />
            {liveIdentity.size} {liveIdentity.size === 1 ? "app" : "apps"}
          </span>
        </div>
      </div>
      <div className="screen-scroll">
        {appStreams.length === 0 ? (
          <div className="empty-hint">
            No apps are playing audio.
            <br />
            Start something noisy and it will show up here.
          </div>
        ) : (
          groups.map((group) => (
            <div key={group.key}>
              <div className="section-label">
                {group.label} · {group.apps.length}
              </div>
              <div className="card">
                {group.apps.map((streams) => (
                  <AppRow key={identity(streams[0])} streams={streams} />
                ))}
              </div>
            </div>
          ))
        )}

        {inactive.length > 0 && (
          <>
            <div className="section-label">Not running · {inactive.length}</div>
            <div className="card card-inactive">
              {inactive.map((app) => (
                <InactiveRow key={`${app.match_prop}:${app.match_value}`} app={app} />
              ))}
            </div>
          </>
        )}

        {ignored.length > 0 && (
          <>
            <button type="button" className="ignored-toggle" onClick={() => setShowIgnored((v) => !v)}>
              <Ms name={showIgnored ? "expand_less" : "expand_more"} />
              {ignored.length} ignored {ignored.length === 1 ? "app" : "apps"}
            </button>
            {showIgnored && (
              <div className="card card-inactive">
                {ignored.map((app) => (
                  <InactiveRow key={`${app.match_prop}:${app.match_value}`} app={app} ignored />
                ))}
              </div>
            )}
          </>
        )}
      </div>
    </div>
  );
}
