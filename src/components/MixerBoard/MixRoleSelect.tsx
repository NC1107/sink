import { useState } from "react";
import { Ms } from "../Icons";
import { MenuItem } from "../MenuItem";
import { Popover } from "../Popover";

interface MixRoleSelectProps {
  /** True: the mix is a recording device. False: a playback device. */
  input: boolean;
  onChange: (input: boolean) => void;
}

/**
 * Which device list a mix shows up in, in the same strip footer where a
 * channel picks its output. The wording is the system's own ("Recording
 * devices", "Playback devices") so the label matches what the user sees
 * in their sound settings.
 */
export function MixRoleSelect({ input, onChange }: Readonly<MixRoleSelectProps>) {
  const [open, setOpen] = useState(false);
  const pick = (value: boolean) => {
    onChange(value);
    setOpen(false);
  };

  return (
    <div style={{ position: "relative" }}>
      <button
        type="button"
        className="strip-route strip-route-btn"
        onClick={() => setOpen((o) => !o)}
        title={
          input
            ? "Shows up as a recording device, which is what a recorder captures"
            : "Shows up as a playback device; a recorder captures its monitor"
        }
      >
        <Ms name={input ? "mic" : "speaker"} />
        <span className="strip-route-name">{input ? "Recording" : "Playback"}</span>
        <Ms name="expand_more" />
      </button>
      <Popover
        open={open}
        onClose={() => setOpen(false)}
        side="top"
        align="center"
        style={{ minWidth: 250 }}
      >
        <MenuItem
          icon="mic"
          selected={input}
          showCheck
          onClick={() => pick(true)}
          title="Where a recorder looks first"
        >
          Recording device
        </MenuItem>
        <MenuItem
          icon="speaker"
          selected={!input}
          showCheck
          onClick={() => pick(false)}
          title="Keeps the mix out of your microphone list; recorders capture its monitor"
        >
          Playback device
        </MenuItem>
      </Popover>
    </div>
  );
}
