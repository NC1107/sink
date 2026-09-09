import { describe, expect, it } from "vitest";
import { fitBoard, snapScale, snappedWidth, windowSize } from "./layout";

describe("fitBoard", () => {
  it("takes its scale from the width, on a step, and fills the height with the fader", () => {
    expect(fitBoard(1125, 900, 900, 60)).toEqual({ scale: 1.25, stripHeight: 640 });
    expect(fitBoard(1000, 700, 900, 60)).toEqual({ scale: 1, stripHeight: 640 });
    expect(fitBoard(1050, 700, 900, 60)).toEqual({ scale: 1.125, stripHeight: 562 });
  });
  it("shrinks when channels are added and floors before it becomes unreadable", () => {
    expect(fitBoard(960, 600, 1100, 60).scale).toBe(0.75);
    expect(fitBoard(842, 520, 1300, 60)).toEqual({ scale: 0.625, stripHeight: 772 });
    expect(fitBoard(960, 600, 2400, 60).scale).toBe(0.5);
  });
  it("is height-limited when the window is short", () => {
    expect(fitBoard(1800, 480, 900, 60)).toEqual({ scale: 0.875, stripHeight: 488 });
  });
  it("caps the scale and tolerates an unmeasured board", () => {
    expect(fitBoard(9000, 9000, 900, 60).scale).toBe(1.25);
    expect(fitBoard(1800, 1000, 0, 60)).toEqual({ scale: 1, stripHeight: 460 });
  });
});

describe("scale steps", () => {
  it("never renders between steps", () => {
    expect(snapScale(1.11)).toBe(1);
    expect(snapScale(0.99)).toBe(0.875);
    expect(snapScale(1.25)).toBe(1.25);
    expect(snapScale(0.3)).toBe(0.5);
  });
  it("snaps a dragged width to the nearest step", () => {
    expect(snappedWidth(1050, 900, 118)).toBe(1018);
    expect(snappedWidth(1100, 900, 118)).toBe(1131);
    expect(snappedWidth(3000, 900, 118)).toBe(1243);
    expect(snappedWidth(400, 900, 118)).toBe(568);
  });
});

describe("window bounds", () => {
  const chrome = { width: 118, height: 80 };
  it("opens at 1x and clamps to the work area", () => {
    expect(windowSize(1326, 60, chrome, { width: 1920, height: 1050 })).toEqual({ width: 1444, height: 760 });
    expect(windowSize(2170, 60, chrome, { width: 1920, height: 1050 })).toEqual({ width: 1920, height: 760 });
    expect(windowSize(1326, 60, chrome, { width: 1920, height: 700 })).toEqual({ width: 1444, height: 700 });
  });
});
