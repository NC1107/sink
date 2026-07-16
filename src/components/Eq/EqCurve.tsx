import { useCallback, useEffect, useRef } from "react";
import type { EqBand, EqConfig } from "../../types";
import { EQ_GAIN_RANGE_DB } from "../../types";
import { curvePoints, freqToX, xToFreq } from "../../lib/eqMath";

// SVG coordinate space; the element scales responsively.
const W = 600;
const H = 220;
const PAD = 8;

const dbToY = (db: number) =>
  PAD + ((EQ_GAIN_RANGE_DB - db) / (2 * EQ_GAIN_RANGE_DB)) * (H - 2 * PAD);
const yToDb = (y: number) =>
  EQ_GAIN_RANGE_DB - ((y - PAD) / (H - 2 * PAD)) * 2 * EQ_GAIN_RANGE_DB;
const fxToX = (fx: number) => PAD + fx * (W - 2 * PAD);
const xToFx = (x: number) => (x - PAD) / (W - 2 * PAD);

/** Frequencies that get a labeled grid line. */
const GRID_FREQS = [50, 100, 500, 1000, 5000, 10000];
const GRID_DBS = [-12, 0, 12];

const fmtFreq = (hz: number) => (hz >= 1000 ? `${hz / 1000}k` : `${hz}`);

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

  return (
    <svg
      ref={svgRef}
      className={"eqm-curve" + (config.enabled ? "" : " off")}
      viewBox={`0 0 ${W} ${H}`}
      role="img"
      aria-label="EQ frequency response"
    >
      {GRID_FREQS.map((f) => (
        <g key={f}>
          <line
            className="eqm-grid"
            x1={fxToX(freqToX(f))}
            x2={fxToX(freqToX(f))}
            y1={PAD}
            y2={H - PAD}
          />
          <text className="eqm-grid-label" x={fxToX(freqToX(f)) + 3} y={H - PAD - 4}>
            {fmtFreq(f)}
          </text>
        </g>
      ))}
      {GRID_DBS.map((db) => (
        <g key={db}>
          <line
            className={"eqm-grid" + (db === 0 ? " zero" : "")}
            x1={PAD}
            x2={W - PAD}
            y1={dbToY(db)}
            y2={dbToY(db)}
          />
          <text className="eqm-grid-label" x={PAD + 2} y={dbToY(db) - 3}>
            {db > 0 ? `+${db}` : db}
          </text>
        </g>
      ))}
      <path className="eqm-fill" d={fill} />
      <path className="eqm-line" d={path} />
      {config.bands.map((band, i) => (
        <circle
          key={i}
          className={"eqm-dot" + (i === selected ? " sel" : "")}
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
