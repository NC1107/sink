import { describe, expect, it } from "vitest";
import { perceptual, volToDb } from "./audio";

describe("volToDb", () => {
  it("maps 100% to 0 dB and 0% to -∞", () => {
    expect(volToDb(100)).toBe("+0.0 dB");
    expect(volToDb(0)).toBe("-∞");
  });

  it("halving volume is roughly -6 dB; boosting is positive", () => {
    expect(volToDb(50)).toBe("-6.0 dB");
    expect(volToDb(150)).toBe("+3.5 dB");
  });
});

describe("perceptual", () => {
  it("clamps to the 0-1 meter range", () => {
    expect(perceptual(0)).toBe(0);
    expect(perceptual(1)).toBe(1);
    expect(perceptual(4)).toBe(1); // over-unity peaks don't overflow the bar
    expect(perceptual(-0.5)).toBe(0); // garbage in, silence out
  });

  it("lifts quiet signals (sqrt curve)", () => {
    expect(perceptual(0.25)).toBeCloseTo(0.5);
    expect(perceptual(0.01)).toBeCloseTo(0.1);
  });
});
