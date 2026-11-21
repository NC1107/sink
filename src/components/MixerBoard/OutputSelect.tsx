import { useState } from "react";
import { useMixerStore } from "../../store/mixer";
import { MAX_CHANNEL_OUTPUTS, type OutputDevice } from "../../types";
import { Ms } from "../Icons";
import { MenuCheckItem } from "../MenuItem";
import { Popover } from "../Popover";
import { Toggle } from "../Toggle";

interface OutputSelectProps {
  /** The channel's name, heading the menu: it opens over its neighbours. */
  channelLabel: string;
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
  onChange: (outputNames: string[]) => void;
}

function deviceIcon(description: string): string {
  const d = description.toLowerCase();
  if (d.includes("headphone") || d.includes("headset") || d.includes("arctis")) return "headphones";
  if (d.includes("hdmi") || d.includes("display")) return "tv";
  if (d.includes("bluetooth")) return "bluetooth";
  return "speaker";
}

interface OutputSummary {
  /** Full wording, for the tooltip and assistive tech. */
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
): OutputSummary {
  // An unplugged device is still a choice; show its node name until it's back.
  const describe = (name: string) => devices.find((d) => d.name === name)?.description ?? name;
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
  channelLabel,
  value,
  resolved = [],
  failover,
  onFailoverChange,
  onChange,
}: Readonly<OutputSelectProps>) {
  const [open, setOpen] = useState(false);
  const outputDevices = useMixerStore((s) => s.outputDevices);
  const { label, shortLabel, icon } = summarizeOutputs(value, resolved, outputDevices);

  const toggle = (name: string) =>
    onChange(value.includes(name) ? value.filter((n) => n !== name) : [...value, name]);
  // Chosen devices that are unplugged right now. They are still the
  // channel's choice (and come back by themselves), so they need a row to
  // be seen and unticked on.
  const disconnected = value.filter((name) => !outputDevices.some((d) => d.name === name));
  const full = value.length >= MAX_CHANNEL_OUTPUTS;
  // Where "System default" lands right now - only known while following it.
  const defaultDevice =
    value.length === 0 ? outputDevices.find((d) => d.name === resolved[0]) : undefined;

  return (
    <div style={{ position: "relative" }}>
      <button
        type="button"
        className="strip-route strip-route-btn"
        onClick={() => setOpen((o) => !o)}
        title={`Output: ${label}`}
        aria-label={`Output: ${label}`}
        aria-haspopup="menu"
        aria-expanded={open}
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
        // A long device name is ellipsised (its row's tooltip has all of
        // it) instead of making the menu cover half the board.
        style={{ width: 340 }}
      >
        <div className="menu-head">{channelLabel} output</div>
        {/* Not a device but the absence of a choice: ticked whenever nothing
            is, so unticking the last device lands back here. */}
        <MenuCheckItem
          checked={value.length === 0}
          title={
            defaultDevice
              ? `Follows the system default, now ${defaultDevice.description}`
              : "Follows whatever the system default output is"
          }
          onClick={() => {
            if (value.length > 0) onChange([]);
          }}
        >
          <span className="menu-item-label">System default</span>
          {defaultDevice && <span className="menu-item-note">{defaultDevice.description}</span>}
        </MenuCheckItem>
        <div className="menu-sep" />
        {outputDevices.map((d) => {
          const checked = value.includes(d.name);
          return (
            <MenuCheckItem
              key={d.name}
              checked={checked}
              disabled={full && !checked}
              title={
                full && !checked
                  ? `A channel plays on at most ${MAX_CHANNEL_OUTPUTS} devices`
                  : d.description
              }
              onClick={() => toggle(d.name)}
            >
              <span className="menu-item-label">{d.description}</span>
            </MenuCheckItem>
          );
        })}
        {disconnected.map((name) => (
          <MenuCheckItem
            key={name}
            checked
            className="muted"
            title={`${name} - plays here again when it is reconnected`}
            onClick={() => toggle(name)}
          >
            <span className="menu-item-label">{name}</span>
            <span className="menu-item-note">not connected</span>
          </MenuCheckItem>
        ))}
        {onFailoverChange && (
          <>
            <div className="menu-sep" />
            {/* Static row: the Toggle is the control, so this must not be a
                button of its own. */}
            <div
              className="menu-item static"
              title="When none of this channel's devices is connected, play on another one instead of going silent"
            >
              <Ms name="sync_alt" />
              <span className="menu-item-label">Fall back when unplugged</span>
              <Toggle on={failover ?? true} onClick={() => onFailoverChange(!(failover ?? true))} />
            </div>
          </>
        )}
      </Popover>
    </div>
  );
}
