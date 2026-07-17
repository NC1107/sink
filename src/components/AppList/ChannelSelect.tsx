import { useState } from "react";
import { useMixerStore } from "../../store/mixer";
import { UNASSIGNED } from "../../types";
import { channelIcon, Ms } from "../Icons";
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

  return (
    <div style={{ position: "relative" }}>
      <button className="select" onClick={() => setOpen((o) => !o)}>
        <Ms name={current ? channelIcon(current) : "help"} />
        <span style={{ minWidth: 52, textAlign: "left" }}>
          {current ? current.label : "Unrouted"}
        </span>
        <Ms name="expand_more" />
      </button>
      <Popover open={open} onClose={() => setOpen(false)} side="bottom" align="end">
        {channels.map((c) => (
          <div
            key={c.name}
            className={"menu-item" + (c.name === value ? " sel" : "")}
            onClick={() => {
              onChange(c.name);
              setOpen(false);
            }}
          >
            <Ms name={channelIcon(c)} />
            <span>{c.label}</span>
            {c.name === value && <Ms name="check" style={{ marginLeft: "auto" }} />}
          </div>
        ))}
        <div
          className={"menu-item" + (value === null ? " sel" : "")}
          onClick={() => {
            onChange(UNASSIGNED);
            setOpen(false);
          }}
        >
          <Ms name="block" />
          <span>Unrouted</span>
          {value === null && <Ms name="check" style={{ marginLeft: "auto" }} />}
        </div>
      </Popover>
    </div>
  );
}
