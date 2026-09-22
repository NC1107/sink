import { describe, expect, it } from "vitest";
import { micStatusLabel } from "./MicScreen";

// A pinned device that hasn't appeared yet must show as waiting even while
// muted.
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
