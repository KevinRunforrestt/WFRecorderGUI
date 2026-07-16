#!/bin/sh
# install_deps_void.sh — Install build and runtime dependencies on Void Linux.
#
# Tested on Void Linux glibc and musl. Installs:
#   - Rust toolchain (via rustup, user-space)
#   - GTK3 + deps dev headers
#   - PipeWire and PulseAudio dev headers
#   - wf-recorder, wlr-randr, slurp, ffmpeg (runtime tools)
#
# Usage:
#   ./install_deps_void.sh        # install everything
#   ./install_deps_void.sh musl   # install for musl target (adds musl target to rustup)
#
# This script is idempotent: re-running it is safe.
# Detects doas or sudo automatically (prefers doas if both are present).

set -eu

TARGET="${1:-glibc}"

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
    echo "  xbps-install -y opendoas   # or"
    echo "  xbps-install -y sudo"
    exit 1
fi
if [ -z "$PRIV" ]; then
    echo "[priv] running as root, no escalation tool needed"
else
    echo "[priv] using $PRIV for privileged operations"
fi

# ---------------------------------------------------------------------------
# Detect Void Linux. We check for xbps-install (the canonical package
# manager) — /etc/void-release can be missing on minimal containers or
# custom installs, but xbps-install is always present on Void.
# ---------------------------------------------------------------------------
echo ""
echo "=== Detecting Void Linux ==="
if [ -f /etc/void-release ]; then
    . /etc/void-release
    echo "Void Linux: $PRETTY_NAME"
elif command -v xbps-install >/dev/null 2>&1; then
    echo "Void Linux detected (xbps-install found; /etc/void-release missing — likely a minimal install)."
else
    echo "ERROR: This is not Void Linux (no /etc/void-release and no xbps-install)."
    echo "Use install_deps_debian.sh for Debian/Ubuntu."
    exit 1
fi

echo ""
echo "=== Updating xbps package lists ==="
$PRIV xbps-install -S || true

echo ""
echo "=== Installing build dependencies ==="
# Base build tools
$PRIV xbps-install -y base-devel curl
# Rust toolchain (we install via rustup so we get a recent version; xbps also
# has rust but it can lag behind).
if ! command -v rustc >/dev/null 2>&1; then
    echo "Installing Rust via rustup…"
    curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh -s -- -y --default-toolchain stable --profile minimal
    . "$HOME/.cargo/env"
else
    echo "Rust already installed: $(rustc --version)"
fi

# fish shell doesn't read ~/.profile or ~/.bashrc, so rustup's PATH setup
# doesn't apply to fish. Add cargo's bin to fish_user_paths manually.
if command -v fish >/dev/null 2>&1; then
    echo "Detected fish shell — adding cargo bin to fish_user_paths…"
    fish -c '
        if not contains "$HOME/.cargo/bin" $fish_user_paths
            set -U fish_user_paths $fish_user_paths "$HOME/.cargo/bin"
            echo "  added ~/.cargo/bin to fish_user_paths"
        else
            echo "  ~/.cargo/bin already in fish_user_paths"
        end
    ' 2>/dev/null || true
    # Also add ~/.local/bin (where we install appimagetool/linuxdeploy)
    fish -c '
        if not contains "$HOME/.local/bin" $fish_user_paths
            set -U fish_user_paths $fish_user_paths "$HOME/.local/bin"
            echo "  added ~/.local/bin to fish_user_paths"
        else
            echo "  ~/.local/bin already in fish_user_paths"
        end
    ' 2>/dev/null || true
fi

# GTK3 + friends. Note: Void package names differ from Debian's:
#   - libwayland-dev  -> wayland-devel
#   - libdbus-1-dev   -> dbus-devel
#   - appstream       -> AppStream (case-sensitive!)
#   - appimagetool / linuxdeploy don't exist in Void repos — we download
#     them separately below if needed.
$PRIV xbps-install -y \
    gtk+3-devel \
    glib-devel \
    pango-devel \
    cairo-devel \
    atk-devel \
    at-spi2-atk-devel \
    gdk-pixbuf-devel \
    wayland-devel \
    wayland-protocols \
    libxkbcommon-devel \
    libepoxy-devel \
    MesaLib-devel \
    fribidi-devel \
    harfbuzz-devel

# Audio backends
$PRIV xbps-install -y \
    pipewire-devel \
    pulseaudio-devel

# VAAPI (hardware video encoding for Intel/AMD GPUs)
# i965-va-driver: Intel gen 7-7.5 (Haswell, Ivy Bridge, etc.)
# intel-media-driver: Intel gen 8+ (Broadwell, Skylake, etc.)
# libva-utils: vainfo command to verify VAAPI support
$PRIV xbps-install -y \
    libva \
    libva-utils \
    i965-va-driver \
    intel-media-driver 2>/dev/null || true
echo "  (VAAPI drivers installed — some may not apply to your GPU)"

# D-Bus (for tray icon and portals)
$PRIV xbps-install -y dbus-devel

# Runtime tools that wf-recorder-gui calls
$PRIV xbps-install -y \
    wf-recorder \
    wlr-randr \
    slurp \
    ffmpeg \
    pipewire \
    wireplumber

# xdg-desktop-portal backends
$PRIV xbps-install -y \
    xdg-desktop-portal \
    xdg-desktop-portal-wlr \
    xdg-desktop-portal-gtk

# AppImage build tools. Void doesn't package appimagetool or linuxdeploy,
# so we download them from upstream (they're self-contained AppImages).
# AppStream (case-sensitive in Void!) provides appstream-util for metainfo.
$PRIV xbps-install -y AppStream
echo ""
echo "=== AppImage tooling (not in Void repos, downloading upstream) ==="
APPIMAGE_DIR="$HOME/.local/bin"
mkdir -p "$APPIMAGE_DIR"
if ! command -v appimagetool >/dev/null 2>&1; then
    echo "Downloading appimagetool…"
    curl -sL -o "$APPIMAGE_DIR/appimagetool" \
        "https://github.com/AppImage/AppImageKit/releases/download/continuous/appimagetool-x86_64.AppImage"
    chmod +x "$APPIMAGE_DIR/appimagetool"
    echo "  installed: $APPIMAGE_DIR/appimagetool"
fi
if ! command -v linuxdeploy >/dev/null 2>&1; then
    echo "Downloading linuxdeploy…"
    curl -sL -o "$APPIMAGE_DIR/linuxdeploy" \
        "https://github.com/linuxdeploy/linuxdeploy/releases/download/continuous/linuxdeploy-x86_64.AppImage"
    chmod +x "$APPIMAGE_DIR/linuxdeploy"
    echo "  installed: $APPIMAGE_DIR/linuxdeploy"
fi
echo "Make sure $APPIMAGE_DIR is in your PATH to build AppImages."

echo ""
echo "=== Adding Rust targets ==="
if [ "$TARGET" = "musl" ]; then
    rustup target add x86_64-unknown-linux-musl
    echo "Installed musl target."
else
    echo "Default glibc target is already installed."
fi

echo ""
echo "=== Verifying installation ==="
echo "rustc: $(rustc --version 2>/dev/null || echo 'NOT FOUND')"
echo "cargo: $(cargo --version 2>/dev/null || echo 'NOT FOUND')"
echo "gtk+-3.0: $(pkg-config --modversion gtk+-3.0 2>/dev/null || echo 'NOT FOUND')"
echo "libpipewire-0.3: $(pkg-config --modversion libpipewire-0.3 2>/dev/null || echo 'NOT FOUND')"
echo "libpulse: $(pkg-config --modversion libpulse 2>/dev/null || echo 'NOT FOUND')"
echo "wf-recorder: $(command -v wf-recorder || echo 'NOT FOUND')"
echo "wlr-randr: $(command -v wlr-randr || echo 'NOT FOUND')"
echo "slurp: $(command -v slurp || echo 'NOT FOUND')"
echo "ffmpeg: $(command -v ffmpeg || echo 'NOT FOUND')"
echo ""
echo "All dependencies installed. Now run:"
echo "  ./build_appimage.sh        # to build the AppImage"
echo "  ./build_static_musl.sh     # to build the static musl binary"
