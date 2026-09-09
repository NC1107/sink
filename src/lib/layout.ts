export interface BoardFit {
  scale: number;
  /** Unscaled strip height that makes the scaled board fill the height. */
  stripHeight: number;
}

const MIN_STRIP = 460;
const MAX_STRIP = 640;
const MIN_SCALE = 0.5;
const MAX_SCALE = 2;

/** Fit the board to the window the way a console fills a screen: the
 * scale comes from the width, so the same layout appears at every size
 * and one more channel just makes everything a little smaller; the fader
 * throw then takes up the height. The floor only stops a full board in
 * the smallest window turning into a thumbnail; the cap keeps a huge
 * window from becoming a toy. `chromeH` is the board's height minus a
 * strip: group heads and padding, which don't stretch. */
export function fitBoard(availW: number, availH: number, boardW: number, chromeH: number): BoardFit {
  if (boardW <= 0 || availW <= 0 || availH <= 0) return { scale: 1, stripHeight: MIN_STRIP };
  const raw = Math.min(availW / boardW, availH / (chromeH + MIN_STRIP), MAX_SCALE);
  const scale = Math.max(MIN_SCALE, Math.floor(raw * 100) / 100);
  // The cap is an on-screen length: a scaled-down board may use taller strips.
  const maxStrip = Math.max(MAX_STRIP, MAX_STRIP / scale);
  const stripHeight = Math.floor(Math.min(maxStrip, Math.max(MIN_STRIP, availH / scale - chromeH)));
  return { scale, stripHeight };
}
