import { describe, expect, it } from "vitest";
import { nextWidth, stripHeight, windowSize } from "./layout";

describe("stripHeight", () => {
  it("fills the height within the fader's bounds", () => {
    expect(stripHeight(680, 60)).toBe(620);
    expect(stripHeight(1400, 60)).toBe(640);
    expect(stripHeight(400, 60)).toBe(460);
  });
});

describe("windowSize", () => {
  it("wraps the board at 1x and clamps to the work area", () => {
    expect(windowSize(1326, 118, { width: 1920, height: 1050 })).toEqual({ width: 1444, height: 760 });
    expect(windowSize(2170, 118, { width: 1920, height: 1050 })).toEqual({ width: 1920, height: 760 });
    expect(windowSize(900, 118, { width: 1366, height: 728 })).toEqual({ width: 1018, height: 728 });
  });
});

describe("nextWidth", () => {
  it("follows the board when the window showed all of it", () => {
    expect(nextWidth(1444, 1588, 1444)).toBe(1588);
    expect(nextWidth(1588, 1444, 1588)).toBe(1444);
    expect(nextWidth(1444, 1444, 1444)).toBeNull();
  });
  it("leaves a window the user narrowed alone", () => {
    expect(nextWidth(1000, 1588, 1444)).toBeNull();
    expect(nextWidth(1000, 1300, 1444)).toBeNull();
  });
  it("sizes a window with no history to the board", () => {
    expect(nextWidth(1280, 1444, null)).toBe(1444);
    expect(nextWidth(1000, 1444, null)).toBe(1444);
  });
});
