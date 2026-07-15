# Fedora packaging

`sink.spec` is the Fedora package for Sink, built and hosted on [COPR](https://copr.fedorainfracloud.org/coprs/nc1107/sink/).
It repackages the release `.deb` (no source build), so it installs with system PipeWire and webkit2gtk and no toolchain.

Install:

```bash
sudo dnf copr enable nc1107/sink
sudo dnf install sink
```

Or grab the `.rpm` straight from a [release](https://github.com/NC1107/sink/releases) and `sudo dnf install ./sink-*.rpm` - no COPR needed.

New COPR version: bump `Version:` in the spec and rebuild (`copr-cli build sink <srpm>`, or let the release CI push it).
The linked-library `Requires` come from the binary's sonames automatically; only the runtime services and the dlopen'd tray lib are declared by hand.
