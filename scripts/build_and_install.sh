#!/bin/sh
# build_and_install.sh — Build wf-recorder-gui from source and install it
# system-wide (binary + .desktop + icon + symlink).
#
# This is the simplest path: no packaging, just compile and copy files.
# Use make_xbps.sh if you want a proper .xbps package instead.
#
# Detects doas or sudo automatically (prefers doas if both are present).
#
# Usage:
#   ./build_and_install.sh             # build release + install to /usr/local
#   ./build_and_install.sh --uninstall # remove installed files

set -eu

APP_NAME="wf-recorder-gui"
APP_ID="io.github.wf-recorder-gui"
PREFIX="${PREFIX:-/usr/local}"

cd "$(dirname "$0")/.."
PROJECT_ROOT="$(pwd)"

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
    echo "  xbps-install -y opendoas   # Void"
    echo "  apt install -y sudo         # Debian"
    exit 1
fi
if [ -z "$PRIV" ]; then
    echo "[priv] running as root"
else
    echo "[priv] using $PRIV"
fi

# ---------------------------------------------------------------------------
# Uninstall mode
# ---------------------------------------------------------------------------
if [ "${1:-}" = "--uninstall" ]; then
    echo "=== Uninstalling $APP_NAME ==="
    $PRIV rm -f "$PREFIX/bin/$APP_NAME"
    $PRIV rm -f "$PREFIX/bin/wf-recorder-tui"
    $PRIV rm -f "$PREFIX/share/applications/$APP_ID.desktop"
    $PRIV rm -f "$PREFIX/share/icons/hicolor/scalable/apps/$APP_ID.svg"
    $PRIV rm -f "$PREFIX/share/metainfo/$APP_ID.appdata.xml"
    echo "Done."
    exit 0
fi

# ---------------------------------------------------------------------------
# Build
# ---------------------------------------------------------------------------
echo ""
echo "=== Building release binary ==="
if ! command -v cargo >/dev/null 2>&1; then
    echo "ERROR: cargo not found. Run install_deps_void.sh first."
    exit 1
fi

# Use a faster build profile for install (no LTO) to save time.
cargo build --release

BINARY="$PROJECT_ROOT/target/release/$APP_NAME"
if [ ! -f "$BINARY" ]; then
    echo "ERROR: build failed — binary not found at $BINARY"
    exit 1
fi

echo ""
echo "=== Installing to $PREFIX ==="
$PRIV install -d "$PREFIX/bin"
$PRIV install -d "$PREFIX/share/applications"
$PRIV install -d "$PREFIX/share/icons/hicolor/scalable/apps"
$PRIV install -d "$PREFIX/share/metainfo"

# Binary
$PRIV install -Dm755 "$BINARY" "$PREFIX/bin/$APP_NAME"

# Symlink so the TUI can be launched as wf-recorder-tui (busybox-style argv0)
$PRIV ln -sf "$APP_NAME" "$PREFIX/bin/wf-recorder-tui"

# .desktop file — rewrite Exec= with the full install path so that
# launchers (rofi, wofi, fuzzel, etc.) that don't inherit the shell PATH
# can still find the binary.
if [ -f "$PROJECT_ROOT/data/$APP_ID.desktop" ]; then
    TMP_DESKTOP="$(mktemp)"
    sed "s|^Exec=.*|Exec=$PREFIX/bin/$APP_NAME|" \
        "$PROJECT_ROOT/data/$APP_ID.desktop" > "$TMP_DESKTOP"
    $PRIV install -Dm644 "$TMP_DESKTOP" \
        "$PREFIX/share/applications/$APP_ID.desktop"
    rm -f "$TMP_DESKTOP"
fi

# Icon (SVG, scalable)
if [ -f "$PROJECT_ROOT/data/icons/$APP_ID.svg" ]; then
    $PRIV install -Dm644 "$PROJECT_ROOT/data/icons/$APP_ID.svg" \
        "$PREFIX/share/icons/hicolor/scalable/apps/$APP_ID.svg"
fi

# AppStream metainfo
if [ -f "$PROJECT_ROOT/data/$APP_ID.appdata.xml" ]; then
    $PRIV install -Dm644 "$PROJECT_ROOT/data/$APP_ID.appdata.xml" \
        "$PREFIX/share/metainfo/$APP_ID.appdata.xml"
fi

# Update icon cache if gtk-update-icon-cache exists
if command -v gtk-update-icon-cache >/dev/null 2>&1; then
    $PRIV gtk-update-icon-cache -f "$PREFIX/share/icons/hicolor" 2>/dev/null || true
fi

# Update desktop database if update-desktop-database exists
if command -v update-desktop-database >/dev/null 2>&1; then
    $PRIV update-desktop-database "$PREFIX/share/applications" 2>/dev/null || true
fi

# fish shell: ensure $PREFIX/bin is in fish_user_paths so the binary is
# found when typed in a fish prompt. POSIX shells get /usr/local/bin from
# /etc/profile or ~/.profile, but fish uses its own universal variable.
if command -v fish >/dev/null 2>&1; then
    echo ""
    echo "=== Configuring fish shell PATH ==="
    fish -c "
        if not contains \"$PREFIX/bin\" \$fish_user_paths
            set -U fish_user_paths \$fish_user_paths \"$PREFIX/bin\"
            echo \"  added $PREFIX/bin to fish_user_paths\"
        else
            echo \"  $PREFIX/bin already in fish_user_paths\"
        end
    " 2>/dev/null || true
fi

echo ""
echo "=== Installed ==="
echo "Binary:    $PREFIX/bin/$APP_NAME"
echo "TUI link:  $PREFIX/bin/wf-recorder-tui"
echo "Desktop:   $PREFIX/share/applications/$APP_ID.desktop"
echo "Icon:      $PREFIX/share/icons/hicolor/scalable/apps/$APP_ID.svg"
echo ""
echo "Verify with:"
echo "  $APP_NAME --version"
echo "  $APP_NAME about"
echo "  wf-recorder-tui --version"
echo ""
echo "To uninstall: ./scripts/build_and_install.sh --uninstall"
