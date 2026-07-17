import { useMixerStore } from "../../store/mixer";
import type { SeenApp } from "../../types";
import { relativeTime } from "../../lib/format";
import { Ms } from "../Icons";
import { AppIcon } from "./AppIcon";
import { ChannelSelect } from "./ChannelSelect";

/**
 * A previously-seen app that isn't currently playing. Routing edits here
 * are "pre-routing": they take effect the moment the app next plays audio.
 */
export function InactiveRow({ app }: Readonly<{ app: SeenApp }>) {
  const setAppAssignment = useMixerStore((s) => s.setAppAssignment);
  const setAppIgnored = useMixerStore((s) => s.setAppIgnored);
  const forgetApp = useMixerStore((s) => s.forgetApp);

  return (
    <div className="row row-inactive">
      <div className="ricon">
        <AppIcon iconPath={app.icon_path} />
      </div>
      <div className="rmain">
        <div className="rtitle" title={app.match_value}>
          <span className="rname">{app.alias ?? app.display_name}</span>
        </div>
        <div className="rsub">last seen {relativeTime(app.last_seen)}</div>
      </div>
      <div className="rtrail">
        <ChannelSelect
          value={app.assigned_sink}
          onChange={(sinkName) => void setAppAssignment(app, sinkName === "" ? null : sinkName)}
        />
        <button
          className="rename-btn row-action"
          title="Ignore - hide this app from Sink"
          aria-label={`Ignore ${app.display_name}`}
          onClick={() => void setAppIgnored(app, true)}
        >
          <Ms name="visibility_off" style={{ fontSize: 16 }} />
        </button>
        <button
          className="rename-btn row-action"
          title="Forget - erase from history (and its routing/alias)"
          aria-label={`Forget ${app.display_name}`}
          onClick={() => void forgetApp(app)}
        >
          <Ms name="delete" style={{ fontSize: 16 }} />
        </button>
      </div>
    </div>
  );
}
