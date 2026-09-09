import { describe, expect, it } from "vitest";
import { fitBoard } from "./layout";

describe("fitBoard", () => {
  it("takes its scale from the width and fills the height with the fader", () => {
    // A 900px board in a 1800x1000 window: 2x, strip 1000/2 - 60 = 440 -> floored to 460.
    expect(fitBoard(1800, 1000, 900, 60)).toEqual({ scale: 1.92, stripHeight: 460 });
    // Plenty of height: the strip grows to its cap rather than the scale.
    expect(fitBoard(1200, 1400, 900, 60)).toEqual({ scale: 1.33, stripHeight: 640 });
  });
  it("shrinks when channels are added and floors before it becomes unreadable", () => {
    expect(fitBoard(960, 600, 1100, 60).scale).toBe(0.87);
    expect(fitBoard(960, 600, 2000, 60).scale).toBe(0.8);
  });
  it("is height-limited when the window is short", () => {
    const fit = fitBoard(1800, 560, 900, 60);
    expect(fit.scale).toBe(1.07);
    expect(fit.stripHeight).toBe(463);
  });
  it("caps the scale and tolerates an unmeasured board", () => {
    expect(fitBoard(9000, 9000, 900, 60).scale).toBe(2);
    expect(fitBoard(1800, 1000, 0, 60)).toEqual({ scale: 1, stripHeight: 460 });
  });
});
