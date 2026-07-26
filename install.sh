#!/usr/bin/env bash
# Sink installer — builds from source and installs system-wide.
#
#   ./install.sh              build + install (asks for sudo once)
#   ./install.sh --deps-only  only install build/runtime dependencies
#   ./install.sh --no-deps    skip dependency installation
#   ./install.sh --uninstall  remove Sink (keeps your ~/.config/sink)
#
# Supports Debian/Ubuntu (and derivatives: Mint, Pop!_OS, PikaOS, …),
# Fedora, and Arch. Your configuration in ~/.config/sink is never touched.
set -euo pipefail

REPO_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
PREFIX="${PREFIX:-/usr}"
APP_ID="sink"

bold() { printf '\033[1m%s\033[0m\n' "$*"; }
info() { printf '\033[1;34m==>\033[0m %s\n' "$*"; }
warn() { printf '\033[1;33m warn:\033[0m %s\n' "$*"; }
die()  { printf '\033[1;31merror:\033[0m %s\n' "$*" >&2; exit 1; }

# ---------------------------------------------------------------- distro
detect_distro() {
  [ -r /etc/os-release ] || { echo unknown; return; }
  . /etc/os-release
  # ID_LIKE catches derivatives (Mint -> ubuntu, PikaOS -> debian, …)
  case " ${ID:-} ${ID_LIKE:-} " in
    *" debian "*|*" ubuntu "*) echo debian ;;
    *" fedora "*|*" rhel "*)   echo fedora ;;
    *" arch "*)                echo arch ;;
    *) echo unknown ;;
  esac
}

install_deps() {
  local distro="$1"
  case "$distro" in
    debian)
      info "Installing dependencies (Debian/Ubuntu)"
      # webkit2gtk-4.1 is Tauri v2's webview; libayatana-appindicator provides
      # the tray icon; libpipewire is what Sink's audio engine links against.
      sudo apt-get update
      sudo apt-get install -y \
        build-essential curl wget file pkg-config \
        libwebkit2gtk-4.1-dev \
        libayatana-appindicator3-dev \
        librsvg2-dev \
        libssl-dev \
        libxdo-dev \
        libpipewire-0.3-dev \
        clang \
        ffmpeg
      ;;
    fedora)
      info "Installing dependencies (Fedora)"
      sudo dnf install -y \
        @development-tools \
        webkit2gtk4.1-devel openssl-devel curl wget file \
        libappindicator-gtk3-devel librsvg2-devel \
        pipewire-devel clang ffmpeg-free
      ;;
    arch)
      info "Installing dependencies (Arch)"
      sudo pacman -S --needed --noconfirm \
        base-devel curl wget file openssl webkit2gtk-4.1 \
        libayatana-appindicator librsvg pipewire clang ffmpeg
      ;;
    *)
      warn "Unknown distribution — install these yourself: a C toolchain,"
      warn "webkit2gtk-4.1, libayatana-appindicator3, librsvg2, openssl,"
      warn "libxdo, pipewire headers, clang, and ffmpeg (optional, for OLED video)."
      ;;
  esac
}

need_rust() {
  if command -v cargo >/dev/null 2>&1; then return; fi
  # rustup's default install puts cargo here but doesn't touch the current shell.
  if [ -x "$HOME/.cargo/bin/cargo" ]; then
    export PATH="$HOME/.cargo/bin:$PATH"
    return
  fi
  info "Installing Rust via rustup"
  curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh -s -- -y
  export PATH="$HOME/.cargo/bin:$PATH"
}

need_node() {
  command -v npm >/dev/null 2>&1 ||
    die "npm not found. Install Node.js 20+ (e.g. https://github.com/nvm-sh/nvm) and re-run."
}

# ---------------------------------------------------------------- actions
do_uninstall() {
  info "Removing Sink (your ~/.config/sink is kept)"
  sudo rm -f "$PREFIX/bin/$APP_ID"
  sudo rm -f "$PREFIX/share/applications/$APP_ID.desktop"
  sudo rm -f "$PREFIX/share/icons/hicolor/512x512/apps/$APP_ID.png"
  sudo rm -f /etc/udev/rules.d/50-sink-steelseries.rules
  sudo udevadm control --reload-rules 2>/dev/null || true
  systemctl --user disable --now sink.service 2>/dev/null || true
  bold "Uninstalled. Config left at ~/.config/sink (delete it manually if you want a clean slate)."
}

do_build() {
  need_rust
  need_node
  info "Installing JavaScript dependencies"
  ( cd "$REPO_DIR" && npm ci 2>/dev/null || npm install )
  info "Building release binary (this takes a few minutes on a cold cache)"
  ( cd "$REPO_DIR" && ./node_modules/.bin/tauri build --no-bundle )
}

do_install() {
  local bin="$REPO_DIR/target/release/$APP_ID"
  [ -x "$bin" ] || die "build output missing: $bin"

  info "Installing binary to $PREFIX/bin/$APP_ID"
  sudo install -Dm755 "$bin" "$PREFIX/bin/$APP_ID"

  info "Installing desktop entry and icon"
  sudo install -Dm644 /dev/stdin "$PREFIX/share/applications/$APP_ID.desktop" <<EOF
[Desktop Entry]
Type=Application
Name=Sink
Comment=Audio routing, mixing and SteelSeries headset control for PipeWire
Exec=$APP_ID
Icon=$APP_ID
Terminal=false
Categories=AudioVideo;Audio;Mixer;
StartupWMClass=$APP_ID
EOF
  local icon="$REPO_DIR/src-tauri/icons/512x512.png"
  [ -f "$icon" ] && sudo install -Dm644 "$icon" \
    "$PREFIX/share/icons/hicolor/512x512/apps/$APP_ID.png"

  # Lets Sink talk to SteelSeries HID devices without root.
  if [ -f "$REPO_DIR/packaging/udev/50-sink-steelseries.rules" ]; then
    info "Installing udev rule for SteelSeries devices"
    sudo install -Dm644 "$REPO_DIR/packaging/udev/50-sink-steelseries.rules" \
      /etc/udev/rules.d/50-sink-steelseries.rules
    sudo udevadm control --reload-rules
    sudo udevadm trigger
  fi

  bold ""
  bold "Sink installed."
  echo "  Launch:      $APP_ID   (or from your application menu)"
  echo "  Config:      ~/.config/sink"
  echo "  Autostart:   enable it in Settings, or your desktop restores the session"
  echo
  echo "If a SteelSeries base station is plugged in, re-plug it once so the new"
  echo "udev rule applies, then open the Headset tab."
}

# ---------------------------------------------------------------- main
DEPS=1
case "${1:-}" in
  --uninstall) do_uninstall; exit 0 ;;
  --deps-only) install_deps "$(detect_distro)"; exit 0 ;;
  --no-deps)   DEPS=0 ;;
  "")          ;;
  *)           die "unknown option: $1" ;;
esac

bold "Sink installer"
[ "$DEPS" -eq 1 ] && install_deps "$(detect_distro)"
do_build
do_install
