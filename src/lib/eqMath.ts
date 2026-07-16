// Frequency-response math for the EQ curve — a hand-synced mirror of the
// RBJ designs in src-tauri/src/audio/pw_native/eq.rs. If a formula changes
// on either side, change both; the parallel test suites pin the contract.

import type { EqBand, EqBandKind, EqConfig } from "../types";
import { EQ_FREQ_MAX_HZ, EQ_FREQ_MIN_HZ } from "../types";

export interface Biquad {
  b0: number;
  b1: number;
  b2: number;
  a1: number;
  a2: number;
}

/** RBJ Audio EQ Cookbook design (see eq.rs for the shared conventions:
 *  shelves use q as slope S, low/high pass ignore gain). */
export function biquadCoeffs(
  kind: EqBandKind,
  freqHz: number,
  gainDb: number,
  q: number,
  sampleRate = 48000,
): Biquad {
  const freq = Math.min(Math.max(freqHz, 1), sampleRate * 0.49);
  const safeQ = Math.max(q, 0.01);
  const w0 = (2 * Math.PI * freq) / sampleRate;
  const sinW0 = Math.sin(w0);
  const cosW0 = Math.cos(w0);
  const a = Math.pow(10, gainDb / 40);

  let b0: number, b1: number, b2: number, a0: number, a1: number, a2: number;
  switch (kind) {
    case "peaking": {
      const alpha = sinW0 / (2 * safeQ);
      b0 = 1 + alpha * a;
      b1 = -2 * cosW0;
      b2 = 1 - alpha * a;
      a0 = 1 + alpha / a;
      a1 = -2 * cosW0;
      a2 = 1 - alpha / a;
      break;
    }
    case "low_shelf":
    case "high_shelf": {
      const s = safeQ;
      const alpha =
        (sinW0 / 2) * Math.sqrt(Math.max((a + 1 / a) * (1 / s - 1) + 2, 0));
      const twoSqrtAAlpha = 2 * Math.sqrt(a) * alpha;
      const ap1 = a + 1;
      const am1 = a - 1;
      if (kind === "low_shelf") {
        b0 = a * (ap1 - am1 * cosW0 + twoSqrtAAlpha);
        b1 = 2 * a * (am1 - ap1 * cosW0);
        b2 = a * (ap1 - am1 * cosW0 - twoSqrtAAlpha);
        a0 = ap1 + am1 * cosW0 + twoSqrtAAlpha;
        a1 = -2 * (am1 + ap1 * cosW0);
        a2 = ap1 + am1 * cosW0 - twoSqrtAAlpha;
      } else {
        b0 = a * (ap1 + am1 * cosW0 + twoSqrtAAlpha);
        b1 = -2 * a * (am1 + ap1 * cosW0);
        b2 = a * (ap1 + am1 * cosW0 - twoSqrtAAlpha);
        a0 = ap1 - am1 * cosW0 + twoSqrtAAlpha;
        a1 = 2 * (am1 - ap1 * cosW0);
        a2 = ap1 - am1 * cosW0 - twoSqrtAAlpha;
      }
      break;
    }
    case "low_pass": {
      const alpha = sinW0 / (2 * safeQ);
      const oneMinusCos = 1 - cosW0;
      b0 = oneMinusCos / 2;
      b1 = oneMinusCos;
      b2 = oneMinusCos / 2;
      a0 = 1 + alpha;
      a1 = -2 * cosW0;
      a2 = 1 - alpha;
      break;
    }
    case "high_pass": {
      const alpha = sinW0 / (2 * safeQ);
      const onePlusCos = 1 + cosW0;
      b0 = onePlusCos / 2;
      b1 = -onePlusCos;
      b2 = onePlusCos / 2;
      a0 = 1 + alpha;
      a1 = -2 * cosW0;
      a2 = 1 - alpha;
      break;
    }
  }
  return { b0: b0 / a0, b1: b1 / a0, b2: b2 / a0, a1: a1 / a0, a2: a2 / a0 };
}

/** |H(e^jw)| of one biquad at `freqHz`, in dB. */
function biquadMagnitudeDb(c: Biquad, freqHz: number, sampleRate: number): number {
  const w = (2 * Math.PI * freqHz) / sampleRate;
  const numRe = c.b0 + c.b1 * Math.cos(w) + c.b2 * Math.cos(2 * w);
  const numIm = -(c.b1 * Math.sin(w) + c.b2 * Math.sin(2 * w));
  const denRe = 1 + c.a1 * Math.cos(w) + c.a2 * Math.cos(2 * w);
  const denIm = -(c.a1 * Math.sin(w) + c.a2 * Math.sin(2 * w));
  const mag = Math.sqrt(
    (numRe * numRe + numIm * numIm) / (denRe * denRe + denIm * denIm),
  );
  return 20 * Math.log10(mag);
}

/** Combined response of the whole config (preamp + band cascade) at `freqHz`. */
export function magnitudeDb(
  bands: EqBand[],
  preampDb: number,
  freqHz: number,
  sampleRate = 48000,
): number {
  let db = preampDb;
  for (const band of bands) {
    const c = biquadCoeffs(band.kind, band.freq_hz, band.gain_db, band.q, sampleRate);
    db += biquadMagnitudeDb(c, freqHz, sampleRate);
  }
  return db;
}

/** Log-spaced response samples across the audible range, for the SVG path. */
export function curvePoints(
  config: EqConfig,
  steps = 128,
  sampleRate = 48000,
): { freq: number; db: number }[] {
  const logMin = Math.log10(EQ_FREQ_MIN_HZ);
  const logMax = Math.log10(EQ_FREQ_MAX_HZ);
  const points: { freq: number; db: number }[] = [];
  for (let i = 0; i <= steps; i++) {
    const freq = Math.pow(10, logMin + ((logMax - logMin) * i) / steps);
    points.push({
      freq,
      db: magnitudeDb(config.bands, config.preamp_db, freq, sampleRate),
    });
  }
  return points;
}

/** X position of a frequency on the log axis, as 0..1. */
export function freqToX(freqHz: number): number {
  const logMin = Math.log10(EQ_FREQ_MIN_HZ);
  const logMax = Math.log10(EQ_FREQ_MAX_HZ);
  const clamped = Math.min(Math.max(freqHz, EQ_FREQ_MIN_HZ), EQ_FREQ_MAX_HZ);
  return (Math.log10(clamped) - logMin) / (logMax - logMin);
}

/** Inverse of freqToX. */
export function xToFreq(x: number): number {
  const logMin = Math.log10(EQ_FREQ_MIN_HZ);
  const logMax = Math.log10(EQ_FREQ_MAX_HZ);
  const t = Math.min(Math.max(x, 0), 1);
  return Math.pow(10, logMin + (logMax - logMin) * t);
}
