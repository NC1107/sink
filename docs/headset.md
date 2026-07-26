# Headset & OLED — Arctis Nova Pro Wireless

Sink can drive a **SteelSeries Arctis Nova Pro Wireless** base station directly
over USB — no SteelSeries GG, no root. It speaks the vendor HID protocol on
`/dev/hidraw` and exposes everything in the **Headset** and **OLED** tabs.

Supported base stations (USB product ids): `0x12e0`, `0x12e5`, `0x225d`
(Nova Pro Wireless and the Xbox "X" variants).

## Headset tab

- **Live status** — headset & charge-slot battery, output volume, wireless
  pairing, Bluetooth, mic mute, power/charging state. Pushed live from the
  device's own event stream plus a 2-second heartbeat.
- **Noise control** — ANC / Transparency / Off, transparency passthrough level,
  audio gain (low/high), and an optional WirePlumber anti-crackle headroom fix.
- **Microphone** — mic volume, sidetone (off/low/med/high), mute-LED brightness.
- **Hardware equalizer** — the device's 10-band EQ (±10 dB) plus its built-in
  presets. This is the EQ baked into the headset, separate from Sink's
  per-channel software EQ.
- **Line out** — speaker vs. stream mode and the stream main-L/R + aux volumes.
- **Power & wireless** — auto shut-off timer and 2.4 GHz speed/range mode.

Settings apply live and are saved to the headset's own memory (debounced).

## OLED tab

The base station's 128×64 monochrome panel:

- **Live dashboard** — battery, volume, ANC and mic state, refreshed on device.
- **Text & notifications** — up to four auto-sized lines; notifications show for
  a set duration then revert.
- **Built-in clips** — original, procedurally generated animations (DOOM fire,
  Matrix rain, starfield, bouncing wordmark, plasma). No third-party media is
  bundled.
- **Upload image / GIF / video** — your own files are scaled to 128×64 and
  Floyd–Steinberg dithered to 1-bit on the fly. Stills and GIFs decode
  in-process; videos go through the system `ffmpeg` (install it to enable
  video). Copyrighted clips (Bad Apple, etc.) are yours to supply — nothing is
  downloaded automatically.
- **SteelSeries UI** — hand the panel back to the firmware at any time.

The firmware keeps repainting the panel, so Sink holds custom content with a
continuous ~24 fps draw loop (which also drives animation).

## Access without root (udev)

Sink needs read/write on the base station's `hidraw` node. The bundled rule
grants this to the logged-in desktop user:

```bash
sudo cp packaging/udev/50-sink-steelseries.rules /etc/udev/rules.d/
sudo udevadm control --reload-rules && sudo udevadm trigger
```

Re-plug the base station afterwards. The rule covers all SteelSeries devices
(vendor `0x1038`).

## Notes

- Don't run another Arctis controller (e.g. SteelSeries GG under Wine, or
  `Linux-Arctis-Manager`/`lam-daemon`) at the same time — two programs writing
  to the same base station will fight over it.
- The protocol was cross-checked against HeadsetControl,
  elegos/Linux-Arctis-Manager and loteran/Arctis-Sound-Manager, and verified
  live against real hardware. OLED encoding follows JerwuQu/ggoled.
