import { useCallback, useEffect, useRef } from "react";
import type { EqBand, EqConfig } from "../../types";
import { EQ_GAIN_RANGE_DB } from "../../types";
import { curvePoints, freqToX, xToFreq } from "../../lib/eqMath";

// SVG coordinate space; the element scales responsively.
const W = 600;
const H = 252;
const PAD = 8;
/** Region-label strip at the top and frequency-label strip at the bottom. */
const HEAD = 20;
const FOOT = 16;
const TOP = HEAD + 4;
const BOTTOM = H - FOOT - 4;

const dbToY = (db: number) =>
  TOP + ((EQ_GAIN_RANGE_DB - db) / (2 * EQ_GAIN_RANGE_DB)) * (BOTTOM - TOP);
const yToDb = (y: number) =>
  EQ_GAIN_RANGE_DB - ((y - TOP) / (BOTTOM - TOP)) * 2 * EQ_GAIN_RANGE_DB;
const fxToX = (fx: number) => PAD + fx * (W - 2 * PAD);
const xToFx = (x: number) => (x - PAD) / (W - 2 * PAD);

/** Sonar-style frequency regions across the top of the plot. */
const REGIONS: { label: string; to: number }[] = [
  { label: "SUB BASS", to: 60 },
  { label: "BASS", to: 250 },
  { label: "LOW MIDS", to: 500 },
  { label: "MID RANGE", to: 2000 },
  { label: "UPPER MIDS", to: 6000 },
  { label: "HIGHS", to: 20000 },
];

/** Frequencies that get a labeled vertical grid line. */
const GRID_FREQS = [20, 50, 100, 200, 500, 1000, 2000, 5000, 10000, 20000];
/** dB lines: labeled majors and unlabeled minors (plot edges are ±24). */
const GRID_DBS_MAJOR = [-12, 0, 12];
const GRID_DBS_MINOR = [-18, -6, 6, 18];

const fmtFreq = (hz: number) => (hz >= 1000 ? `${hz / 1000}kHz` : `${hz}Hz`);
const fmtDb = (db: number) => `${db > 0 ? "+" : ""}${db} dB`;

/** Per-band dot colors (index-keyed; mirrored as chips on the band rows). */
const BAND_COLORS = [
  "#a78bfa",
  "#6366f1",
  "#ec4899",
  "#ef4444",
  "#f97316",
  "#f59e0b",
  "#a3e635",
  "#22c55e",
  "#2dd4bf",
  "#38bdf8",
];

export const bandColor = (index: number) => BAND_COLORS[index % BAND_COLORS.length];

/** Bands without a gain axis: their dot rides the 0 dB line. */
const gainless = (band: EqBand) => band.kind === "low_pass" || band.kind === "high_pass";

interface EqCurveProps {
  config: EqConfig;
  selected: number;
  onSelect: (index: number) => void;
  onBandChange: (index: number, patch: Partial<EqBand>) => void;
}

/** The interactive response curve: drag a dot to set freq/gain, scroll on
 *  it to tighten/widen Q, double-click to zero the band. */
export function EqCurve({ config, selected, onSelect, onBandChange }: EqCurveProps) {
  const svgRef = useRef<SVGSVGElement>(null);
  const dragIndex = useRef<number>(-1);

  const dragTo = useCallback(
    (clientX: number, clientY: number) => {
      const svg = svgRef.current;
      const index = dragIndex.current;
      if (!svg || index < 0) return;
      const band = config.bands[index];
      if (!band) return;
      const r = svg.getBoundingClientRect();
      const x = ((clientX - r.left) / r.width) * W;
      const y = ((clientY - r.top) / r.height) * H;
      const freq_hz = Math.round(xToFreq(xToFx(x)));
      const patch: Partial<EqBand> = { freq_hz };
      if (!gainless(band)) {
        const db = Math.max(-EQ_GAIN_RANGE_DB, Math.min(EQ_GAIN_RANGE_DB, yToDb(y)));
        patch.gain_db = Math.round(db * 10) / 10;
      }
      onBandChange(index, patch);
    },
    [config.bands, onBandChange],
  );

  // Window-level listeners attached once; latest handler via ref (Fader idiom).
  const dragToRef = useRef(dragTo);
  dragToRef.current = dragTo;

  useEffect(() => {
    const move = (e: PointerEvent) => {
      if (dragIndex.current >= 0) dragToRef.current(e.clientX, e.clientY);
    };
    const up = () => {
      dragIndex.current = -1;
    };
    window.addEventListener("pointermove", move);
    window.addEventListener("pointerup", up);
    return () => {
      window.removeEventListener("pointermove", move);
      window.removeEventListener("pointerup", up);
    };
  }, []);

  const points = curvePoints(config);
  const path = points
    .map(
      (p, i) =>
        `${i === 0 ? "M" : "L"}${fxToX(freqToX(p.freq)).toFixed(1)},${dbToY(
          Math.max(-EQ_GAIN_RANGE_DB, Math.min(EQ_GAIN_RANGE_DB, p.db)),
        ).toFixed(1)}`,
    )
    .join(" ");
  const zeroY = dbToY(0);
  const fill = `${path} L${fxToX(1).toFixed(1)},${zeroY} L${fxToX(0).toFixed(1)},${zeroY} Z`;

  // Region strip geometry (log axis).
  let regionFrom = 20;
  const regions = REGIONS.map(({ label, to }) => {
    const x0 = fxToX(freqToX(regionFrom));
    const x1 = fxToX(freqToX(to));
    regionFrom = to;
    return { label, x0, x1 };
  });

  return (
    <svg
      ref={svgRef}
      className={"eqm-curve" + (config.enabled ? "" : " off")}
      viewBox={`0 0 ${W} ${H}`}
      role="img"
      aria-label="EQ frequency response"
    >
      {/* frequency-region strip */}
      {regions.map(({ label, x0, x1 }, i) => (
        <g key={label}>
          <rect className="eqm-region" x={x0 + 1} y={PAD - 4} width={x1 - x0 - 2} height={HEAD - 4} rx={3} />
          <text className="eqm-region-label" x={(x0 + x1) / 2} y={PAD + HEAD / 2 + 1}>
            {label}
          </text>
          {i > 0 && (
            <line className="eqm-grid region" x1={x0} x2={x0} y1={TOP} y2={BOTTOM} />
          )}
        </g>
      ))}

      {/* vertical grid + frequency labels (with units, Sonar-style) */}
      {GRID_FREQS.map((f, i) => {
        const x = fxToX(freqToX(f));
        // Edge labels hug inward so they don't clip at the borders.
        const edge =
          i === 0 ? "start" : i === GRID_FREQS.length - 1 ? "end" : "middle";
        return (
          <g key={f}>
            <line className="eqm-grid" x1={x} x2={x} y1={TOP} y2={BOTTOM} />
            <text
              className="eqm-axis-label freq"
              x={edge === "start" ? x + 2 : edge === "end" ? x - 2 : x}
              y={H - 5}
              textAnchor={edge}
            >
              {fmtFreq(f)}
            </text>
          </g>
        );
      })}

      {/* horizontal grid + dB labels */}
      {GRID_DBS_MINOR.map((db) => (
        <line
          key={db}
          className="eqm-grid minor"
          x1={PAD}
          x2={W - PAD}
          y1={dbToY(db)}
          y2={dbToY(db)}
        />
      ))}
      {GRID_DBS_MAJOR.map((db) => (
        <g key={db}>
          <line
            className={"eqm-grid" + (db === 0 ? " zero" : "")}
            x1={PAD}
            x2={W - PAD}
            y1={dbToY(db)}
            y2={dbToY(db)}
          />
          <text className="eqm-axis-label" x={PAD + 3} y={dbToY(db) - 4}>
            {fmtDb(db)}
          </text>
        </g>
      ))}

      <path className="eqm-fill" d={fill} />
      <path className="eqm-line" d={path} />
      {config.bands.map((band, i) => (
        <circle
          key={i}
          className={"eqm-dot" + (i === selected ? " sel" : "")}
          style={{ fill: bandColor(i) }}
          cx={fxToX(freqToX(band.freq_hz))}
          cy={gainless(band) ? zeroY : dbToY(band.gain_db)}
          r={i === selected ? 8 : 6}
          onPointerDown={(e) => {
            e.preventDefault();
            onSelect(i);
            dragIndex.current = i;
          }}
          onDoubleClick={() => onBandChange(i, { gain_db: 0 })}
          onWheel={(e) => {
            // Scroll tightens/widens the band (Q, or slope on shelves).
            const dir = e.deltaY > 0 ? -1 : 1;
            const q = Math.max(0.1, Math.min(10, band.q * (dir > 0 ? 1.12 : 1 / 1.12)));
            onBandChange(i, { q: Math.round(q * 100) / 100 });
          }}
        >
          <title>{`${Math.round(band.freq_hz)} Hz, ${band.gain_db.toFixed(1)} dB — drag to move, scroll for width, double-click to zero`}</title>
        </circle>
      ))}
    </svg>
  );
}
