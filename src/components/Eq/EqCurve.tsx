import { useCallback, useEffect, useRef } from "react";
import type { EqBand, EqConfig } from "../../types";
import { EQ_GAIN_RANGE_DB } from "../../types";
import { curvePoints, freqToX, xToFreq } from "../../lib/eqMath";

// SVG coordinate space; the element scales responsively. All labels live
// in gutters OUTSIDE the plot rectangle: regions above, dB left, Hz below.
const W = 600;
const H = 256;
const LEFT = 46;
const RIGHT = 8;
const HEAD = 26;
const FOOT = 18;
const TOP = HEAD + 4;
const BOTTOM = H - FOOT - 4;

const dbToY = (db: number) =>
  TOP + ((EQ_GAIN_RANGE_DB - db) / (2 * EQ_GAIN_RANGE_DB)) * (BOTTOM - TOP);
const yToDb = (y: number) =>
  EQ_GAIN_RANGE_DB - ((y - TOP) / (BOTTOM - TOP)) * 2 * EQ_GAIN_RANGE_DB;
const fxToX = (fx: number) => LEFT + fx * (W - LEFT - RIGHT);
const xToFx = (x: number) => (x - LEFT) / (W - LEFT - RIGHT);

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

/** Edge frequency labels hug inward so they don't clip at the plot borders. */
function edgeAnchor(i: number, n: number): "start" | "end" | "middle" {
  if (i === 0) return "start";
  if (i === n - 1) return "end";
  return "middle";
}

interface EqCurveProps {
  config: EqConfig;
  selected: number;
  onSelect: (index: number) => void;
  onBandChange: (index: number, patch: Partial<EqBand>) => void;
}

/** The interactive response curve: drag a dot to set freq/gain, scroll on
 *  it to tighten/widen Q, double-click to zero the band. */
export function EqCurve({ config, selected, onSelect, onBandChange }: Readonly<EqCurveProps>) {
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
      {/* the plot area itself; everything textual sits outside it */}
      <rect
        className="eqm-plot"
        x={LEFT}
        y={TOP}
        width={W - LEFT - RIGHT}
        height={BOTTOM - TOP}
      />

      {/* frequency-region strip (above the plot). The gapped pills already
          delineate regions, so no full-height dividers clutter the plot. */}
      {regions.map(({ label, x0, x1 }) => (
        <g key={label}>
          <rect className="eqm-region" x={x0 + 1} y={2} width={x1 - x0 - 2} height={HEAD - 4} rx={3} />
          <text className="eqm-region-label" x={(x0 + x1) / 2} y={2 + (HEAD - 4) / 2 + 1}>
            {label}
          </text>
        </g>
      ))}

      {/* vertical grid + frequency labels (below the plot) */}
      {GRID_FREQS.map((f, i) => {
        const x = fxToX(freqToX(f));
        const edge = edgeAnchor(i, GRID_FREQS.length);
        let labelX = x;
        if (edge === "start") labelX = x - 4;
        else if (edge === "end") labelX = x + 4;
        return (
          <g key={f}>
            <line className="eqm-grid" x1={x} x2={x} y1={TOP} y2={BOTTOM} />
            <text
              className="eqm-axis-label freq"
              x={labelX}
              y={H - 5}
              style={{ textAnchor: edge }}
            >
              {fmtFreq(f)}
            </text>
          </g>
        );
      })}

      {/* horizontal grid + dB labels (left of the plot) */}
      {GRID_DBS_MINOR.map((db) => (
        <line
          key={db}
          className="eqm-grid minor"
          x1={LEFT}
          x2={W - RIGHT}
          y1={dbToY(db)}
          y2={dbToY(db)}
        />
      ))}
      {GRID_DBS_MAJOR.map((db) => (
        <g key={db}>
          <line
            className={"eqm-grid" + (db === 0 ? " zero" : "")}
            x1={LEFT}
            x2={W - RIGHT}
            y1={dbToY(db)}
            y2={dbToY(db)}
          />
          <text className="eqm-axis-label db" x={LEFT - 6} y={dbToY(db)}>
            {fmtDb(db)}
          </text>
        </g>
      ))}

      <path className="eqm-fill" d={fill} />
      <path className="eqm-line" d={path} />
      {config.bands.map((band, i) => {
        const cx = fxToX(freqToX(band.freq_hz));
        const cy = gainless(band) ? zeroY : dbToY(band.gain_db);
        return (
          <g key={i}>
            {/* Oversized transparent target: the visible dot is small but
                carries three gestures, so widen where the pointer lands. */}
            <circle
              className="eqm-hit"
              cx={cx}
              cy={cy}
              r={15}
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
              <title>{`${Math.round(band.freq_hz)} Hz, ${band.gain_db.toFixed(1)} dB - drag to move, scroll for width, double-click to zero`}</title>
            </circle>
            <circle
              className={"eqm-dot" + (i === selected ? " sel" : "")}
              style={{ fill: bandColor(i) }}
              cx={cx}
              cy={cy}
              r={i === selected ? 8 : 6}
            />
          </g>
        );
      })}
    </svg>
  );
}
