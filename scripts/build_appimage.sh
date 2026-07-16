#!/bin/sh
# build_appimage.sh — Build a portable AppImage of wf-recorder-gui.
#
# The AppImage bundles the wf-recorder-gui binary plus the GTK3 shared
# libraries it needs. The resulting file is portable: it should run on any
# x86_64 Linux with glibc >= 2.31 (Ubuntu 20.04+).
#
# Requirements (installed by install_deps_*.sh):
#   - Rust toolchain (cargo)
#   - GTK3 dev headers (for compiling)
#   - linuxdeploy + AppImage plugin
#   - appimagetool (or appimage package)
#
# Usage:
#   ./build_appimage.sh

set -eu

cd "$(dirname "$0")/.."
PROJECT_ROOT="$(pwd)"
BUILD_DIR="$PROJECT_ROOT/target/appimage"
APPDIR="$BUILD_DIR/AppDir"
APP_NAME="wf-recorder-gui"
APP_ID="io.github.wf-recorder-gui"

echo "=== Building release binary ==="
cargo build --release

BINARY="$PROJECT_ROOT/target/release/$APP_NAME"
if [ ! -f "$BINARY" ]; then
    echo "ERROR: release binary not found at $BINARY"
    exit 1
fi

echo ""
echo "=== Preparing AppDir ==="
rm -rf "$BUILD_DIR"
mkdir -p "$APPDIR/usr/bin"
mkdir -p "$APPDIR/usr/share/applications"
mkdir -p "$APPDIR/usr/share/icons/hicolor/scalable/apps"
mkdir -p "$APPDIR/usr/share/metainfo"

# Copy binary
cp "$BINARY" "$APPDIR/usr/bin/$APP_NAME"

# Copy .desktop file
cat > "$APPDIR/usr/share/applications/$APP_ID.desktop" <<EOF
[Desktop Entry]
Type=Application
Name=WFRecorderGUI
Comment=GTK3 GUI for wf-recorder on wlroots compositors
Exec=$APP_NAME
Icon=$APP_ID
Categories=AudioVideo;Video;Recorder;
Terminal=false
Keywords=screen;recorder;capture;wayland;wlroots;
EOF

# Copy icon (assumes an SVG exists in data/icons/)
if [ -f "$PROJECT_ROOT/data/icons/$APP_ID.svg" ]; then
    cp "$PROJECT_ROOT/data/icons/$APP_ID.svg" "$APPDIR/usr/share/icons/hicolor/scalable/apps/"
fi

# Copy AppStream metainfo
if [ -f "$PROJECT_ROOT/data/$APP_ID.appdata.xml" ]; then
    cp "$PROJECT_ROOT/data/$APP_ID.appdata.xml" "$APPDIR/usr/share/metainfo/"
fi

# AppRun script
cat > "$APPDIR/AppRun" <<'EOF'
#!/bin/sh
HERE="$(dirname "$(readlink -f "$0")")"
export GTK_CSD=0
exec "$HERE/usr/bin/wf-recorder-gui" "$@"
EOF
chmod +x "$APPDIR/AppRun"

echo ""
echo "=== Running linuxdeploy ==="
# linuxdeploy produces the AppDir structure and bundles dependencies.
# We use the GTK3 plugin so all GTK libs are bundled.
LINUXDEPLOY="${LINUXDEPLOY:-linuxdeploy-x86_64.AppImage}"
if ! command -v "$LINUXDEPLOY" >/dev/null 2>&1 && [ ! -f "$LINUXDEPLOY" ]; then
    echo "Downloading linuxdeploy…"
    curl -L -o "$BUILD_DIR/linuxdeploy" \
        "https://github.com/linuxdeploy/linuxdeploy/releases/download/continuous/linuxdeploy-x86_64.AppImage"
    chmod +x "$BUILD_DIR/linuxdeploy"
    LINUXDEPLOY="$BUILD_DIR/linuxdeploy"
fi
export OUTPUT="$BUILD_DIR/$APP_NAME-x86_64.AppImage"
"$LINUXDEPLOY" \
    --appdir "$APPDIR" \
    --plugin gtk \
    --output appimage || {
        echo "linuxdeploy failed; trying without GTK plugin…"
        "$LINUXDEPLOY" \
            --appdir "$APPDIR" \
            --output appimage
    }

echo ""
echo "=== AppImage built ==="
ls -la "$OUTPUT"
echo ""
echo "To use it:"
echo "  chmod +x $OUTPUT"
echo "  ./$OUTPUT"
