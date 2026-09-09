export interface Size {
  width: number;
  height: number;
}

/** First-launch height: the 620px strips plus the chrome around them. */
export const WINDOW_HEIGHT = 760;
/** A half tile on a 1366px laptop is 683px; four strips still work there. */
export const MIN_WINDOW_WIDTH = 640;

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
