export interface BoardFit {
  scale: number;
  /** Unscaled strip height that makes the scaled board fill the height. */
  stripHeight: number;
}

const MIN_STRIP = 460;
const MAX_STRIP = 640;
const MIN_SCALE = 0.5;
const MAX_SCALE = 1.25;

/** Fit the board to the window the way a console fills a screen: the
 * scale comes from the width, so the same layout appears at every size
 * and one more channel just makes everything a little smaller; the fader
 * throw then takes up the height. The floor stops a full board in a small
 * window turning into a thumbnail; past the cap a big window shows the
 * board at the cap with room to its right. `chromeH` is the board's
 * height minus a strip: group heads and padding. */
export function fitBoard(availW: number, availH: number, boardW: number, chromeH: number): BoardFit {
  if (boardW <= 0 || availW <= 0 || availH <= 0) return { scale: 1, stripHeight: MIN_STRIP };
  const raw = Math.min(availW / boardW, availH / (chromeH + MIN_STRIP), MAX_SCALE);
  const scale = Math.max(MIN_SCALE, Math.floor(raw * 100) / 100);
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

