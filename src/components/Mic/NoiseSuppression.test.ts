import { describe, expect, it } from "vitest";
import { modeTooltip, noiseSubtitle, pickAction } from "./NoiseSuppression";
import type { NoiseEngineStatus } from "../../types";

const status = (over: Partial<NoiseEngineStatus> = {}): NoiseEngineStatus => ({
  supported: true,
  installed: "downloaded",
  download_bytes: 52_711_472,
  version: "0.5.6",
  state: "running",
  ...over,
});

// The subtitle is the row's only status surface, so each state must say
// something distinct, and warnings must never show for the wrong mode.
describe("noiseSubtitle", () => {
  it("names the engine in use with a link to it", () => {
    expect(noiseSubtitle("strong", status(), null, null).link?.label).toBe("DeepFilterNet 0.5.6");
    expect(noiseSubtitle("light", status(), null, null).link?.label).toBe("RNNoise");
  });
  it("offers deleting the download only beside Strong", () => {
    expect(noiseSubtitle("strong", status(), null, null).deletable).toBe(true);
    expect(noiseSubtitle("light", status(), null, null).deletable).toBeFalsy();
    expect(noiseSubtitle("strong", status({ installed: "system" }), null, null).deletable).toBe(
      false,
    );
  });
  it("marks a system install", () => {
    const s = noiseSubtitle("strong", status({ installed: "system" }), null, null);
    expect(s.link?.after).toBe(" (system install)");
  });
  it("shows download progress", () => {
    const s = noiseSubtitle("light", null, [21_000_000, 52_711_472], null);
    expect(s.text).toBe("Downloading DeepFilterNet - 21 of 53 MB");
  });
  it("warns on a failed download with how to retry", () => {
    const s = noiseSubtitle("light", null, null, "download failed");
    expect(s.warn).toBe(true);
    expect(s.text).toContain("pick Strong to try again");
  });
  it("warns when the Strong engine stopped, only while Strong is picked", () => {
    expect(noiseSubtitle("strong", status({ state: "failed" }), null, null).warn).toBe(true);
    expect(noiseSubtitle("light", status({ state: "failed" }), null, null).warn).toBeFalsy();
  });
});

describe("modeTooltip", () => {
  it("gives rough per-core costs that depend on the machine", () => {
    expect(modeTooltip("light", status())).toContain("~0.3% of a core");
    expect(modeTooltip("strong", status())).toContain("~10% of a core and 150 MB of RAM");
    expect(modeTooltip("strong", status())).toContain("Depending on your specs");
  });
  it("mentions the download only until it's installed", () => {
    expect(modeTooltip("strong", status({ installed: null }))).toContain(
      "53 MB download on first use",
    );
    expect(modeTooltip("strong", status())).not.toContain("download");
  });
});

describe("pickAction", () => {
  it("sets Off and Light directly", () => {
    expect(pickAction("off", "strong", status())).toBe("set");
    expect(pickAction("light", "off", null)).toBe("set");
  });
  it("switches to an installed Strong without asking", () => {
    expect(pickAction("strong", "light", status())).toBe("set");
  });
  it("asks before downloading, including before the status has loaded", () => {
    expect(pickAction("strong", "light", status({ installed: null }))).toBe("confirm-download");
    expect(pickAction("strong", "light", null)).toBe("confirm-download");
  });
  it("retries a failed engine when Strong is picked again", () => {
    expect(pickAction("strong", "strong", status({ state: "failed" }))).toBe("retry");
    // From another mode a failed state is stale; just switch.
    expect(pickAction("strong", "light", status({ state: "failed" }))).toBe("set");
  });
});

describe("modeTooltip before the status loads", () => {
  it("never claims a 0 MB download", () => {
    expect(modeTooltip("strong", null)).not.toContain("MB download");
  });
});
