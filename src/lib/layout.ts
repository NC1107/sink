export interface BoardFit {
  scale: number;
  /** Unscaled strip height that makes the scaled board fill the height. */
  stripHeight: number;
}

const MIN_STRIP = 460;
const MAX_STRIP = 640;
const MIN_SCALE = 0.5;
const MAX_SCALE = 1;

/** The window is sized to show the board at 1x, so scaling only happens
 * when the screen is too small for it: the scale comes from the width and
 * the fader throw takes up the height. The floor stops a full board on a
 * small screen turning into a thumbnail. `chromeH` is the board's height
 * minus a strip: group heads and padding, which don't stretch. */
export interface Size {
  width: number;
  height: number;
}

/** Height every board gets; the fader throw fills it. */
const WINDOW_HEIGHT = 760;

/** The window that shows a `boardW`-wide board at 1x, `chromeW` being the
 * rail, padding and border around it, clamped to the monitor's work area. */
export function windowSize(boardW: number, chromeW: number, work: Size): Size {
  return {
    width: Math.min(Math.ceil(boardW + chromeW), Math.floor(work.width)),
    height: Math.min(WINDOW_HEIGHT, Math.floor(work.height)),
  };
}

export function fitBoard(availW: number, availH: number, boardW: number, chromeH: number): BoardFit {
  if (boardW <= 0 || availW <= 0 || availH <= 0) return { scale: 1, stripHeight: MIN_STRIP };
  const raw = Math.min(availW / boardW, availH / (chromeH + MIN_STRIP), MAX_SCALE);
  const scale = Math.max(MIN_SCALE, Math.floor(raw * 100) / 100);
  // The cap is an on-screen length: a scaled-down board may use taller strips.
  const maxStrip = Math.max(MAX_STRIP, MAX_STRIP / scale);
  const stripHeight = Math.floor(Math.min(maxStrip, Math.max(MIN_STRIP, availH / scale - chromeH)));
  return { scale, stripHeight };
}
