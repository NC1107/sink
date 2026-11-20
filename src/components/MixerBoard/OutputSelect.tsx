import { useState } from "react";
import type { CSSProperties } from "react";
import { useMixerStore } from "../../store/mixer";
import type { OutputDevice } from "../../types";
import { Ms } from "../Icons";
import { MenuItem } from "../MenuItem";
import { Popover } from "../Popover";
import { Toggle } from "../Toggle";

interface OutputSelectProps {
  /** Chosen output node names; the channel plays on all of them. Empty =
   *  follow system default. */
  value: string[];
  /**
   * The devices this channel is actually playing on right now. Shown while
   * following the system default, so failover to another device is visible.
   */
  resolved?: string[];
  /** Whether this channel fails over to another device (default true). */
  failover?: boolean;
  /** Toggle auto-failover for this channel. Omit to hide the toggle. */
  onFailoverChange?: (enabled: boolean) => void;
  /** "Mixed" display for the all-channels pill when selections differ. */
  mixed?: boolean;
  onChange: (outputNames: string[]) => void;
  /** Compact footer style (channel strip) vs pill style (mixer top bar). */
  compact?: boolean;
  popoverStyle?: CSSProperties;
}

function deviceIcon(description: string): string {
  const d = description.toLowerCase();
  if (d.includes("headphone") || d.includes("headset") || d.includes("arctis")) return "headphones";
  if (d.includes("hdmi") || d.includes("display")) return "tv";
  if (d.includes("bluetooth")) return "bluetooth";
  return "speaker";
}

interface OutputSummary {
  /** Full wording for the tooltip and the pill. */
  label: string;
  /** One short word (or count) that fits a 122px strip footer. */
  shortLabel: string;
  icon: string;
}

/** How a channel's output choice reads on its strip. */
export function summarizeOutputs(
  value: string[],
  resolved: string[],
  devices: OutputDevice[],
  mixed = false,
): OutputSummary {
  // An unplugged device is still a choice; show its node name until it's back.
  const describe = (name: string) => devices.find((d) => d.name === name)?.description ?? name;
  if (mixed) return { label: "Per-channel", shortLabel: "Mixed", icon: "speaker_group" };
  if (value.length > 1) {
    return {
      label: value.map(describe).join(" + "),
      shortLabel: `${value.length} outputs`,
      icon: "speaker_group",
    };
  }
  if (value.length === 1) {
    const description = describe(value[0]);
    const known = devices.some((d) => d.name === value[0]);
    return {
      label: description,
      shortLabel: description.split(" ")[0],
      icon: known ? deviceIcon(description) : "arrow_forward",
    };
  }
  // Following the default: name the device it lands on, so failover shows.
  const landed = devices.find((d) => d.name === resolved[0]);
  if (landed) {
    return {
      label: `System default (${landed.description})`,
      shortLabel: landed.description.split(" ")[0],
      icon: deviceIcon(landed.description),
    };
  }
  return { label: "System default", shortLabel: "Default", icon: "arrow_forward" };
}

export function OutputSelect({
  value,
  resolved = [],
  failover,
  onFailoverChange,
  mixed,
  onChange,
  compact,
  popoverStyle,
}: Readonly<OutputSelectProps>) {
  const [open, setOpen] = useState(false);
  const outputDevices = useMixerStore((s) => s.outputDevices);
  const { label, shortLabel, icon } = summarizeOutputs(value, resolved, outputDevices, mixed);

  const pickOnly = (outputNames: string[]) => {
    onChange(outputNames);
    setOpen(false);
  };

  const items = (
    <>
      <MenuItem
        icon="speaker_group"
        selected={!mixed && value.length === 0}
        showCheck
        onClick={() => pickOnly([])}
      >
        System default
      </MenuItem>
      {outputDevices.map((d) => {
        const chosen = !mixed && value.includes(d.name);
        // The last chosen device has nothing to be removed in favour of;
        // "System default" is how a channel stops pinning one.
        const removable = chosen && value.length > 1;
        return (
          <div key={d.name} className="menu-row">
            {/* Clicking a device plays on it alone: switching outputs stays
                one click. The trailing button adds or drops it instead. */}
            <MenuItem
              icon={deviceIcon(d.description)}
              selected={chosen}
              showCheck
              onClick={() => pickOnly([d.name])}
            >
              {d.description}
            </MenuItem>
            {(!chosen || removable) && (
              <button
                type="button"
                className="menu-row-btn"
                aria-label={
                  chosen ? `Stop playing on ${d.description}` : `Also play on ${d.description}`
                }
                title={chosen ? "Stop playing on this device" : "Also play on this device"}
                onClick={() =>
                  onChange(chosen ? value.filter((name) => name !== d.name) : [...value, d.name])
                }
              >
                <Ms name={chosen ? "remove" : "add"} />
              </button>
            )}
          </div>
        );
      })}
      {onFailoverChange && !mixed && (
        <>
          <div className="menu-sep" />
          {/* Static row: the Toggle is the control, so this must not be a
              button of its own. */}
          <div
            className="menu-item static"
            title="Off: stays silent if none of its devices is there"
          >
            <Ms name="sync_alt" />
            <span className="menu-item-label">Fail over to another device</span>
            <Toggle on={failover ?? true} onClick={() => onFailoverChange(!(failover ?? true))} />
          </div>
        </>
      )}
    </>
  );

  if (compact) {
    return (
      <div style={{ position: "relative" }}>
        <button
          type="button"
          className="strip-route strip-route-btn"
          onClick={() => setOpen((o) => !o)}
          title={`Output: ${label}`}
        >
          <Ms name={icon} />
          <span className="strip-route-name">{shortLabel}</span>
          <Ms name="expand_more" />
        </button>
        <Popover
          open={open}
          onClose={() => setOpen(false)}
          side="top"
          align="center"
          style={popoverStyle}
        >
          {items}
        </Popover>
      </div>
    );
  }

  return (
    <div style={{ position: "relative" }}>
      <button type="button" className="out-pill" onClick={() => setOpen((o) => !o)}>
        <Ms name={icon} />
        <span>{label}</span>
        <Ms name="expand_more" className="chev" />
      </button>
      <Popover
        open={open}
        onClose={() => setOpen(false)}
        side="bottom"
        align="start"
        style={popoverStyle}
      >
        {items}
      </Popover>
    </div>
  );
}
