import { useState } from "react";
import type { CSSProperties } from "react";
import { useMixerStore } from "../../store/mixer";
import { Ms } from "../Icons";
import { MenuItem } from "../MenuItem";
import { Popover } from "../Popover";
import { Toggle } from "../Toggle";

interface OutputSelectProps {
  /** Selected output node name; null = follow system default. */
  value: string | null;
  /**
   * When following the system default, the device this channel actually
   * resolves to right now (node name). Shown on the strip so the user sees
   * where audio really goes, and so failover to another device is visible.
   */
  resolved?: string | null;
  /** Whether this channel fails over to another device (default true). */
  failover?: boolean;
  /** Toggle auto-failover for this channel. Omit to hide the toggle. */
  onFailoverChange?: (enabled: boolean) => void;
  /** "Mixed" display for the all-channels pill when selections differ. */
  mixed?: boolean;
  onChange: (outputName: string | null) => void;
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

export function OutputSelect({
  value,
  resolved,
  failover,
  onFailoverChange,
  mixed,
  onChange,
  compact,
  popoverStyle,
}: Readonly<OutputSelectProps>) {
  const [open, setOpen] = useState(false);
  const outputDevices = useMixerStore((s) => s.outputDevices);

  const current = value === null ? null : outputDevices.find((d) => d.name === value);
  // While following the default, the device it currently resolves to (so the
  // strip shows where audio actually goes, and reflects failover).
  const resolvedDevice =
    value === null && resolved ? outputDevices.find((d) => d.name === resolved) : undefined;
  const shown = current ?? resolvedDevice;

  let label: string;
  if (mixed) label = "Per-channel";
  else if (value !== null) label = current?.description ?? value;
  else if (resolvedDevice) label = `System default (${resolvedDevice.description})`;
  else label = "System default";

  // Compact footer label: a single meaningful word that fits a 122px strip.
  // Following default shows the live device so the user sees where it lands.
  let shortLabel: string;
  if (mixed) shortLabel = "Mixed";
  else if (value !== null) shortLabel = label.split(" ")[0];
  else if (resolvedDevice) shortLabel = resolvedDevice.description.split(" ")[0];
  else shortLabel = "Default";

  const items = (
    <>
      <MenuItem
        icon="speaker_group"
        selected={!mixed && value === null}
        showCheck
        onClick={() => {
          onChange(null);
          setOpen(false);
        }}
      >
        System default
      </MenuItem>
      {outputDevices.map((d) => (
        <MenuItem
          key={d.name}
          icon={deviceIcon(d.description)}
          selected={!mixed && d.name === value}
          showCheck
          onClick={() => {
            onChange(d.name);
            setOpen(false);
          }}
        >
          {d.description}
        </MenuItem>
      ))}
      {onFailoverChange && !mixed && (
        <>
          <div className="menu-sep" />
          {/* Static row: the Toggle is the control, so this must not be a
              button of its own. */}
          <div
            className="menu-item static"
            title="Off: this channel plays only on the device above (or the exact system default) and stays silent if it's gone, instead of failing over to another output."
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
          className="strip-route strip-route-btn"
          onClick={() => setOpen((o) => !o)}
          title={`Output: ${label}`}
        >
          <Ms name={shown ? deviceIcon(shown.description) : "arrow_forward"} />
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
      <button className="out-pill" onClick={() => setOpen((o) => !o)}>
        <Ms name={shown ? deviceIcon(shown.description) : "speaker_group"} />
        <span>{label}</span>
        <Ms name="expand_more" className="chev" />
      </button>
      <Popover open={open} onClose={() => setOpen(false)} side="bottom" align="start" style={popoverStyle}>
        {items}
      </Popover>
    </div>
  );
}
