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
  act(() => root.render(<OutputSelect compact value={value} onChange={onChange} />));
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
