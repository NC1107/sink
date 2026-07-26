# Sink

<!-- social-badges:start -->
[![Discord](https://img.shields.io/badge/Discord-5865F2?logo=discord&logoColor=white)](https://discord.gg/jUMuSxGf6q)
[![GitHub](https://img.shields.io/badge/GitHub-181717?logo=github&logoColor=white)](https://github.com/NC1107)
[![Patreon](https://img.shields.io/badge/Patreon-F96854?logo=patreon&logoColor=white)](https://patreon.com/NPC1107)
<!-- social-badges:end -->

SteelSeries Sonar for Linux. Built on PipeWire.

Route each app to its own channel - Game, Chat, Music - and control
volume, mute, and output device per channel. Build mixes for OBS and a
processed virtual microphone for voice chat.

![Mixer](docs/mixer.png)

```
 apps ─► channels ─► your ears
              └────► a Mix ─► OBS / recorder
```

## Features

- **Channels** - per-app routing with volume, mute, meters, and a choice
  of output device per channel
- **Apps** - running apps appear automatically; assign once, remembered
  forever
- **Mixes** - recordable sources for OBS. Master Mix carries everything;
  custom mixes can carry "everything except music" and stay current as
  channels change. In OBS, add a mix as an audio input - not Desktop Audio.
- **Equalizer** - per-channel parametric EQ (up to 10 bands) with a
  draggable response curve, bundled community presets, and import/export
  including AutoEq text blocks
- **Microphone** - noise gate, compressor and limiter into a virtual mic
  you select in Discord or OBS. Pairs well with
  [NoiseTorch](https://github.com/noisetorch/NoiseTorch) on the input for
  noise suppression before the chain.
- **Profiles** - save and switch full layouts from the tray
- **Headset** - full control of a **SteelSeries Arctis Nova Pro Wireless**
  base station over USB (no root, no SteelSeries GG): live battery, ANC /
  transparency, sidetone, mic volume & mute-LED, 10-band hardware EQ, auto-off,
  wireless speed/range, line-out, and a ChatMix read-out. See
  [docs/headset.md](docs/headset.md).
- **OLED** - drive the base station's 128×64 display: live dashboard,
  system monitor (CPU/RAM/GPU), now playing, desktop-notification mirroring,
  text, built-in animations, and your own images / GIFs / videos, scaled and
  dithered on the fly.
- **Mouse** - SteelSeries **Aerox 9 Wireless**: DPI presets, polling rate,
  per-zone RGB, reactive lighting, sleep and dim timeouts, battery.
- **Themes** - ships the original look plus a **Tokyo Night** palette,
  switchable in Settings.

![Equalizer](docs/eq.png)
![Mic](docs/mic.png)
![Apps](docs/apps.png)

## Install

**Build and install from source (any distro)**

```bash
git clone <your-fork-url> sink && cd sink
./install.sh
```

`install.sh` installs the build/runtime dependencies for Debian/Ubuntu (and
derivatives like Mint, Pop!_OS), Fedora and Arch, builds the release binary,
and installs the app, its desktop entry and the udev rule that lets Sink talk
to SteelSeries devices without root. Useful flags: `--deps-only`, `--no-deps`,
`--uninstall` (uninstall keeps your `~/.config/sink`).

**Debian / Ubuntu — manual dependencies**

If you would rather do it by hand:

```bash
sudo apt update
sudo apt install -y build-essential curl wget file pkg-config \
  libwebkit2gtk-4.1-dev libayatana-appindicator3-dev librsvg2-dev \
  libssl-dev libxdo-dev libpipewire-0.3-dev clang ffmpeg
curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh   # if you have no Rust
npm install && ./node_modules/.bin/tauri build --no-bundle
sudo install -Dm755 target/release/sink /usr/bin/sink
```

`ffmpeg` is optional and only needed to play videos on the headset OLED.

### Prebuilt packages


**Arch / Manjaro / EndeavourOS** - from the [AUR](https://aur.archlinux.org/packages/sink-bin):

```bash
yay -S sink-bin      # or: paru -S sink-bin
```

**Fedora** - from [COPR](https://copr.fedorainfracloud.org/coprs/nc1107/sink/):

```bash
sudo dnf copr enable nc1107/sink
sudo dnf install sink
```

Both track new releases, so you update through your package manager like any
other package.

Otherwise, grab the latest from [Releases](https://github.com/NC1107/sink/releases)
and install the file directly:

**Fedora / openSUSE**

```bash
sudo dnf install ./sink-*.x86_64.rpm
```

**Debian / Ubuntu / Mint**

```bash
sudo apt install ./sink_*_amd64.deb
```

**Arch / Manjaro / EndeavourOS**

```bash
sudo pacman -U ./sink-bin-*-x86_64.pkg.tar.zst
```

These install the app properly - launcher entry, icon, uninstall
through your package manager.

**Any other distro - AppImage (portable, no root)**

```bash
chmod +x sink_*_amd64.AppImage
./sink_*_amd64.AppImage
```

To get a launcher entry for an AppImage, use
[Gear Lever](https://flathub.org/apps/it.mijorus.gearlever) or
AppImageLauncher.

Requires PipeWire with `pipewire-pulse` and WirePlumber 0.5+ (the default
on most current distros).

## Build

```bash
npm install
npm run tauri dev      # run
npm run tauri build    # package
```

Config lives in `~/.config/sink` as plain JSON.

## License

[GPL-3.0](LICENSE)
