# Arch packaging

`PKGBUILD` is the Arch package for Sink, `sink-bin`.
It repackages the release `.deb` (no source build), so it installs with system PipeWire and webkit2gtk and no toolchain.

The `archpkg` job in `.github/workflows/release.yml` stamps it with each release's version and `.deb` checksum, builds it with `makepkg`, and attaches the `.pkg.tar.zst` to the release.
It is also on the [AUR](https://aur.archlinux.org/packages/sink-bin) (account `nc1107`).

Install: `yay -S sink-bin` (or `paru -S sink-bin`), or `sudo pacman -U` the `.pkg.tar.zst` from a release.

New AUR version: push an updated `PKGBUILD` + regenerated `.SRCINFO` (`makepkg --printsrcinfo`) to `ssh://aur@aur.archlinux.org/sink-bin.git`.
