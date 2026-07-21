import { useState } from "react";
import { useMixerStore } from "../../store/mixer";
import { UNASSIGNED } from "../../types";
import { channelIcon, Ms } from "../Icons";
import { MenuItem } from "../MenuItem";
import { Popover } from "../Popover";

interface ChannelSelectProps {
  /** Currently assigned sink name, or null when unassigned. */
  value: string | null;
  onChange: (sinkName: string) => void;
}

/** Dropdown to route an app stream onto a channel. */
export function ChannelSelect({ value, onChange }: Readonly<ChannelSelectProps>) {
  const [open, setOpen] = useState(false);
  const channels = useMixerStore((s) => s.channels);

  const current = channels.find((c) => c.name === value);
  // An assignment outlives the channel it names - deleted here, or belonging
  // to a profile that isn't loaded. Reporting that as "Unrouted" would be a
  // lie, and picking Unrouted to confirm it would delete a real assignment.
  const missing = value !== null && !current;

  let icon: string;
  if (current) icon = channelIcon(current);
  else if (missing) icon = "link_off";
  else icon = "block";

  let label: string;
  if (current) label = current.label;
  else if (missing) label = "Missing";
  else label = "Unrouted";

  return (
    <div style={{ position: "relative" }}>
      <button
        type="button"
        className="select"
        onClick={() => setOpen((o) => !o)}
        title={
          missing
            ? `Assigned to "${value}", which no profile in use has. Audio plays on the default output until that channel exists again.`
            : undefined
        }
      >
        <Ms name={icon} />
        <span style={{ minWidth: 52, textAlign: "left" }}>{label}</span>
        <Ms name="expand_more" />
      </button>
      <Popover open={open} onClose={() => setOpen(false)} side="bottom" align="end">
        {channels.map((c) => (
          <MenuItem
            key={c.name}
            icon={channelIcon(c)}
            selected={c.name === value}
            showCheck
            onClick={() => {
              onChange(c.name);
              setOpen(false);
            }}
          >
            {c.label}
          </MenuItem>
        ))}
        <MenuItem
          icon="block"
          selected={value === null}
          showCheck
          onClick={() => {
            onChange(UNASSIGNED);
            setOpen(false);
          }}
        >
          Unrouted
        </MenuItem>
      </Popover>
    </div>
  );
}
