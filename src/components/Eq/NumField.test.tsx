import { act } from "react";
import { createRoot, type Root } from "react-dom/client";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

import { NumField } from "./NumField";

let host: HTMLDivElement;
let root: Root;

beforeEach(() => {
  host = document.createElement("div");
  document.body.appendChild(host);
  root = createRoot(host);
});

afterEach(() => {
  act(() => root.unmount());
  host.remove();
});

const input = () => host.querySelector<HTMLInputElement>("input")!;

/** Type the field's whole text, the way each keystroke reaches React. */
const typeText = (text: string) =>
  act(() => {
    const i = input();
    Object.getOwnPropertyDescriptor(HTMLInputElement.prototype, "value")!.set!.call(i, text);
    i.dispatchEvent(new Event("input", { bubbles: true }));
  });

const key = (k: string) =>
  act(() => {
    input().dispatchEvent(new KeyboardEvent("keydown", { key: k, bubbles: true }));
  });

const blur = () =>
  act(() => {
    input().dispatchEvent(new FocusEvent("focusout", { bubbles: true }));
  });

/** A field wired like the EQ's: the parent holds the value. */
function render(initial: number, min: number, max: number, step = 1) {
  const onChange = vi.fn();
  let value = initial;
  const draw = () =>
    act(() =>
      root.render(
        <NumField
          value={value}
          min={min}
          max={max}
          step={step}
          aria-label="field"
          onChange={(n) => {
            onChange(n);
            value = n;
            draw();
          }}
        />,
      ),
    );
  draw();
  return { onChange, current: () => value };
}

describe("NumField", () => {
  it("lets you backspace past the minimum and type a new number", () => {
    // The bug: 105 -> backspace -> "10" snapped to 20, so typing 80 gave 2080.
    const f = render(105, 20, 20000);
    for (const text of ["10", "1", ""]) {
      typeText(text);
      expect(input().value).toBe(text);
    }
    typeText("8");
    typeText("80");
    blur();
    expect(f.current()).toBe(80);
    expect(input().value).toBe("80");
  });

  it("accepts a negative typed one character at a time", () => {
    const f = render(0, -24, 24, 0.5);
    typeText("");
    typeText("-");
    expect(input().value).toBe("-");
    typeText("-6");
    expect(f.current()).toBe(-6); // live once it reads as a number
    blur();
    expect(f.current()).toBe(-6);
  });

  it("only clamps when you finish", () => {
    const f = render(500, 20, 20000);
    typeText("5");
    expect(input().value).toBe("5"); // not snapped to 20 mid-typing
    expect(f.onChange).not.toHaveBeenCalled();
    key("Enter");
    expect(f.current()).toBe(20);
  });

  it("keeps the old value when the field is left empty or unparseable", () => {
    const f = render(1500, 20, 20000);
    typeText("");
    blur();
    expect(f.current()).toBe(1500);
    expect(input().value).toBe("1500");
    typeText("abc");
    blur();
    expect(f.current()).toBe(1500);
  });

  it("Escape restores the value from before the edit", () => {
    const f = render(1000, 20, 20000);
    typeText("300"); // applied live
    expect(f.current()).toBe(300);
    key("Escape");
    expect(f.current()).toBe(1000);
    expect(input().value).toBe("1000");
  });

  it("arrow keys step and stay in range", () => {
    const f = render(23.5, -24, 24, 0.5);
    key("ArrowUp");
    expect(f.current()).toBe(24);
    key("ArrowUp");
    expect(f.current()).toBe(24);
    key("ArrowDown");
    expect(f.current()).toBe(23.5);
  });
});
