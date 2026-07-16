#!/bin/sh
# build_static_musl.sh — Build a static-ish musl binary of wf-recorder-gui.
#
# IMPORTANT: A truly 100% static GTK3 binary is impossible because GTK3 uses
# dlopen() for many plugins (IM modules, pixbuf loaders, themes). What this
# script produces instead is:
#
#   - A binary linked against musl libc (no glibc dependency at all)
#   - GTK3 and friends linked statically where possible, dynamically otherwise
#   - A small runtime wrapper that sets GDK_PIXBUF_MODULE_FILE,
#     GTK_IM_MODULE_FILE, etc. so the binary works on a clean target system
#
# The result is a single ~10-15 MB binary that runs on any x86_64 Linux
# with musl, plus a few .so files for GTK3's plugin loaders (bundled in
# a tarball alongside the binary).
#
# Requirements (installed by install_deps_void.sh musl):
#   - Rust with x86_64-unknown-linux-musl target
#   - musl-dev, musl GTK3 -dev packages (Void Linux provides these)
#
# Usage:
#   ./build_static_musl.sh

set -eu

cd "$(dirname "$0")/.."
PROJECT_ROOT="$(pwd)"
APP_NAME="wf-recorder-gui"
OUTPUT_DIR="$PROJECT_ROOT/target/musl"
TARBALL="$PROJECT_ROOT/target/${APP_NAME}-musl-x86_64.tar.gz"

echo "=== Checking musl target ==="
if ! rustup target list --installed | grep -q x86_64-unknown-linux-musl; then
    echo "ERROR: x86_64-unknown-linux-musl target not installed."
    echo "Run: rustup target add x86_64-unknown-linux-musl"
    echo "Or: ./scripts/install_deps_void.sh musl"
    exit 1
fi

# On Void Linux, the musl GTK3 dev packages install to /usr/lib/musl/ and
# pkg-config files are tagged with musl in their name. We set PKG_CONFIG_PATH
# to find them.
MUSL_LIBDIR="${MUSL_LIBDIR:-/usr/lib/musl}"
if [ -d "$MUSL_LIBDIR/pkgconfig" ]; then
    export PKG_CONFIG_PATH="$MUSL_LIBDIR/pkgconfig:${PKG_CONFIG_PATH:-}"
fi

# Make sure pkg-config uses the musl sysroot.
export PKG_CONFIG_ALLOW_SYSTEM_CFLAGS=1
export PKG_CONFIG_ALLOW_SYSTEM_LIBS=1

# Static linking flags for musl.
export RUSTFLAGS="${RUSTFLAGS:-} -C target-feature=+crt-static -C link-arg=-static"

# Some crates need the C compiler to use musl.
export CC_x86_64_unknown_linux_musl="${CC_musl:-musl-gcc}"
export CXX_x86_64_unknown_linux_musl="${CXX_musl:-musl-g++}"
export AR_x86_64_unknown_linux_musl="${AR_musl:-ar}"

echo ""
echo "=== Verifying musl environment ==="
echo "PKG_CONFIG_PATH=$PKG_CONFIG_PATH"
echo "CC=$CC_x86_64_unknown_linux_musl"
echo "RUSTFLAGS=$RUSTFLAGS"
echo ""
echo "GTK3 version: $(pkg-config --modversion gtk+-3.0 2>/dev/null || echo 'NOT FOUND')"

echo ""
echo "=== Building musl binary ==="
cargo build --release --target x86_64-unknown-linux-musl

BINARY="$PROJECT_ROOT/target/x86_64-unknown-linux-musl/release/$APP_NAME"
if [ ! -f "$BINARY" ]; then
    echo "ERROR: musl binary not found at $BINARY"
    exit 1
fi

echo ""
echo "=== Packaging ==="
rm -rf "$OUTPUT_DIR"
mkdir -p "$OUTPUT_DIR/bin"
cp "$BINARY" "$OUTPUT_DIR/bin/$APP_NAME"

# Bundle the GTK3 pixbuf loaders and IM modules that the binary will need
# at runtime. These are small .so files.
mkdir -p "$OUTPUT_DIR/lib/gdk-pixbuf-2.0/2.10.0/loaders"
mkdir -p "$OUTPUT_DIR/lib/gtk-3.0/3.0.0/immodules"

# Try to copy the loaders (best-effort; depends on the host distro).
LOADER_DIR=$(pkg-config --variable=gdk_pixbuf_moduledir gdk-pixbuf-2.0 2>/dev/null || echo "")
if [ -n "$LOADER_DIR" ] && [ -d "$LOADER_DIR" ]; then
    cp "$LOADER_DIR"/*.so "$OUTPUT_DIR/lib/gdk-pixbuf-2.0/2.10.0/loaders/" 2>/dev/null || true
fi

IMMODULE_DIR=$(pkg-config --variable=gtk_immodules gtk+-3.0 2>/dev/null || echo "")
if [ -n "$IMMODULE_DIR" ] && [ -d "$IMMODULE_DIR" ]; then
    cp "$IMMODULE_DIR"/*.so "$OUTPUT_DIR/lib/gtk-3.0/3.0.0/immodules/" 2>/dev/null || true
fi

# Write a launcher script that sets up the runtime env so the binary finds
# its bundled modules.
cat > "$OUTPUT_DIR/$APP_NAME.sh" <<EOF
#!/bin/sh
# Launcher for the musl-static build of $APP_NAME.
HERE="\$(dirname "\$(readlink -f "\$0")")"
export GTK_CSD=0
export GDK_PIXBUF_MODULE_FILE="\$HERE/lib/gdk-pixbuf-2.0/2.10.0/loaders.cache"
export GTK_IM_MODULE_FILE="\$HERE/lib/gtk-3.0/3.0.0/immodules.cache"
export XDG_DATA_DIRS="\$HERE/share:\${XDG_DATA_DIRS:-}"
export LD_LIBRARY_PATH="\$HERE/lib:\${LD_LIBRARY_PATH:-}"
exec "\$HERE/bin/$APP_NAME" "\$@"
EOF
chmod +x "$OUTPUT_DIR/$APP_NAME.sh"

# Generate the loaders.cache file if gdk-pixbuf-query-loaders is available.
if command -v gdk-pixbuf-query-loaders >/dev/null 2>&1; then
    gdk-pixbuf-query-loaders "$OUTPUT_DIR/lib/gdk-pixbuf-2.0/2.10.0/loaders"/*.so \
        > "$OUTPUT_DIR/lib/gdk-pixbuf-2.0/2.10.0/loaders.cache" 2>/dev/null || true
fi
if command -v gtk-query-immodules-3.0 >/dev/null 2>&1; then
    gtk-query-immodules-3.0 "$OUTPUT_DIR/lib/gtk-3.0/3.0.0/immodules"/*.so \
        > "$OUTPUT_DIR/lib/gtk-3.0/3.0.0/immodules.cache" 2>/dev/null || true
fi

# Copy data files (icons, desktop file, presets).
mkdir -p "$OUTPUT_DIR/share/icons/hicolor/scalable/apps"
mkdir -p "$OUTPUT_DIR/share/applications"
if [ -f "$PROJECT_ROOT/data/icons/io.github.wf-recorder-gui.svg" ]; then
    cp "$PROJECT_ROOT/data/icons/io.github.wf-recorder-gui.svg" \
       "$OUTPUT_DIR/share/icons/hicolor/scalable/apps/"
fi
cat > "$OUTPUT_DIR/share/applications/io.github.wf-recorder-gui.desktop" <<EOF
[Desktop Entry]
Type=Application
Name=WFRecorderGUI
Comment=GTK3 GUI for wf-recorder on wlroots compositors
Exec=wf-recorder-gui.sh
Icon=io.github.wf-recorder-gui
Categories=AudioVideo;Video;Recorder;
Terminal=false
EOF

# Create the tarball.
tar -czf "$TARBALL" -C "$OUTPUT_DIR" .

echo ""
echo "=== Musl build complete ==="
ls -la "$BINARY"
echo ""
echo "Tarball: $TARBALL"
ls -la "$TARBALL"
echo ""
echo "To install (use doas instead of sudo if that's what you have):"
echo "  sudo tar -xzf $TARBALL -C /opt/$APP_NAME"
echo "  sudo ln -sf /opt/$APP_NAME/$APP_NAME.sh /usr/local/bin/$APP_NAME"
echo "  # or with doas:"
echo "  doas tar -xzf $TARBALL -C /opt/$APP_NAME"
echo "  doas ln -sf /opt/$APP_NAME/$APP_NAME.sh /usr/local/bin/$APP_NAME"
echo ""
echo "Or run directly:"
echo "  $OUTPUT_DIR/$APP_NAME.sh"
