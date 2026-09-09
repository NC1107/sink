import { describe, expect, it } from "vitest";
import { fitBoard, windowSize } from "./layout";

describe("fitBoard", () => {
  it("never scales up and fills the height with the fader", () => {
    expect(fitBoard(1800, 1000, 900, 60)).toEqual({ scale: 1, stripHeight: 640 });
    expect(fitBoard(1000, 680, 900, 60)).toEqual({ scale: 1, stripHeight: 620 });
  });
  it("shrinks when channels are added and floors before it becomes unreadable", () => {
    expect(fitBoard(960, 600, 1100, 60).scale).toBe(0.87);
    expect(fitBoard(842, 520, 1300, 60)).toEqual({ scale: 0.64, stripHeight: 752 });
    expect(fitBoard(960, 600, 2400, 60).scale).toBe(0.5);
  });
  it("is height-limited when the window is short", () => {
    expect(fitBoard(1800, 480, 900, 60)).toEqual({ scale: 0.92, stripHeight: 461 });
  });
  it("tolerates an unmeasured board", () => {
    expect(fitBoard(1800, 1000, 0, 60)).toEqual({ scale: 1, stripHeight: 460 });
  });
});

describe("windowSize", () => {
  it("wraps the board at 1x and clamps to the work area", () => {
    expect(windowSize(1326, 118, { width: 1920, height: 1050 })).toEqual({ width: 1444, height: 760 });
    expect(windowSize(2170, 118, { width: 1920, height: 1050 })).toEqual({ width: 1920, height: 760 });
    expect(windowSize(900, 118, { width: 1366, height: 728 })).toEqual({ width: 1018, height: 728 });
  });
});
