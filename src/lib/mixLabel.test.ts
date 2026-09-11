import { describe, expect, it } from "vitest";
import { memberLabel } from "../components/MixerBoard/StreamMixStrip";

describe("memberLabel", () => {
  it("counts what the mix carries, not what it leaves out", () => {
    // The old wording said "all but 3" for a mix carrying one channel.
    expect(memberLabel(1, 4)).toBe("1 channel");
    expect(memberLabel(2, 4)).toBe("2 channels");
  });

  it("says so when it carries everything", () => {
    expect(memberLabel(4, 4)).toBe("all channels");
  });

  it("handles a mixer with no channels at all", () => {
    expect(memberLabel(0, 0)).toBe("0 channels");
  });
});
