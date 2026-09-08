import { describe, expect, it } from "vitest";
import { fitScale } from "./layout";

describe("fitScale", () => {
  it("scales up to the tighter axis", () => {
    expect(fitScale(1800, 900, 900, 600)).toBe(1.5);
    expect(fitScale(1800, 800, 900, 600)).toBe(1.33);
    expect(fitScale(1000, 2000, 900, 600)).toBe(1.11);
  });
  it("never shrinks and never passes the cap", () => {
    expect(fitScale(800, 500, 900, 600)).toBe(1);
    expect(fitScale(9000, 9000, 900, 600)).toBe(1.5);
    expect(fitScale(9000, 9000, 900, 600, 1.2)).toBe(1.2);
  });
  it("tolerates an unmeasured board", () => {
    expect(fitScale(1800, 900, 0, 0)).toBe(1);
  });
});
