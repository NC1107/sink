import { describe, expect, it } from "vitest";
import { acceleratorFrom } from "./HotkeysSection";

describe("acceleratorFrom", () => {
  it("spells modifiers and the key code the X11 grabber understands", () => {
    expect(acceleratorFrom({ code: "ArrowLeft", ctrlKey: true, altKey: true, shiftKey: false, metaKey: false })).toBe(
      "Ctrl+Alt+ArrowLeft",
    );
    expect(acceleratorFrom({ code: "KeyP", ctrlKey: false, altKey: false, shiftKey: true, metaKey: true })).toBe(
      "Shift+Super+KeyP",
    );
  });
  it("refuses a bare key", () => {
    expect(acceleratorFrom({ code: "KeyA", ctrlKey: false, altKey: false, shiftKey: false, metaKey: false })).toBeNull();
  });
  it("waits for a real key when only a modifier is down", () => {
    expect(acceleratorFrom({ code: "ControlLeft", ctrlKey: true, altKey: false, shiftKey: false, metaKey: false })).toBeNull();
    expect(acceleratorFrom({ code: "", ctrlKey: true, altKey: false, shiftKey: false, metaKey: false })).toBeNull();
  });
});
