/** Shared audio display math. */

/** PulseAudio-style dB readout for a volume percentage (100% = 0 dB). */
export function volToDb(v: number): string {
  if (v === 0) return "-∞";
  const db = 20 * Math.log10(v / 100);
  return (db >= 0 ? "+" : "") + db.toFixed(1) + " dB";
}

/** Map a linear peak amplitude (0-1) to a perceptual meter height. */
export function perceptual(amplitude: number): number {
  return Math.min(1, Math.sqrt(Math.max(0, amplitude)));
}
