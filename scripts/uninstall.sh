#!/bin/sh
# uninstall.sh — Remove wf-recorder-gui from the system.
#
# Removes:
#   - Binary: $PREFIX/bin/wf-recorder-gui
#   - Symlink: $PREFIX/bin/wf-recorder-tui
#   - .desktop file
#   - Icon (SVG)
#   - AppStream metainfo
#   - License file
#   - Optional: $PREFIX/bin from fish_user_paths (with --clean-fish)
#
# Detects doas or sudo automatically (prefers doas if both are present).
#
# Usage:
#   ./uninstall.sh                  # remove files only
#   ./uninstall.sh --clean-fish     # also remove PATH entry from fish
#   ./uninstall.sh --purge-config   # also remove ~/.config/wf-recorder-gui/

set -eu

APP_NAME="wf-recorder-gui"
APP_ID="io.github.wf-recorder-gui"
PREFIX="${PREFIX:-/usr/local}"

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
    echo "ERROR: Neither doas nor sudo found."
    echo "Run this script as root, or install doas/sudo."
    exit 1
fi
if [ -z "$PRIV" ]; then
    echo "[priv] running as root"
else
    echo "[priv] using $PRIV"
fi

# Parse flags
CLEAN_FISH=0
PURGE_CONFIG=0
for arg in "$@"; do
    case "$arg" in
        --clean-fish) CLEAN_FISH=1 ;;
        --purge-config) PURGE_CONFIG=1 ;;
        *) echo "Unknown flag: $arg"; exit 1 ;;
    esac
done

echo ""
echo "=== Removing $APP_NAME from $PREFIX ==="

# Files to remove
FILES_TO_REMOVE="
    $PREFIX/bin/$APP_NAME
    $PREFIX/bin/wf-recorder-tui
    $PREFIX/share/applications/$APP_ID.desktop
    $PREFIX/share/icons/hicolor/scalable/apps/$APP_ID.svg
    $PREFIX/share/metainfo/$APP_ID.appdata.xml
    $PREFIX/share/licenses/$APP_NAME/LICENSE
"

REMOVED=0
SKIPPED=0
for f in $FILES_TO_REMOVE; do
    if [ -e "$f" ] || [ -L "$f" ]; then
        $PRIV rm -f "$f"
        echo "  removed: $f"
        REMOVED=$((REMOVED + 1))
    else
        echo "  skip (not found): $f"
        SKIPPED=$((SKIPPED + 1))
    fi
done

# Update icon/desktop caches if the tools exist
if command -v gtk-update-icon-cache >/dev/null 2>&1; then
    $PRIV gtk-update-icon-cache -f "$PREFIX/share/icons/hicolor" 2>/dev/null || true
fi
if command -v update-desktop-database >/dev/null 2>&1; then
    $PRIV update-desktop-database "$PREFIX/share/applications" 2>/dev/null || true
fi

# Optionally clean fish PATH
if [ "$CLEAN_FISH" = "1" ] && command -v fish >/dev/null 2>&1; then
    echo ""
    echo "=== Cleaning fish_user_paths ==="
    fish -c "
        if contains \"$PREFIX/bin\" \$fish_user_paths
            set -e --universal fish_user_paths[(contains -i \"$PREFIX/bin\" \$fish_user_paths)]
            echo \"  removed $PREFIX/bin from fish_user_paths\"
        else
            echo \"  $PREFIX/bin not in fish_user_paths\"
        end
    " 2>/dev/null || true
fi

# Optionally purge user config
if [ "$PURGE_CONFIG" = "1" ]; then
    CONFIG_DIR="${XDG_CONFIG_HOME:-$HOME/.config}/wf-recorder-gui"
    echo ""
    echo "=== Purging user config ==="
    if [ -d "$CONFIG_DIR" ]; then
        rm -rf "$CONFIG_DIR"
        echo "  removed: $CONFIG_DIR"
    else
        echo "  no config dir at $CONFIG_DIR"
    fi
fi

echo ""
echo "=== Done ==="
echo "Removed: $REMOVED files"
echo "Skipped: $SKIPPED files (not installed)"
if [ "$PURGE_CONFIG" = "0" ]; then
    echo ""
    echo "User config kept at: ${XDG_CONFIG_HOME:-$HOME/.config}/wf-recorder-gui"
    echo "To also remove it: ./uninstall.sh --purge-config"
fi
