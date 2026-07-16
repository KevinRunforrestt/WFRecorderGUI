#!/bin/sh
# install_deps_debian.sh — Install build and runtime dependencies on
# Debian/Ubuntu and derivatives.
#
# Tested on Debian 13 (trixie), Ubuntu 22.04, Ubuntu 24.04.
#
# Usage:
#   ./install_deps_debian.sh
#
# Detects doas or sudo automatically (prefers doas if both are present).

set -eu

# ---------------------------------------------------------------------------
# Detect privilege escalation tool (doas or sudo).
# ---------------------------------------------------------------------------
detect_priv() {
    if [ "$(id -u)" = "0" ]; then
        echo ""
        return
    fi
    if command -v doas >/dev/null 2>&1; then
        echo "doas"
    elif command -v sudo >/dev/null 2>&1; then
        echo "sudo"
    else
        echo ""
    fi
}

PRIV="$(detect_priv)"
if [ -z "$PRIV" ] && [ "$(id -u)" != "0" ]; then
    echo "ERROR: Neither doas nor sudo found. Install one of them:"
    echo "  apt install -y doas      # or"
    echo "  apt install -y sudo"
    exit 1
fi
if [ -z "$PRIV" ]; then
    echo "[priv] running as root, no escalation tool needed"
else
    echo "[priv] using $PRIV for privileged operations"
fi

if ! command -v apt-get >/dev/null 2>&1; then
    echo "ERROR: apt-get not found. This script is for Debian/Ubuntu."
    exit 1
fi

echo ""
echo "=== Updating apt package lists ==="
$PRIV apt-get update -y

echo ""
echo "=== Installing build dependencies ==="
$PRIV apt-get install -y \
    build-essential \
    curl \
    pkg-config \
    libgtk-3-dev \
    libglib2.0-dev \
    libpango1.0-dev \
    libcairo2-dev \
    libatk1.0-dev \
    libatk-bridge2.0-dev \
    libgdk-pixbuf-2.0-dev \
    libwayland-dev \
    libxkbcommon-dev \
    libepoxy-dev \
    libfribidi-dev \
    libharfbuzz-dev \
    libpipewire-0.3-dev \
    libpulse-dev \
    libdbus-1-dev \
    libcloudproviders-dev \
    libatspi2.0-dev

# Rust toolchain via rustup (always recent)
if ! command -v rustc >/dev/null 2>&1; then
    echo "Installing Rust via rustup…"
    curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh -s -- -y --default-toolchain stable --profile minimal
    . "$HOME/.cargo/env"
else
    echo "Rust already installed: $(rustc --version)"
fi

echo ""
echo "=== Installing runtime tools ==="
$PRIV apt-get install -y \
    wf-recorder \
    wlr-randr \
    slurp \
    ffmpeg \
    pipewire \
    wireplumber \
    xdg-desktop-portal \
    xdg-desktop-portal-wlr \
    xdg-desktop-portal-gtk

echo ""
echo "=== Verifying installation ==="
echo "rustc: $(rustc --version 2>/dev/null || echo 'NOT FOUND')"
echo "gtk+-3.0: $(pkg-config --modversion gtk+-3.0 2>/dev/null || echo 'NOT FOUND')"
echo "libpipewire-0.3: $(pkg-config --modversion libpipewire-0.3 2>/dev/null || echo 'NOT FOUND')"
echo ""
echo "All dependencies installed. Now run:"
echo "  ./build_appimage.sh        # to build the AppImage"
echo "  ./build_static_musl.sh     # to build the static musl binary"
