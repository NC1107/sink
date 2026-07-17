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
  /** Zero-based position, used for the row's accessible name. */
  index: number;
  band: EqBand;
  /** Dot color on the curve - repeated here so row and dot read as one. */
  color: string;
  selected: boolean;
  /** False for the sole remaining band; the EQ always keeps at least one. */
  canRemove: boolean;
  onSelect: () => void;
  onChange: (patch: Partial<EqBand>) => void;
  onRemove: () => void;
}

/** Numeric editor for one band - the keyboard-accessible twin of the
 *  curve dot (drag isn't reachable for everyone). */
export function EqBandRow({ index, band, color, selected, canRemove, onSelect, onChange, onRemove }: Readonly<EqBandRowProps>) {
  const clampNum = (v: string, lo: number, hi: number, fallback: number) => {
    const n = Number(v);
    return Number.isFinite(n) ? Math.max(lo, Math.min(hi, n)) : fallback;
  };

  const widthLabel = isShelf(band.kind) ? "Slope" : "Q";

  return (
    <div
      className={"eqm-band" + (selected ? " sel" : "")}
      role="group"
      aria-label={`Band ${index + 1}`}
      onPointerDown={onSelect}
    >
      <span className="eqm-chip" style={{ background: color }} aria-hidden="true" />
      <select
        className="eqm-kind"
        value={band.kind}
        aria-label={`Band ${index + 1} type`}
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
          aria-label={`Band ${index + 1} frequency in hertz`}
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
          aria-label={`Band ${index + 1} gain in decibels`}
          onChange={(e) =>
            onChange({
              gain_db: clampNum(e.target.value, -EQ_GAIN_RANGE_DB, EQ_GAIN_RANGE_DB, band.gain_db),
            })
          }
        />
      </label>
      <label className="eqm-field">
        {/* One schema field, two meanings (see EqBand.q). */}
        <span>{widthLabel}</span>
        <input
          type="number"
          min={0.1}
          max={10}
          step={0.1}
          value={band.q}
          aria-label={`Band ${index + 1} ${widthLabel}`}
          onChange={(e) => onChange({ q: clampNum(e.target.value, 0.1, 10, band.q) })}
        />
      </label>
      <button
        className="eqm-remove"
        disabled={!canRemove}
        title={canRemove ? "Remove band" : "The EQ keeps at least one band"}
        aria-label={`Remove band ${index + 1}`}
        onClick={onRemove}
      >
        <Ms name="close" style={{ fontSize: 14 }} />
      </button>
    </div>
  );
}
