import { act } from "react";
import { createRoot, type Root } from "react-dom/client";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

import { VolumeReadout } from "./VolumeReadout";
import { StripName } from "./StripName";

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

const key = (el: Element, k: string) =>
  act(() => {
    el.dispatchEvent(new KeyboardEvent("keydown", { key: k, bubbles: true }));
  });

const trigger = () => host.querySelector<HTMLElement>('[role="button"]');
const input = () => host.querySelector<HTMLInputElement>("input");

describe("VolumeReadout", () => {
  it("is in the tab order and opens for typing on Enter", () => {
    act(() => root.render(<VolumeReadout percent={72} onChange={() => {}} />));
    const t = trigger();
    expect(t?.tabIndex).toBe(0);
    expect(input()).toBeNull();
    key(t!, "Enter");
    expect(input()?.value).toBe("72");
  });

  it("commits a typed level and clamps it to the ceiling", () => {
    const onChange = vi.fn();
    act(() => root.render(<VolumeReadout percent={72} max={150} onChange={onChange} />));
    key(trigger()!, " ");
    const i = input()!;
    act(() => {
      // React listens for the native input event on the tracked value setter.
      Object.getOwnPropertyDescriptor(HTMLInputElement.prototype, "value")!.set!.call(i, "999");
      i.dispatchEvent(new Event("input", { bubbles: true }));
    });
    key(i, "Enter");
    expect(onChange).toHaveBeenCalledWith(150);
    expect(input()).toBeNull();
  });

  it("Escape abandons the edit without a change", () => {
    const onChange = vi.fn();
    act(() => root.render(<VolumeReadout percent={40} onChange={onChange} />));
    key(trigger()!, "Enter");
    key(input()!, "Escape");
    expect(onChange).not.toHaveBeenCalled();
    expect(input()).toBeNull();
  });
});

describe("StripName", () => {
  it("opens for renaming on Enter and commits a changed name", () => {
    const onRename = vi.fn();
    act(() => root.render(<StripName label="Game" onRename={onRename} />));
    key(trigger()!, "Enter");
    const i = input()!;
    expect(i.value).toBe("Game");
    act(() => {
      Object.getOwnPropertyDescriptor(HTMLInputElement.prototype, "value")!.set!.call(i, " Games ");
      i.dispatchEvent(new Event("input", { bubbles: true }));
    });
    key(i, "Enter");
    expect(onRename).toHaveBeenCalledWith("Games");
  });

  it("does not rename on an unchanged or empty draft", () => {
    const onRename = vi.fn();
    act(() => root.render(<StripName label="Game" onRename={onRename} />));
    key(trigger()!, "Enter");
    key(input()!, "Enter");
    expect(onRename).not.toHaveBeenCalled();
  });
});
