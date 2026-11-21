import { act } from "react";
import { createRoot, type Root } from "react-dom/client";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

import { useMixerStore } from "../../store/mixer";
import { OutputSelect, summarizeOutputs } from "./OutputSelect";

const HEADSET = { index: 1, name: "alsa_output.usb-Headset", description: "Arctis Nova 5" };
const SPEAKERS = { index: 2, name: "alsa_output.pci-Speakers", description: "Speakers Analog" };
const TV = { index: 3, name: "alsa_output.hdmi", description: "HDMI Display" };
const DEVICES = [HEADSET, SPEAKERS, TV];

let host: HTMLDivElement;
let root: Root;

beforeEach(() => {
  host = document.createElement("div");
  document.body.appendChild(host);
  root = createRoot(host);
  useMixerStore.setState({ outputDevices: DEVICES });
});

afterEach(() => {
  act(() => root.unmount());
  host.remove();
});

const click = (el: Element | null | undefined) =>
  act(() => {
    el!.dispatchEvent(new MouseEvent("click", { bubbles: true }));
  });

/** Opens the picker for a channel routed to `value`; returns the change spy. */
function open(value: string[]) {
  const onChange = vi.fn();
  act(() => root.render(<OutputSelect channelLabel="Music" value={value} onChange={onChange} />));
  click(host.querySelector(".strip-route-btn"));
  return onChange;
}

// The menu renders through a portal, outside the host.
const row = (description: string) =>
  [...document.querySelectorAll('[role="menuitemcheckbox"]')].find((el) =>
    el.textContent?.includes(description),
  );
const checked = (description: string) => row(description)?.getAttribute("aria-checked");
const menuOpen = () => document.querySelector('[role="menuitemcheckbox"]') !== null;

describe("OutputSelect", () => {
  // #114: music on the headphones and the speakers at the same time.
  it("ticking a device adds it and keeps the menu open for more", () => {
    const onChange = open([HEADSET.name]);
    click(row("Speakers Analog"));
    expect(onChange).toHaveBeenCalledWith([HEADSET.name, SPEAKERS.name]);
    expect(menuOpen()).toBe(true);
  });

  it("unticking one of several devices leaves the rest", () => {
    const onChange = open([HEADSET.name, SPEAKERS.name, TV.name]);
    click(row("Speakers Analog"));
    expect(onChange).toHaveBeenCalledWith([HEADSET.name, TV.name]);
  });

  it("ticks every chosen device, and only those", () => {
    open([HEADSET.name, SPEAKERS.name]);
    expect(checked("Arctis Nova 5")).toBe("true");
    expect(checked("Speakers Analog")).toBe("true");
    expect(checked("HDMI Display")).toBe("false");
    expect(checked("System default")).toBe("false");
  });

  // "System default" is the absence of a choice, not one more device.
  it("system default is ticked exactly when no device is", () => {
    open([]);
    expect(checked("System default")).toBe("true");
    expect(checked("Arctis Nova 5")).toBe("false");
  });

  it("ticking a device while following the default pins just that device", () => {
    const onChange = open([]);
    click(row("Arctis Nova 5"));
    expect(onChange).toHaveBeenCalledWith([HEADSET.name]);
  });

  it("unticking the last device goes back to the system default", () => {
    const onChange = open([HEADSET.name]);
    click(row("Arctis Nova 5"));
    expect(onChange).toHaveBeenCalledWith([]);
  });

  it("ticking system default clears every chosen device", () => {
    const onChange = open([HEADSET.name, SPEAKERS.name]);
    click(row("System default"));
    expect(onChange).toHaveBeenCalledWith([]);
  });

  it("clicking system default while it is already ticked changes nothing", () => {
    const onChange = open([]);
    click(row("System default"));
    expect(onChange).not.toHaveBeenCalled();
  });

  // It had no row at all: nothing in the menu was ticked, and the only way
  // to drop it was to clear every device.
  it("an unplugged chosen device keeps a row it can be unticked on", () => {
    const onChange = open(["alsa_output.usb-Gone", SPEAKERS.name]);
    expect(checked("alsa_output.usb-Gone")).toBe("true");
    expect(row("alsa_output.usb-Gone")?.textContent).toContain("not connected");
    click(row("alsa_output.usb-Gone"));
    expect(onChange).toHaveBeenCalledWith([SPEAKERS.name]);
  });

  it("stops offering more devices once the channel is full", () => {
    const eight = Array.from({ length: 8 }, (_, i) => `alsa_output.dev${i}`);
    const onChange = open(eight);
    const spare = row("HDMI Display") as HTMLButtonElement;
    expect(spare.disabled).toBe(true);
    click(spare);
    expect(onChange).not.toHaveBeenCalled();
    // What is chosen can still be dropped.
    expect((row("alsa_output.dev3") as HTMLButtonElement).disabled).toBe(false);
  });

  it("the trigger says what it opens and whether it is open", () => {
    const trigger = () => host.querySelector(".strip-route-btn");
    act(() =>
      root.render(
        <OutputSelect
          channelLabel="Music"
          value={[HEADSET.name, SPEAKERS.name]}
          onChange={vi.fn()}
        />,
      ),
    );
    expect(trigger()?.getAttribute("aria-label")).toBe("Output: Arctis Nova 5 + Speakers Analog");
    expect(trigger()?.getAttribute("aria-expanded")).toBe("false");
    click(trigger());
    expect(trigger()?.getAttribute("aria-expanded")).toBe("true");
  });

  // A strip's menu opens over its neighbours; it has to say whose it is.
  it("is headed with its channel's name", () => {
    open([]);
    expect(document.querySelector(".menu-head")?.textContent).toBe("Music output");
  });

  it("system default names the device it lands on while it is followed", () => {
    const onChange = vi.fn();
    act(() =>
      root.render(
        <OutputSelect
          channelLabel="Music"
          value={[]}
          resolved={[SPEAKERS.name]}
          onChange={onChange}
        />,
      ),
    );
    click(host.querySelector(".strip-route-btn"));
    expect(row("System default")?.textContent).toContain("Speakers Analog");
    // Still the default row, not a second Speakers row.
    expect(checked("System default")).toBe("true");
  });

  it("every device row carries its full name for when it is cut off", () => {
    open([]);
    expect(row("Speakers Analog")?.getAttribute("title")).toBe("Speakers Analog");
  });
});

describe("summarizeOutputs", () => {
  it("names the one device, by a word that fits the strip", () => {
    expect(summarizeOutputs([HEADSET.name], [], DEVICES)).toEqual({
      label: "Arctis Nova 5",
      shortLabel: "Arctis",
      icon: "headphones",
    });
  });

  it("counts several devices and lists them in full", () => {
    expect(summarizeOutputs([HEADSET.name, SPEAKERS.name], [], DEVICES)).toEqual({
      label: "Arctis Nova 5 + Speakers Analog",
      shortLabel: "2 outputs",
      icon: "speaker_group",
    });
  });

  it("shows where the system default lands, including after failover", () => {
    expect(summarizeOutputs([], [SPEAKERS.name], DEVICES)).toEqual({
      label: "System default (Speakers Analog)",
      shortLabel: "Speakers",
      icon: "speaker",
    });
    expect(summarizeOutputs([], [], DEVICES).shortLabel).toBe("Default");
  });

  it("keeps showing an unplugged device as the choice", () => {
    expect(summarizeOutputs(["alsa_output.usb-Gone"], [SPEAKERS.name], DEVICES).label).toBe(
      "alsa_output.usb-Gone",
    );
  });
});
