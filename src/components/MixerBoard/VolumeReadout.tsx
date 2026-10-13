import { useState } from "react";
import { volToDb } from "../../lib/audio";
import { useEditTrigger } from "./useEditTrigger";

interface VolumeReadoutProps {
  percent: number;
  /** Same ceiling as the strip's fader. */
  max?: number;
  onChange: (percent: number) => void;
  /** Shown instead of the dB figure, e.g. the mic's "gain". */
  unit?: string;
}

/** A strip's level, typed in directly on double-click or Enter. */
export function VolumeReadout({
  percent,
  max = 100,
  onChange,
  unit,
}: Readonly<VolumeReadoutProps>) {
  const [draft, setDraft] = useState<string | null>(null);
  const trigger = useEditTrigger(() => setDraft(String(percent)));

  const close = () => {
    setDraft(null);
    trigger.restoreFocus();
  };

  const commit = () => {
    const typed = Number.parseInt(draft ?? "", 10);
    close();
    if (Number.isNaN(typed)) return;
    const next = Math.min(max, Math.max(0, typed));
    if (next !== percent) onChange(next);
  };

  if (draft !== null) {
    return (
      <div className="strip-readout">
        <input
          className="strip-readout-input"
          value={draft}
          autoFocus
          inputMode="numeric"
          maxLength={3}
          aria-label="Level percent"
          onFocus={(e) => e.target.select()}
          onChange={(e) => setDraft(e.target.value.replace(/\D/g, ""))}
          onBlur={commit}
          onKeyDown={(e) => {
            if (e.key === "Enter") commit();
            if (e.key === "Escape") close();
          }}
        />
        <span style={{ fontSize: 11 }}>%</span>
      </div>
    );
  }

  return (
    <div
      {...trigger.props}
      className="strip-readout strip-readout-editable"
      title={`Double-click or press Enter to type a level (0-${max}%)`}
      aria-label={`Level ${percent} percent, press Enter to type a level`}
    >
      {percent}
      <span style={{ fontSize: 11 }}>%</span> <span className="db">{unit ?? volToDb(percent)}</span>
    </div>
  );
}
