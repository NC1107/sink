import { describe, expect, it } from "vitest";
import { fitBoard, windowSize } from "./layout";

describe("fitBoard", () => {
  it("takes its scale from the width and fills the height with the fader", () => {
    expect(fitBoard(1125, 900, 900, 60)).toEqual({ scale: 1.25, stripHeight: 640 });
    expect(fitBoard(1000, 700, 900, 60)).toEqual({ scale: 1.11, stripHeight: 570 });
  });
  it("shrinks when channels are added and floors before it becomes unreadable", () => {
    expect(fitBoard(960, 600, 1100, 60).scale).toBe(0.87);
    expect(fitBoard(842, 520, 1300, 60)).toEqual({ scale: 0.64, stripHeight: 752 });
    expect(fitBoard(960, 600, 2400, 60).scale).toBe(0.5);
  });
  it("is height-limited when the window is short", () => {
    const fit = fitBoard(1800, 560, 900, 60);
    expect(fit.scale).toBe(1.07);
    expect(fit.stripHeight).toBe(463);
  });
  it("caps the scale and tolerates an unmeasured board", () => {
    expect(fitBoard(9000, 9000, 900, 60).scale).toBe(1.25);
    expect(fitBoard(1800, 1000, 0, 60)).toEqual({ scale: 1, stripHeight: 460 });
  });
});

describe("window bounds", () => {
  const chrome = { width: 118, height: 80 };
  it("opens at 1x and clamps to the work area", () => {
    expect(windowSize(1326, 60, chrome, { width: 1920, height: 1050 })).toEqual({ width: 1444, height: 760 });
    expect(windowSize(2170, 60, chrome, { width: 1920, height: 1050 })).toEqual({ width: 1920, height: 760 });
  });
});
