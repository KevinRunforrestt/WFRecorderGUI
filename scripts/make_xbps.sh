#!/bin/sh
# make_xbps.sh — Build a native .xbps package using xbps-src (the official
# Void Linux build system).
#
# Supports two build modes:
#   --glibc  (default): standard glibc build, produces a dynamically linked
#            binary that requires the glibc system libraries.
#   --musl:  builds inside a musl chroot via xbps-src -A x86_64-musl.
#            Produces a dynamically linked binary for Void musl systems.
#            GTK3 plugin modules (pixbuf loaders, IM modules) that use
#            dlopen are bundled in the package with a launcher wrapper.
#
# NOTE: Fully static linking is NOT possible with GTK3 on Void because:
#   1. Void does not ship static .a archives for GTK3, cairo, pango, etc.
#   2. GTK3 uses dlopen() internally, which requires dynamic linking.
#   The musl build produces a normal dynamic binary that runs on Void musl.
#   For Void glibc systems, use --glibc instead.
#
# IMPORTANT: xbps-src provides a FULLY ISOLATED build environment (chroot).
# No musl libraries, Rust musl target, or special toolchain are needed on
# the HOST system. Everything is installed automatically inside the chroot
# based on the template's makedepends/hostmakedepends.
#
# This script automates the full process:
#   1. Clones void-packages (if not already present) — as normal user
#   2. Bootstraps the chroot with the correct architecture — as normal user
#   3. Copies and patches the template into srcpkgs/ — as normal user
#   4. Creates a source tarball with sha256 — as normal user
#   5. Runs `xbps-src pkg wf-recorder-gui` — as normal user
#   6. Copies the resulting .xbps to target/ — as normal user
#   7. Optionally installs with xbps-install (this DOES need doas/sudo)
#
# xbps-src CANNOT run as root. The script refuses if invoked as root.
#
# Requirements (on the HOST only):
#   - Void Linux (xbps-src only works on Void)
#   - git
#   - ~2 GB free disk (for the void-packages clone + chroot)
#
# Usage:
#   ./make_xbps.sh                  # build glibc + copy .xbps to target/
#   ./make_xbps.sh --musl           # build musl + copy .xbps to target/
#   ./make_xbps.sh --install        # build glibc + install with xbps-install
#   ./make_xbps.sh --musl --install # build musl + install
#   ./make_xbps.sh --clean          # remove the void-packages clone after build

set -eu

APP_NAME="wf-recorder-gui"
APP_ID="io.github.wf-recorder-gui"

# ---------------------------------------------------------------------------
# Parse flags
# ---------------------------------------------------------------------------
DO_INSTALL=0
DO_CLEAN=0
BUILD_MODE="glibc"
for arg in "$@"; do
    case "$arg" in
        --install) DO_INSTALL=1 ;;
        --clean)   DO_CLEAN=1 ;;
        --musl)    BUILD_MODE="musl" ;;
        --glibc)   BUILD_MODE="glibc" ;;
        *) echo "Unknown flag: $arg"; exit 1 ;;
    esac
done

# Architecture: xbps-src uses x86_64-musl for musl builds.
if [ "$BUILD_MODE" = "musl" ]; then
    XBPS_ARCH="x86_64-musl"
else
    XBPS_ARCH="x86_64"
fi

cd "$(dirname "$0")/.."
PROJECT_ROOT="$(pwd)"
BUILD_DIR="$PROJECT_ROOT/target/xbps-${BUILD_MODE}"
VOID_PACKAGES_DIR="${VOID_PACKAGES_DIR:-$BUILD_DIR/void-packages}"

echo "=== Build configuration ==="
echo "  mode:       $BUILD_MODE"
echo "  arch:       $XBPS_ARCH"
echo "  project:    $PROJECT_ROOT"
echo "  output:     $BUILD_DIR"
echo ""

# ---------------------------------------------------------------------------
# REFUSE to run as root or under sudo/doas.
# ---------------------------------------------------------------------------
if [ "$(id -u)" = "0" ]; then
    echo "ERROR: This script must NOT be run as root."
    echo "xbps-src uses chroot and user namespaces that break under root."
    echo "Run it as your normal user. Only --install needs doas/sudo."
    exit 1
fi

if [ -n "${SUDO_USER:-}" ] || [ -n "${DOAS_USER:-}" ]; then
    echo "ERROR: This script must NOT be run under sudo or doas."
    echo "xbps-src cannot run as root. Run as your normal user."
    exit 1
fi

# ---------------------------------------------------------------------------
# Detect privilege escalation tool (only for the final --install step)
# ---------------------------------------------------------------------------
detect_priv() {
    if command -v doas >/dev/null 2>&1; then
        echo "doas"
    elif command -v sudo >/dev/null 2>&1; then
        echo "sudo"
    else
        echo ""
    fi
}

# ---------------------------------------------------------------------------
# Sanity checks (HOST only — xbps-src handles the rest inside the chroot)
# ---------------------------------------------------------------------------
if ! command -v git >/dev/null 2>&1; then
    echo "ERROR: git not found. Install it: xbps-install -y git"
    exit 1
fi

if ! command -v xbps-uhelper >/dev/null 2>&1; then
    echo "ERROR: This doesn't look like Void Linux (xbps-uhelper not found)."
    echo "xbps-src only works on Void Linux."
    exit 1
fi

# ---------------------------------------------------------------------------
# Step 1: Clone void-packages (as normal user)
# ---------------------------------------------------------------------------
if [ ! -d "$VOID_PACKAGES_DIR" ] || [ ! -f "$VOID_PACKAGES_DIR/xbps-src" ]; then
    echo "=== Cloning void-packages ==="
    mkdir -p "$BUILD_DIR"
    git clone --depth=1 https://github.com/void-linux/void-packages.git "$VOID_PACKAGES_DIR"
    echo "  cloned to $VOID_PACKAGES_DIR"
else
    echo "=== void-packages already present ==="
    echo "  $VOID_PACKAGES_DIR"
    echo "  updating…"
    cd "$VOID_PACKAGES_DIR"
    git pull --ff-only 2>/dev/null || true
    cd "$PROJECT_ROOT"
fi

cd "$VOID_PACKAGES_DIR"

# ---------------------------------------------------------------------------
# Step 2: Bootstrap the chroot (only needed once).
#
# xbps-src creates an isolated chroot with the target architecture.
# For --musl, the chroot is a complete musl-based Void system — no musl
# libraries or Rust musl target are needed on the HOST.
# ---------------------------------------------------------------------------
NEED_BOOTSTRAP=0
if [ ! -d masterdir ] || [ ! -f masterdir/.xbps_chroot_init ]; then
    NEED_BOOTSTRAP=1
else
    # Check if the existing chroot matches the requested architecture.
    # If we have a glibc chroot but want musl (or vice versa), re-bootstrap.
    CHROOT_ARCH=""
    if [ -f masterdir/.xbps_chroot_init ]; then
        CHROOT_ARCH="$(cat masterdir/.xbps_chroot_init 2>/dev/null || true)"
    fi
    if [ -n "$CHROOT_ARCH" ] && [ "$CHROOT_ARCH" != "$XBPS_ARCH" ]; then
        echo ""
        echo "=== Chroot arch mismatch ==="
        echo "  existing: $CHROOT_ARCH"
        echo "  requested: $XBPS_ARCH"
        echo "  Re-bootstrapping…"
        NEED_BOOTSTRAP=1
        # Clean the old masterdir so binary-bootstrap starts fresh
        rm -rf masterdir
    fi
fi

if [ "$NEED_BOOTSTRAP" = "1" ]; then
    echo ""
    echo "=== Bootstrapping xbps-src chroot ($XBPS_ARCH) ==="
    echo "  (this takes ~5-10 minutes the first time)"
    echo "  running as user: $(whoami)"
    ./xbps-src -A "$XBPS_ARCH" binary-bootstrap
else
    echo ""
    echo "=== Chroot already bootstrapped ($XBPS_ARCH) ==="
fi

# ---------------------------------------------------------------------------
# Step 3: Copy template into srcpkgs/ and patch for musl if needed.
#
# For musl builds we patch the template to:
#   - Override do_install() to bundle GTK3 runtime modules
#
# The build itself is a NORMAL native build inside the musl chroot —
# no special RUSTFLAGS, no --target override. xbps-src's cargo build
# style handles everything correctly, including --target ${RUST_TARGET}
# which puts the binary in target/${RUST_TARGET}/release/.
#
# Why NOT static linking:
#   - GTK3, cairo, pango, harfbuzz, etc. do NOT have .a archives in Void
#   - GTK3 uses dlopen() internally (pixbuf loaders, IM modules)
#   - +crt-static makes the linker use -Bstatic for ALL native libs → fails
#   There is no way to link GTK3 statically on Void. Period.
# ---------------------------------------------------------------------------
echo ""
echo "=== Installing template ==="
TEMPLATE_SRC="$PROJECT_ROOT/scripts/void-template"
TEMPLATE_DST="$VOID_PACKAGES_DIR/srcpkgs/$APP_NAME"
rm -rf "$TEMPLATE_DST"
cp -r "$TEMPLATE_SRC" "$TEMPLATE_DST"
echo "  copied template to $TEMPLATE_DST"

if [ "$BUILD_MODE" = "musl" ]; then
    echo "  patching template for musl build…"

    cat >> "$TEMPLATE_DST/template" <<'TEMPLATE_MUSL_PATCH'

# === MUSL BUILD OVERRIDES (injected by make_xbps.sh --musl) ===
#
# We are building inside a musl chroot (xbps-src -A x86_64-musl), so the
# toolchain is already musl-native. The chroot already has:
#   - musl as the system libc (musl-devel is part of the base chroot)
#   - Rust with x86_64-unknown-linux-musl as the default host triple
#   - GTK3 dev headers in the standard /usr/lib/pkgconfig paths
#   - gcc/cc as the native musl compiler
#
# This is a NORMAL native build — no static linking flags.
# Static linking is NOT possible with GTK3 on Void because:
#   1. Void does not ship static .a archives for GTK3, cairo, pango, etc.
#   2. GTK3 uses dlopen() internally, requiring dynamic linking.
#   3. +crt-static makes the linker pass -Bstatic for ALL native libs,
#      which fails because the .a files don't exist.
#
# We only override do_install() to bundle GTK3 runtime modules and
# provide a launcher wrapper. The default do_build() from build_style=cargo
# handles the compilation correctly as a native musl build.
#
# NOTE: xbps-src's cargo build style uses --target ${RUST_TARGET} when
# building for non-glibc architectures. For x86_64-musl, RUST_TARGET is
# x86_64-unknown-linux-musl, so the binary ends up in:
#   target/x86_64-unknown-linux-musl/release/
# NOT in target/release/. We must use the correct path.

# APP_NAME is not defined in the template — use pkgname (set above).
APP_NAME="${pkgname}"

do_install() {
    # Install the binary.
    # xbps-src's cargo build style sets RUST_TARGET for musl builds,
    # which puts the binary in target/${RUST_TARGET}/release/ instead
    # of target/release/. Detect the correct path automatically.
    local _bin
    if [ -n "${RUST_TARGET:-}" ] && [ -f "target/${RUST_TARGET}/release/${APP_NAME}" ]; then
        _bin="target/${RUST_TARGET}/release/${APP_NAME}"
    else
        _bin="target/release/${APP_NAME}"
    fi
    vbin "${_bin}"

    # .desktop file
    sed "s|^Exec=.*|Exec=/usr/bin/${APP_NAME}|" \
        ${FILESDIR}/io.github.wf-recorder-gui.desktop \
        > ${wrksrc}/wf-recorder-gui.desktop
    vinstall ${wrksrc}/wf-recorder-gui.desktop 644 usr/share/applications io.github.wf-recorder-gui.desktop

    # Icon (SVG)
    vinstall ${FILESDIR}/io.github.wf-recorder-gui.svg 644 usr/share/icons/hicolor/scalable/apps

    # AppStream metainfo
    vinstall ${FILESDIR}/io.github.wf-recorder-gui.appdata.xml 644 usr/share/metainfo

    # Symlink for TUI mode (busybox-style argv0 detection)
    ln -sf ${APP_NAME} ${DESTDIR}/usr/bin/wf-recorder-tui

    # License
    vlicense LICENSE

    # --- Bundle GTK3 runtime modules ---
    # GTK3 uses dlopen() for pixbuf loaders and input method modules.
    # These MUST be loaded at runtime — they cannot be statically linked.
    # We bundle them in the package and the launcher script sets the
    # environment variables so the binary finds them.

    # Bundle gdk-pixbuf loaders
    vmkdir usr/lib/${APP_NAME}/gdk-pixbuf/loaders
    local _loader_dir
    _loader_dir="$(pkg-config --variable=gdk_pixbuf_moduledir gdk-pixbuf-2.0 2>/dev/null || echo "")"
    if [ -n "$_loader_dir" ] && [ -d "$_loader_dir" ]; then
        cp "$_loader_dir"/*.so ${DESTDIR}/usr/lib/${APP_NAME}/gdk-pixbuf/loaders/ 2>/dev/null || true
    fi

    # Bundle GTK3 IM modules
    vmkdir usr/lib/${APP_NAME}/gtk-3.0/immodules
    local _immodule_dir
    _immodule_dir="$(pkg-config --variable=gtk_immodules gtk+-3.0 2>/dev/null || echo "")"
    if [ -n "$_immodule_dir" ] && [ -d "$_immodule_dir" ]; then
        cp "$_immodule_dir"/*.so ${DESTDIR}/usr/lib/${APP_NAME}/gtk-3.0/immodules/ 2>/dev/null || true
    fi

    # Generate loaders.cache and fix absolute paths
    if command -v gdk-pixbuf-query-loaders >/dev/null 2>&1; then
        gdk-pixbuf-query-loaders ${DESTDIR}/usr/lib/${APP_NAME}/gdk-pixbuf/loaders/*.so \
            > ${DESTDIR}/usr/lib/${APP_NAME}/gdk-pixbuf/loaders.cache 2>/dev/null || true
        sed -i "s|${DESTDIR}||g" ${DESTDIR}/usr/lib/${APP_NAME}/gdk-pixbuf/loaders.cache 2>/dev/null || true
    fi

    # Generate immodules.cache and fix absolute paths
    if command -v gtk-query-immodules-3.0 >/dev/null 2>&1; then
        gtk-query-immodules-3.0 ${DESTDIR}/usr/lib/${APP_NAME}/gtk-3.0/immodules/*.so \
            > ${DESTDIR}/usr/lib/${APP_NAME}/gtk-3.0/immodules.cache 2>/dev/null || true
        sed -i "s|${DESTDIR}||g" ${DESTDIR}/usr/lib/${APP_NAME}/gtk-3.0/immodules.cache 2>/dev/null || true
    fi

    # Install launcher wrapper script that sets GTK environment variables
    # so the binary can find the bundled modules at runtime.
    cat > ${wrksrc}/${APP_NAME}.sh <<LAUNCHER
#!/bin/sh
# Launcher for musl build of ${APP_NAME}.
# Sets up GTK3 runtime paths for bundled modules.
export GTK_CSD=0
export GDK_PIXBUF_MODULE_FILE="/usr/lib/${APP_NAME}/gdk-pixbuf/loaders.cache"
export GTK_IM_MODULE_FILE="/usr/lib/${APP_NAME}/gtk-3.0/immodules.cache"
export XDG_DATA_DIRS="/usr/lib/${APP_NAME}/share:\${XDG_DATA_DIRS:-/usr/local/share:/usr/share}"
exec /usr/bin/${APP_NAME} "\$@"
LAUNCHER
    vbin ${wrksrc}/${APP_NAME}.sh ${APP_NAME}-launcher
}

# Override post_install to avoid re-installing files already handled by do_install.
# The original template defines post_install() which installs .desktop, icon, etc.
# Since our do_install() already handles all of that plus the GTK3 modules, we
# skip post_install to prevent duplicate file installations.
post_install() {
    :
}
TEMPLATE_MUSL_PATCH

    echo "  template patched for musl build"
fi

# Read version from template
VERSION="$(grep '^version=' "$TEMPLATE_DST/template" | head -1 | sed 's/version=//; s/"//g')"
PKGVER="${APP_NAME}-${VERSION}"
TARBALL_NAME="${PKGVER}.tar.gz"

# ---------------------------------------------------------------------------
# Step 4: Create a source tarball and place it in xbps-src's distfiles dir.
#
# xbps-src does NOT accept checksum=SKIP for local files — we must compute
# the actual sha256 and inject it into the template before building.
# ---------------------------------------------------------------------------
DISTDIR="$VOID_PACKAGES_DIR/hostdir/sources/${PKGVER}"
TARBALL_PATH="$DISTDIR/$TARBALL_NAME"

echo ""
echo "=== Creating source tarball ==="
echo "  version: $VERSION"
echo "  pkgver: $PKGVER"
echo "  tarball: $TARBALL_PATH"

mkdir -p "$DISTDIR"
rm -f "$TARBALL_PATH"

(cd "$PROJECT_ROOT" && \
    tar czf "$TARBALL_PATH" \
        --exclude='target' \
        --exclude='.git' \
        --exclude='sysroot' \
        --exclude='presets' \
        --exclude='*.xbps' \
        --exclude='*.tmp' \
        . )
echo "  tarball created ($(du -h "$TARBALL_PATH" | cut -f1))"

SHA256="$(sha256sum "$TARBALL_PATH" | awk '{print $1}')"
echo "  sha256: $SHA256"
sed -i "s|__SHA256_PLACEHOLDER__|${SHA256}|" "$TEMPLATE_DST/template"
echo "  injected checksum into template"

# ---------------------------------------------------------------------------
# Step 5: Build the package with xbps-src (as normal user — NO root!)
#
# We use -f (force) to ensure a clean rebuild from scratch.
# For musl, -A x86_64-musl makes xbps-src create a musl chroot where
# ALL dependencies (GTK3, Rust, etc.) are the musl variants, installed
# automatically from the template's makedepends.
# ---------------------------------------------------------------------------
echo ""
echo "=== Building package with xbps-src ==="
echo "  build mode:  $BUILD_MODE"
echo "  target arch: $XBPS_ARCH"
echo "  (this takes ~3-5 minutes for a Rust project)"
echo "  running as user: $(whoami)"
echo "  using -f (force clean rebuild)"

if [ "$BUILD_MODE" = "musl" ]; then
    echo ""
    echo "  Musl build (inside isolated musl chroot):"
    echo "    - xbps-src -A x86_64-musl creates a full musl chroot"
    echo "    - All deps (GTK3, Rust, etc.) installed as musl variants"
    echo "    - Normal native build (same as other Void musl packages)"
    echo "    - GTK3 modules bundled in /usr/lib/$APP_NAME/"
    echo ""
fi

./xbps-src -A "$XBPS_ARCH" -H "$VOID_PACKAGES_DIR/hostdir" -f pkg "$APP_NAME"

# ---------------------------------------------------------------------------
# Step 6: Find and copy the resulting .xbps (as normal user)
# ---------------------------------------------------------------------------
XBPS_FILE=""

# Try with the full arch suffix first (e.g. .x86_64-musl.xbps)
XBPS_FILE="$(ls "$VOID_PACKAGES_DIR/hostdir/binpkgs/${APP_NAME}"-*.${XBPS_ARCH}.xbps 2>/dev/null | head -1 || true)"

# Fallback: try generic x86_64
if [ -z "$XBPS_FILE" ]; then
    XBPS_FILE="$(ls "$VOID_PACKAGES_DIR/hostdir/binpkgs/${APP_NAME}"-*.x86_64.xbps 2>/dev/null | head -1 || true)"
fi

# Fallback: try any .xbps
if [ -z "$XBPS_FILE" ]; then
    XBPS_FILE="$(ls "$VOID_PACKAGES_DIR/hostdir/binpkgs/${APP_NAME}"-*.xbps 2>/dev/null | head -1 || true)"
fi

if [ -z "$XBPS_FILE" ]; then
    echo "ERROR: .xbps file not found after build."
    echo "Check $VOID_PACKAGES_DIR/hostdir/binpkgs/"
    echo ""
    echo "Available packages:"
    ls -la "$VOID_PACKAGES_DIR/hostdir/binpkgs/" 2>/dev/null || echo "  (directory not found)"
    exit 1
fi

mkdir -p "$BUILD_DIR"
cp "$XBPS_FILE" "$BUILD_DIR/"
PKG_FILE="$BUILD_DIR/$(basename "$XBPS_FILE")"

echo ""
echo "=== Package built ==="
ls -la "$PKG_FILE"

# Verify the binary if possible
if [ "$BUILD_MODE" = "musl" ]; then
    echo ""
    echo "=== Musl binary verification ==="
    XBPS_EXTRACT_DIR="$BUILD_DIR/inspect"
    rm -rf "$XBPS_EXTRACT_DIR"
    mkdir -p "$XBPS_EXTRACT_DIR"
    if tar -xzf "$PKG_FILE" -C "$XBPS_EXTRACT_DIR" 2>/dev/null; then
        BINARY_IN_PKG="$(find "$XBPS_EXTRACT_DIR" -name "$APP_NAME" -type f 2>/dev/null | head -1 || true)"
        if [ -n "$BINARY_IN_PKG" ]; then
            echo "  binary: $(basename "$BINARY_IN_PKG")"
            file "$BINARY_IN_PKG" 2>/dev/null || true
            echo "  dynamic dependencies:"
            ldd "$BINARY_IN_PKG" 2>/dev/null || echo "    (statically linked or unreadable)"
        else
            echo "  (binary not found in package for inspection)"
        fi
    else
        echo "  (could not extract package for inspection)"
    fi
    rm -rf "$XBPS_EXTRACT_DIR"
fi

# ---------------------------------------------------------------------------
# Step 7: Optionally install (THIS step needs doas/sudo)
# ---------------------------------------------------------------------------
if [ "$DO_INSTALL" = "1" ]; then
    PRIV="$(detect_priv)"
    if [ -z "$PRIV" ]; then
        echo ""
        echo "WARNING: Neither doas nor sudo found."
        echo "Install the package manually:"
        echo "  xbps-install $PKG_FILE   # as root"
    else
        echo ""
        echo "=== Installing package (needs $PRIV) ==="
        $PRIV xbps-install -y "$PKG_FILE" || {
            echo "  dependency resolution failed, retrying with --force…"
            $PRIV xbps-install -yf "$PKG_FILE"
        }
        echo ""
        echo "Installed. Verify with:"
        echo "  $APP_NAME --version"
        if [ "$BUILD_MODE" = "musl" ]; then
            echo ""
            echo "Note: For the musl build, you can also use the launcher wrapper:"
            echo "  ${APP_NAME}-launcher"
            echo "This sets GTK3 environment variables for bundled modules."
        fi
    fi
fi

# ---------------------------------------------------------------------------
# Step 8: Optionally clean up the void-packages clone
# ---------------------------------------------------------------------------
if [ "$DO_CLEAN" = "1" ]; then
    echo ""
    echo "=== Cleaning up void-packages clone ==="
    rm -rf "$VOID_PACKAGES_DIR"
    echo "  removed $VOID_PACKAGES_DIR"
fi

echo ""
echo "=== Done ==="
echo "Build mode: $BUILD_MODE"
echo "Package: $PKG_FILE"
echo ""
if [ "$BUILD_MODE" = "musl" ]; then
    echo "This is a MUSL build:"
    echo "  - Built inside an isolated musl chroot (xbps-src -A x86_64-musl)"
    echo "  - Normal dynamic build (same as other Void musl GTK3 packages)"
    echo "  - Works on Void Linux x86_64 (musl)"
    echo "  - GTK3 runtime modules bundled in /usr/lib/$APP_NAME/"
    echo "  - Launcher wrapper: ${APP_NAME}-launcher"
    echo ""
    echo "NOTE: This package only works on Void musl systems."
    echo "For glibc Void, use: ./make_xbps.sh --glibc"
    echo ""
    echo "To install on a Void Linux x86_64 (musl) system:"
else
    echo "To install on any Void Linux x86_64 (glibc) system:"
fi
echo "  doas xbps-install $PKG_FILE"
echo "  # or with sudo:"
echo "  sudo xbps-install $PKG_FILE"
echo ""
echo "To remove the void-packages clone (saves ~2 GB):"
echo "  ./make_xbps.sh --clean"
echo ""
echo "To build for musl instead:"
echo "  ./make_xbps.sh --musl"
echo "  ./make_xbps.sh --musl --install"
