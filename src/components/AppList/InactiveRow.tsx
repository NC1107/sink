import { useMixerStore } from "../../store/mixer";
import type { SeenApp } from "../../types";
import { relativeTime } from "../../lib/format";
import { IconButton } from "../IconButton";
import { AppIcon } from "./AppIcon";
import { ChannelSelect } from "./ChannelSelect";

/**
 * A previously-seen app that isn't currently playing. Routing edits here
 * are "pre-routing": they take effect the moment the app next plays audio.
 * Ignored apps use the same row minus the routing control - they are hidden
 * from Sink until un-ignored, so there is nothing to route.
 */
export function InactiveRow({ app, ignored }: Readonly<{ app: SeenApp; ignored?: boolean }>) {
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
        {!ignored && (
          <ChannelSelect
            value={app.assigned_sink}
            onChange={(sinkName) => void setAppAssignment(app, sinkName === "" ? null : sinkName)}
          />
        )}
        {ignored ? (
          <IconButton
            reveal
            icon="visibility"
            title="Stop ignoring"
            label={`Stop ignoring ${app.display_name}`}
            onClick={() => void setAppIgnored(app, false)}
          />
        ) : (
          <IconButton
            reveal
            icon="visibility_off"
            title="Ignore - hide this app from Sink"
            label={`Ignore ${app.display_name}`}
            onClick={() => void setAppIgnored(app, true)}
          />
        )}
        <IconButton
          reveal
          icon="delete"
          title="Forget - erase from history (and its routing/alias)"
          label={`Forget ${app.display_name}`}
          onClick={() => void forgetApp(app)}
        />
      </div>
    </div>
  );
}
