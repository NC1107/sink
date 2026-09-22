import { describe, expect, it } from "vitest";
import { micStatusLabel } from "./MicScreen";

// enabled, muted, waiting -> label. Waiting is the #73 case: a pinned device
// that has not appeared yet, and it must show even while muted.
describe("micStatusLabel", () => {
  it.each([
    [false, false, false, "Off"],
    [false, true, true, "Off"],
    [true, false, true, "Waiting"],
    [true, true, true, "Waiting"],
    [true, true, false, "Muted"],
    [true, false, false, "Live"],
  ])("enabled=%s muted=%s waiting=%s -> %s", (enabled, muted, waiting, label) => {
    expect(micStatusLabel(enabled, muted, waiting)).toBe(label);
  });
});
