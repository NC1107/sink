%global debug_package %{nil}

Name:           sink
Version:        0.1.19
Release:        1%{?dist}
Summary:        SteelSeries Sonar for Linux - per-app audio routing, mixing, and a processed virtual mic on PipeWire

License:        GPL-3.0-only
URL:            https://github.com/NC1107/sink
# The prebuilt release .deb: same FHS payload as the .rpm, linked against
# system libraries. Repackaged rather than built from source - a Tauri build
# needs network for npm/cargo, which a COPR/mock buildroot does not have.
Source0:        https://github.com/NC1107/sink/releases/download/v%{version}/sink_%{version}_amd64.deb

ExclusiveArch:  x86_64

BuildRequires:  binutils
BuildRequires:  tar
BuildRequires:  gzip

# Runtime services and the dlopen'd tray library (the app aborts on startup
# without libayatana-appindicator, so it is a hard dep). Linked libraries
# (webkit2gtk4.1, libsoup3, gtk3, pipewire-libs, ...) are added automatically
# from the binary's NEEDED sonames.
Requires:       wireplumber
Requires:       pipewire-pulseaudio
Requires:       pulseaudio-utils
Requires:       hicolor-icon-theme
Requires:       libayatana-appindicator3.so.1()(64bit)

%description
Sink is a Linux-native audio routing and mixing app built on PipeWire, in the
spirit of SteelSeries Sonar or Voicemeeter. Create named virtual channels
(Game, Chat, Music, System), assign application audio streams to them, control
volume and mute per channel, and route a processed virtual microphone.

This package repackages the official prebuilt release, linked against the
system PipeWire and webkit2gtk - no bundled libraries and no build toolchain.

%prep
# Source0 is a .deb, not a tarball: create the build dir (-c), skip the default
# unpack (-T), then peel the ar container down to its payload.
%setup -q -c -T
ar x %{SOURCE0}
tar xzf data.tar.gz

%build
# Nothing to build - the binary is prebuilt.

%install
mkdir -p %{buildroot}%{_prefix}
cp -a usr/. %{buildroot}%{_prefix}/

%files
%{_bindir}/sink
%{_datadir}/applications/sink.desktop
%{_datadir}/icons/hicolor/*/apps/sink.png

%changelog
* Wed Jul 15 2026 NC1107 <nickpconn@gmail.com> - 0.1.19-1
- Initial COPR packaging (repackage of the official release .deb).
