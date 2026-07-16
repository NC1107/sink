// Pins the TS side of the hand-synced RBJ math (the Rust twin lives in
// src-tauri/src/audio/pw_native/eq.rs with the same assertions).

import { describe, expect, it } from "vitest";
import { curvePoints, freqToX, magnitudeDb, xToFreq } from "./eqMath";
import type { EqBand } from "../types";
import { defaultEqConfig } from "../types";

const band = (kind: EqBand["kind"], freq_hz: number, gain_db: number, q: number): EqBand => ({
  kind,
  freq_hz,
  gain_db,
  q,
});

describe("eqMath", () => {
  it("peaking band matches its gain at the center frequency", () => {
    const db = magnitudeDb([band("peaking", 1000, 6, 1)], 0, 1000);
    expect(db).toBeCloseTo(6, 1);
  });

  it("peaking band is flat far from the center", () => {
    const db = magnitudeDb([band("peaking", 1000, 12, 2)], 0, 60);
    expect(Math.abs(db)).toBeLessThan(0.5);
  });

  it("low pass rolls off above the cutoff", () => {
    const bands = [band("low_pass", 1000, 0, 0.71)];
    expect(Math.abs(magnitudeDb(bands, 0, 100))).toBeLessThan(0.5);
    expect(magnitudeDb(bands, 0, 8000)).toBeLessThan(-30);
  });

  it("a flat config is 0 dB everywhere", () => {
    const config = defaultEqConfig();
    for (const { db } of curvePoints(config, 32)) {
      expect(Math.abs(db)).toBeLessThan(0.01);
    }
  });

  it("preamp shifts the whole curve uniformly", () => {
    const config = { ...defaultEqConfig(), preamp_db: -6 };
    for (const { db } of curvePoints(config, 16)) {
      expect(db).toBeCloseTo(-6, 3);
    }
  });

  it("freqToX and xToFreq are inverses on the log axis", () => {
    for (const freq of [20, 100, 1000, 10000, 20000]) {
      expect(xToFreq(freqToX(freq))).toBeCloseTo(freq, 0);
    }
    expect(freqToX(20)).toBe(0);
    expect(freqToX(20000)).toBe(1);
  });
});
