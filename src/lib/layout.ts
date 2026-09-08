/** Largest uniform scale that fits a `w`x`h` board into the available
 * area, never shrinking below 1 and never past `max` (a console blown up
 * past that reads as a toy). Two decimals keep the transform stable. */
export function fitScale(availW: number, availH: number, w: number, h: number, max = 1.5): number {
  if (w <= 0 || h <= 0) return 1;
  const raw = Math.min(availW / w, availH / h, max);
  return Math.max(1, Math.floor(raw * 100) / 100);
}
