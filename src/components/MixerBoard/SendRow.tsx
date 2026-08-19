import { useRef } from "react";
import { MAX_VOLUME } from "../../types";
import { Ms } from "../Icons";
import { HSlider } from "../AppList/HSlider";

/** One member's send level within one mix: what that mix's recorders hear,
 *  independent of the member's own fader. Mute is gain 0 scoped to this mix,
 *  restoring the pre-mute level on unmute (the main strips' mute shape). */
export function SendRow({
  icon,
  label,
  value,
  onChange,
}: Readonly<{
  icon: string;
  label: string;
  value: number;
  onChange: (value: number) => void;
}>) {
  const muted = value === 0;
  const preMute = useRef(100);

  const toggleMute = () => {
    if (muted) {
      onChange(preMute.current || 100);
    } else {
      preMute.current = value;
      onChange(0);
    }
  };

  return (
    <div className={"send-row" + (muted ? " muted" : "")}>
      <Ms name={icon} style={{ fontSize: 14 }} />
      <span className="send-row-label">{label}</span>
      <HSlider value={value} max={MAX_VOLUME} onChange={onChange} />
      <button
        type="button"
        className={"sbtn send-row-btn" + (muted ? " on-mute" : "")}
        aria-pressed={muted}
        onClick={toggleMute}
        title={muted ? "Unmute in this mix" : "Mute in this mix"}
      >
        <Ms name={muted ? "volume_off" : "volume_up"} style={{ fontSize: 14 }} />
      </button>
      <button
        type="button"
        className="sbtn send-row-btn"
        disabled={value === 100}
        onClick={() => onChange(100)}
        title="Reset to 100%"
      >
        <Ms name="restart_alt" style={{ fontSize: 14 }} />
      </button>
    </div>
  );
}
