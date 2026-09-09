export interface Size {
  width: number;
  height: number;
}

const MIN_STRIP = 460;
const MAX_STRIP = 640;
/** Height every board gets; the fader throw fills it. */
export const WINDOW_HEIGHT = 760;
/** Narrowest useful window: a strip and a half plus the rail. */
export const MIN_WINDOW_WIDTH = 480;

/** Fader length that fills the available height, within the bounds a
 * fader stays usable at. `chromeH` is the board's height minus a strip. */
export function stripHeight(availH: number, chromeH: number): number {
  return Math.floor(Math.min(MAX_STRIP, Math.max(MIN_STRIP, availH - chromeH)));
}

/** The window that shows a `boardW`-wide board at 1x, `chromeW` being the
 * rail, padding and border around it, clamped to the monitor's work area. */
export function windowSize(boardW: number, chromeW: number, work: Size): Size {
  return {
    width: Math.min(Math.ceil(boardW + chromeW), Math.floor(work.width)),
    height: Math.min(WINDOW_HEIGHT, Math.floor(work.height)),
  };
}

/** Width to give the window when the board's natural width changes. A
 * window the user narrowed to less than the previous board is theirs and
 * keeps its width (the board scrolls); otherwise it follows the board. */
export function nextWidth(current: number, natural: number, lastNatural: number | null): number | null {
  if (lastNatural !== null && current < lastNatural) return null;
  return current === natural ? null : natural;
}
