#!/bin/sh
# uninstall_deps_void.sh — Remove BUILD-ONLY dependencies installed by
# install_deps_void.sh on Void Linux.
#
# This script ONLY removes:
#   - All -devel packages (headers, .so symlinks, pkgconfig files)
#   - base-devel (compiler, linker, make)
#   - wayland-protocols (build-time only, no runtime binary)
#   - AppImage tooling downloaded to ~/.local/bin (appimagetool, linuxdeploy)
#
# This script does NOT remove:
#   - Runtime tools: wf-recorder, wlr-randr, slurp, ffmpeg, pipewire,
#     wireplumber, xdg-desktop-portal, xdg-desktop-portal-wlr,
#     xdg-desktop-portal-gtk
#   - These are needed for wf-recorder-gui to work at runtime.
#
# If a -devel package is a dependency of another -devel package, xbps-remove
# -R removes both automatically (recursive removal).
#
# Rust (installed via rustup in $HOME) is NOT removed by default because the
# user may have other Rust projects. To also remove Rust:
#   ./uninstall_deps_void.sh --all
# or manually: rustup self uninstall
#
# Detects doas or sudo automatically (prefers doas if both are present).
#
# Usage:
#   ./uninstall_deps_void.sh                # remove -devel + base-devel + AppImage tools
#   ./uninstall_deps_void.sh --keep-appimage # don't remove ~/.local/bin appimagetool/linuxdeploy
#   ./uninstall_deps_void.sh --all           # also uninstall Rust toolchain
#   ./uninstall_deps_void.sh --runtime       # ALSO remove runtime tools (wf-recorder, etc.)
#                                            # use this only if you're uninstalling wf-recorder-gui too

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
    echo "  xbps-install -y opendoas   # or"
    echo "  xbps-install -y sudo"
    exit 1
fi
if [ -z "$PRIV" ]; then
    echo "[priv] running as root, no escalation tool needed"
else
    echo "[priv] using $PRIV for privileged operations"
fi

if ! command -v xbps-remove >/dev/null 2>&1; then
    echo "ERROR: xbps-remove not found. This script is for Void Linux."
    exit 1
fi

# Parse flags
REMOVE_RUST=0
REMOVE_RUNTIME=0
KEEP_APPIMAGE=0
for arg in "$@"; do
    case "$arg" in
        --all) REMOVE_RUST=1 ;;
        --runtime) REMOVE_RUNTIME=1 ;;
        --keep-appimage) KEEP_APPIMAGE=1 ;;
        *) echo "Unknown flag: $arg"; exit 1 ;;
    esac
done

# ---------------------------------------------------------------------------
# Build-time -devel packages. These are safe to remove — they only contain
# headers and .so symlinks, no runtime binaries.
#
# We use `xbps-remove -R` (recursive) so that if -devel package B depends on
# -devel package A, both get removed. If a -devel package is still needed by
# an installed runtime package, xbps-remove will refuse and we skip it.
#
# We add `|| true` after each command so the script continues even if some
# packages aren't installed or can't be removed.
# ---------------------------------------------------------------------------
DEVEL_PACKAGES="
    gtk+3-devel
    glib-devel
    libglib-devel
    pango-devel
    cairo-devel
    atk-devel
    at-spi2-atk-devel
    at-spi2-core-devel
    gdk-pixbuf-devel
    wayland-devel
    wayland-protocols
    libxkbcommon-devel
    libepoxy-devel
    MesaLib-devel
    MesaLib-dri-devel
    fribidi-devel
    harfbuzz-devel
    pipewire-devel
    pulseaudio-devel
    libpulseaudio-devel
    dbus-devel
    gdk-pixbuf-xlib-devel
    cloudproviders-devel
"

echo ""
echo "=== Removing build-time -devel packages (recursive) ==="
echo "These are headers and .so symlinks only — no runtime binaries."
echo "If a -devel package depends on another -devel package, both are removed."
echo ""
$PRIV xbps-remove -Ry $DEVEL_PACKAGES 2>&1 || true

# ---------------------------------------------------------------------------
# base-devel: contains gcc, make, binutils, etc. These are ONLY needed for
# compilation. We remove them with -R (recursive) so subpackages go too.
#
# Safety: xbps-remove will refuse if another installed package depends on
# base-devel (e.g. some pre-compiled binary that declares a build-dep).
# In that case the removal is skipped — that's the correct behavior.
# ---------------------------------------------------------------------------
echo ""
echo "=== Removing base-devel (compiler toolchain) ==="
echo "If a runtime package depends on base-devel, it will be kept."
$PRIV xbps-remove -Ry base-devel 2>&1 || true

# ---------------------------------------------------------------------------
# AppImage tooling downloaded to ~/.local/bin by install_deps_void.sh.
# These are standalone AppImages, not xbps packages — we just delete the
# files. Skip if --keep-appimage was passed.
# ---------------------------------------------------------------------------
if [ "$KEEP_APPIMAGE" = "0" ]; then
    echo ""
    echo "=== Removing AppImage tooling from ~/.local/bin ==="
    APPIMAGE_DIR="$HOME/.local/bin"
    for tool in appimagetool linuxdeploy; do
        if [ -f "$APPIMAGE_DIR/$tool" ]; then
            rm -f "$APPIMAGE_DIR/$tool"
            echo "  removed: $APPIMAGE_DIR/$tool"
        else
            echo "  skip (not found): $APPIMAGE_DIR/$tool"
        fi
    done
    # Also remove them from fish_user_paths if ~/.local/bin is now empty
    if command -v fish >/dev/null 2>&1; then
        if [ -z "$(ls -A "$APPIMAGE_DIR" 2>/dev/null)" ]; then
            fish -c "
                if contains \"$APPIMAGE_DIR\" \$fish_user_paths
                    set -e --universal fish_user_paths[(contains -i \"$APPIMAGE_DIR\" \$fish_user_paths)]
                    echo \"  removed $APPIMAGE_DIR from fish_user_paths (dir is empty)\"
                end
            " 2>/dev/null || true
        fi
    fi
else
    echo ""
    echo "=== Keeping AppImage tooling (--keep-appimage) ==="
fi

# ---------------------------------------------------------------------------
# Optional: remove runtime tools. Only if --runtime was passed.
# By default we KEEP these because wf-recorder-gui needs them to work.
# ---------------------------------------------------------------------------
if [ "$REMOVE_RUNTIME" = "1" ]; then
    echo ""
    echo "=== Removing runtime tools (--runtime) ==="
    echo "WARNING: wf-recorder-gui will NOT work after this!"
    echo "Only do this if you're uninstalling wf-recorder-gui too."
    echo ""
    $PRIV xbps-remove -Ry \
        wf-recorder \
        wlr-randr \
        slurp \
        ffmpeg \
        pipewire \
        wireplumber \
        xdg-desktop-portal \
        xdg-desktop-portal-wlr \
        xdg-desktop-portal-gtk 2>&1 || true
else
    echo ""
    echo "=== Keeping runtime tools ==="
    echo "wf-recorder, wlr-randr, slurp, ffmpeg, pipewire, wireplumber,"
    echo "xdg-desktop-portal{,-wlr,-gtk} are KEPT because wf-recorder-gui"
    echo "needs them at runtime."
    echo ""
    echo "To also remove them: ./uninstall_deps_void.sh --runtime"
fi

# ---------------------------------------------------------------------------
# Clean up orphaned dependencies (packages that were only installed as deps
# of the -devel packages we just removed).
# ---------------------------------------------------------------------------
echo ""
echo "=== Cleaning up orphaned dependencies ==="
$PRIV xbps-remove -oy 2>&1 || true

# ---------------------------------------------------------------------------
# Optional: remove Rust toolchain.
# ---------------------------------------------------------------------------
if [ "$REMOVE_RUST" = "1" ]; then
    echo ""
    echo "=== Removing Rust toolchain ==="
    if command -v rustup >/dev/null 2>&1; then
        rustup self uninstall -y || true
    else
        echo "  rustup not found, skipping."
    fi
    # Also remove cargo bin from fish_user_paths
    if command -v fish >/dev/null 2>&1; then
        fish -c "
            if contains \"$HOME/.cargo/bin\" \$fish_user_paths
                set -e --universal fish_user_paths[(contains -i \"$HOME/.cargo/bin\" \$fish_user_paths)]
                echo \"  removed ~/.cargo/bin from fish_user_paths\"
            end
        " 2>/dev/null || true
    fi
else
    echo ""
    echo "Rust toolchain kept. To remove it: ./uninstall_deps_void.sh --all"
    echo "Or manually: rustup self uninstall"
fi

echo ""
echo "=== Done ==="
echo "Build dependencies removed. Runtime tools kept."
echo ""
echo "If you're also uninstalling wf-recorder-gui itself:"
echo "  ./scripts/uninstall.sh              # removes the binary + .desktop + icon"
echo "  ./scripts/uninstall_deps_void.sh --runtime  # removes wf-recorder, slurp, etc."
echo "  ./scripts/uninstall_deps_void.sh --all      # also removes Rust"
