import { describe, expect, it } from "vitest";
import { engineNotice } from "./NoiseSuppression";
import type { NoiseEngineStatus } from "../../types";

const status = (over: Partial<NoiseEngineStatus>): NoiseEngineStatus => ({
  supported: true,
  installed: null,
  download_bytes: 52_711_472,
  state: "idle",
  ...over,
});

// The download offer and the failure note must never show for the wrong
// mode, or a Light user would be nagged about Strong.
describe("engineNotice", () => {
  it("offers the download when the user asks for Strong", () => {
    expect(engineNotice("light", status({}), true)).toBe("offer");
  });
  it("offers it when Strong is selected but the plugin went missing", () => {
    expect(engineNotice("strong", status({ state: "missing" }), false)).toBe("offer");
  });
  it("reports a failed engine only while Strong is selected", () => {
    expect(engineNotice("strong", status({ state: "failed" }), false)).toBe("failed");
    expect(engineNotice("light", status({ state: "failed" }), false)).toBeNull();
  });
  it("stays quiet while Strong runs", () => {
    expect(engineNotice("strong", status({ state: "running" }), false)).toBeNull();
  });
  it("shows nothing before the status loads", () => {
    expect(engineNotice("strong", null, true)).toBeNull();
  });
});
