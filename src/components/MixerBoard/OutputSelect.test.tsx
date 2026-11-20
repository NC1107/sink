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

const row = (description: string) =>
  [...document.querySelectorAll('[role="menuitemradio"]')].find((el) =>
    el.textContent?.includes(description),
  );
// The menu renders through a portal, outside the host.
const sideButton = (label: string) => document.querySelector(`[aria-label="${label}"]`);
const menuOpen = () => document.querySelector('[role="menuitemradio"]') !== null;

describe("OutputSelect", () => {
  // Most channels play on one device: switching it must stay one click.
  it("clicking a device plays on it alone and closes the menu", () => {
    const onChange = open([HEADSET.name, SPEAKERS.name]);
    click(row("HDMI Display"));
    expect(onChange).toHaveBeenCalledWith([TV.name]);
    expect(menuOpen()).toBe(false);
  });

  // #114: music on the headphones and the speakers at the same time.
  it("adds a second device and keeps the menu open for more", () => {
    const onChange = open([HEADSET.name]);
    click(sideButton("Also play on Speakers Analog"));
    expect(onChange).toHaveBeenCalledWith([HEADSET.name, SPEAKERS.name]);
    expect(menuOpen()).toBe(true);
  });

  it("drops one of several devices and leaves the rest", () => {
    const onChange = open([HEADSET.name, SPEAKERS.name, TV.name]);
    click(sideButton("Stop playing on Speakers Analog"));
    expect(onChange).toHaveBeenCalledWith([HEADSET.name, TV.name]);
  });

  it("marks every chosen device, and only those", () => {
    open([HEADSET.name, SPEAKERS.name]);
    expect(row("Arctis Nova 5")?.getAttribute("aria-checked")).toBe("true");
    expect(row("Speakers Analog")?.getAttribute("aria-checked")).toBe("true");
    expect(row("HDMI Display")?.getAttribute("aria-checked")).toBe("false");
    expect(row("System default")?.getAttribute("aria-checked")).toBe("false");
  });

  // Dropping the last device would leave nothing chosen without saying
  // what happens next; "System default" is the explicit way there.
  it("offers no remove button on the only chosen device", () => {
    open([HEADSET.name]);
    expect(sideButton("Stop playing on Arctis Nova 5")).toBeNull();
    expect(sideButton("Also play on Arctis Nova 5")).toBeNull();
  });

  it("system default clears every chosen device", () => {
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
