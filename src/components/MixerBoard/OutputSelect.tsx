import { useState } from "react";
import type { CSSProperties } from "react";
import { useMixerStore } from "../../store/mixer";
import type { OutputDevice } from "../../types";
import { Ms } from "../Icons";
import { MenuCheckItem } from "../MenuItem";
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

  const toggle = (name: string) =>
    onChange(value.includes(name) ? value.filter((n) => n !== name) : [...value, name]);

  const items = (
    <>
      {/* Not a device but the absence of a choice: ticked whenever nothing
          is, so unticking the last device lands back here. */}
      <MenuCheckItem checked={!mixed && value.length === 0} onClick={() => onChange([])}>
        <span className="menu-item-label">System default</span>
      </MenuCheckItem>
      <div className="menu-sep" />
      {outputDevices.map((d) => (
        <MenuCheckItem
          key={d.name}
          checked={!mixed && value.includes(d.name)}
          onClick={() => toggle(d.name)}
        >
          <span className="menu-item-label">{d.description}</span>
        </MenuCheckItem>
      ))}
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
