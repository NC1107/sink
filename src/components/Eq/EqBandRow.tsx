import type { EqBand, EqBandKind } from "../../types";
import { EQ_FREQ_MAX_HZ, EQ_FREQ_MIN_HZ, EQ_GAIN_RANGE_DB } from "../../types";
import { Ms } from "../Icons";

const KINDS: { value: EqBandKind; label: string }[] = [
  { value: "peaking", label: "Peak" },
  { value: "low_shelf", label: "Low shelf" },
  { value: "high_shelf", label: "High shelf" },
  { value: "low_pass", label: "Low pass" },
  { value: "high_pass", label: "High pass" },
];

const isShelf = (kind: EqBandKind) => kind === "low_shelf" || kind === "high_shelf";
const isPass = (kind: EqBandKind) => kind === "low_pass" || kind === "high_pass";

interface EqBandRowProps {
  band: EqBand;
  /** Dot color on the curve — repeated here so row and dot read as one. */
  color: string;
  selected: boolean;
  onSelect: () => void;
  onChange: (patch: Partial<EqBand>) => void;
  onRemove: () => void;
}

/** Numeric editor for one band — the keyboard-accessible twin of the
 *  curve dot (drag isn't reachable for everyone). */
export function EqBandRow({ band, color, selected, onSelect, onChange, onRemove }: EqBandRowProps) {
  const clampNum = (v: string, lo: number, hi: number, fallback: number) => {
    const n = Number(v);
    return Number.isFinite(n) ? Math.max(lo, Math.min(hi, n)) : fallback;
  };

  return (
    <div className={"eqm-band" + (selected ? " sel" : "")} onPointerDown={onSelect}>
      <span className="eqm-chip" style={{ background: color }} aria-hidden="true" />
      <select
        className="eqm-kind"
        value={band.kind}
        aria-label="Band type"
        onChange={(e) => onChange({ kind: e.target.value as EqBandKind })}
      >
        {KINDS.map((k) => (
          <option key={k.value} value={k.value}>
            {k.label}
          </option>
        ))}
      </select>
      <label className="eqm-field">
        <span>Hz</span>
        <input
          type="number"
          min={EQ_FREQ_MIN_HZ}
          max={EQ_FREQ_MAX_HZ}
          step={1}
          value={Math.round(band.freq_hz)}
          onChange={(e) =>
            onChange({
              freq_hz: clampNum(e.target.value, EQ_FREQ_MIN_HZ, EQ_FREQ_MAX_HZ, band.freq_hz),
            })
          }
        />
      </label>
      <label className="eqm-field">
        <span>dB</span>
        <input
          type="number"
          min={-EQ_GAIN_RANGE_DB}
          max={EQ_GAIN_RANGE_DB}
          step={0.5}
          value={band.gain_db}
          disabled={isPass(band.kind)}
          title={isPass(band.kind) ? "Pass filters have no gain" : undefined}
          onChange={(e) =>
            onChange({
              gain_db: clampNum(e.target.value, -EQ_GAIN_RANGE_DB, EQ_GAIN_RANGE_DB, band.gain_db),
            })
          }
        />
      </label>
      <label className="eqm-field">
        {/* One schema field, two meanings (see EqBand.q). */}
        <span>{isShelf(band.kind) ? "Slope" : "Q"}</span>
        <input
          type="number"
          min={0.1}
          max={10}
          step={0.1}
          value={band.q}
          onChange={(e) => onChange({ q: clampNum(e.target.value, 0.1, 10, band.q) })}
        />
      </label>
      <button className="eqm-remove" title="Remove band" aria-label="Remove band" onClick={onRemove}>
        <Ms name="close" style={{ fontSize: 14 }} />
      </button>
    </div>
  );
}
