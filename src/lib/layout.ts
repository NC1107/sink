export interface BoardFit {
  scale: number;
  /** Unscaled strip height that makes the scaled board fill the height. */
  stripHeight: number;
}

const MIN_STRIP = 460;
const MAX_STRIP = 640;
/** The scales the board renders at. Anything in between blurs 1px lines
 * and text, so the board picks the nearest step below what fits and the
 * window snaps to that step when a resize ends. */
export const SCALE_STEPS = [0.5, 0.625, 0.75, 0.875, 1, 1.125, 1.25] as const;
const MIN_SCALE = SCALE_STEPS[0];
const MAX_SCALE = SCALE_STEPS[SCALE_STEPS.length - 1];

/** Largest step that still fits `raw`, never below the smallest. */
export function snapScale(raw: number): number {
  let best: number = MIN_SCALE;
  for (const step of SCALE_STEPS) if (step <= raw + 1e-9) best = step;
  return best;
}

/** Window width that shows a `boardW`-wide board at the step nearest to
 * the current width, `chromeW` being everything around the board. */
export function snappedWidth(current: number, boardW: number, chromeW: number): number {
  const raw = (current - chromeW) / boardW;
  let best: number = MIN_SCALE;
  for (const step of SCALE_STEPS) if (Math.abs(step - raw) < Math.abs(best - raw)) best = step;
  return Math.round(boardW * best + chromeW);
}

/** Fit the board to the window the way a console fills a screen: the
 * scale comes from the width, so the same layout appears at every size
 * and one more channel just makes everything a little smaller; the fader
 * throw then takes up the height. The floor stops a full board in a small
 * window turning into a thumbnail; past the cap a big window shows the
 * board at the cap with room to its right. `chromeH` is the board's
 * height minus a strip: group heads and padding. */
export function fitBoard(
  availW: number,
  availH: number,
  boardW: number,
  chromeH: number,
): BoardFit {
  if (boardW <= 0 || availW <= 0 || availH <= 0) return { scale: 1, stripHeight: MIN_STRIP };
  const scale = snapScale(Math.min(availW / boardW, availH / (chromeH + MIN_STRIP), MAX_SCALE));
  // The cap is an on-screen length: a scaled-down board may use taller strips.
  const maxStrip = Math.max(MAX_STRIP, MAX_STRIP / scale);
  const stripHeight = Math.floor(Math.min(maxStrip, Math.max(MIN_STRIP, availH / scale - chromeH)));
  return { scale, stripHeight };
}

interface Size {
  width: number;
  height: number;
}

/** The window that shows a `boardW`-wide board at 1x with 620px faders,
 * `chrome` being the rail, padding, title bar and border around it,
 * clamped to the monitor's work area. */
export function windowSize(boardW: number, chromeH: number, chrome: Size, work: Size): Size {
  return {
    width: Math.min(Math.ceil(boardW + chrome.width), Math.floor(work.width)),
    height: Math.min(Math.ceil(chromeH + 620 + chrome.height), Math.floor(work.height)),
  };
}
