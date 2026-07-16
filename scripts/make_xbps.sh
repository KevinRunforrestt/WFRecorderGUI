#!/bin/sh
# make_xbps.sh — Build a native .xbps package using xbps-src (the official
# Void Linux build system).
#
# This script automates the full process:
#   1. Clones void-packages (if not already present) — as normal user
#   2. Copies the wf-recorder-gui template into srcpkgs/ — as normal user
#   3. Copies the source tree into the build directory — as normal user
#   4. Runs `xbps-src pkg wf-recorder-gui` — as normal user (xbps-src
#      uses Linux capabilities + user namespaces, NEVER root)
#   5. Copies the resulting .xbps to target/ — as normal user
#   6. Optionally installs with xbps-install (this DOES need doas/sudo)
#
# IMPORTANT: xbps-src CANNOT run as root. The script refuses to run if
# invoked with sudo/doas or as root user. Only the final --install step
# uses doas/sudo.
#
# Requirements:
#   - Void Linux (xbps-src only works on Void)
#   - git (for cloning void-packages)
#   - ~2 GB free disk (for the void-packages clone + chroot)
#   - Your user must have permission to use xbps-src (any Void user can)
#
# Usage:
#   ./make_xbps.sh                # build + copy .xbps to target/
#   ./make_xbps.sh --install      # build + install with xbps-install
#   ./make_xbps.sh --clean        # remove the void-packages clone after build

set -eu

APP_NAME="wf-recorder-gui"
APP_ID="io.github.wf-recorder-gui"
ARCH="x86_64"

cd "$(dirname "$0")/.."
PROJECT_ROOT="$(pwd)"
BUILD_DIR="$PROJECT_ROOT/target/xbps"
VOID_PACKAGES_DIR="${VOID_PACKAGES_DIR:-$BUILD_DIR/void-packages}"

# ---------------------------------------------------------------------------
# REFUSE to run as root or under sudo/doas. xbps-src uses chroot and
# user namespaces, which break when run as root. The script must be
# invoked as a normal user.
# ---------------------------------------------------------------------------
if [ "$(id -u)" = "0" ]; then
    echo "ERROR: This script must NOT be run as root."
    echo "xbps-src uses chroot and user namespaces that break under root."
    echo ""
    echo "Run it as your normal user:"
    echo "  ./make_xbps.sh"
    echo ""
    echo "Only the final install step (--install) will prompt for doas/sudo."
    exit 1
fi

# Detect if running under sudo/doas (SUDO_USER or DOAS_USER env vars are set)
if [ -n "${SUDO_USER:-}" ] || [ -n "${DOAS_USER:-}" ]; then
    echo "ERROR: This script must NOT be run under sudo or doas."
    echo "xbps-src cannot run as root, even via sudo/doas."
    echo ""
    echo "Run it as your normal user (without sudo/doas):"
    echo "  ./make_xbps.sh"
    exit 1
fi

# Parse flags
DO_INSTALL=0
DO_CLEAN=0
for arg in "$@"; do
    case "$arg" in
        --install) DO_INSTALL=1 ;;
        --clean) DO_CLEAN=1 ;;
        *) echo "Unknown flag: $arg"; exit 1 ;;
    esac
done

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
# Sanity checks
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
    # Update to latest
    echo "  updating…"
    cd "$VOID_PACKAGES_DIR"
    git pull --ff-only 2>/dev/null || true
    cd "$PROJECT_ROOT"
fi

cd "$VOID_PACKAGES_DIR"

# ---------------------------------------------------------------------------
# Step 2: Bootstrap the chroot (only needed once).
# xbps-src handles this itself — it uses user namespaces and capabilities,
# NO root required. It creates masterdir/ and hostdir/ under the user's
# ownership.
# ---------------------------------------------------------------------------
if [ ! -d masterdir ] || [ ! -f masterdir/.xbps_chroot_init ]; then
    echo ""
    echo "=== Bootstrapping xbps-src chroot ==="
    echo "  (this takes ~5-10 minutes the first time)"
    echo "  running as user: $(whoami)"
    ./xbps-src binary-bootstrap
else
    echo ""
    echo "=== Chroot already bootstrapped ==="
fi

# ---------------------------------------------------------------------------
# Step 3: Copy template into srcpkgs/ (as normal user)
# ---------------------------------------------------------------------------
echo ""
echo "=== Installing template ==="
TEMPLATE_SRC="$PROJECT_ROOT/scripts/void-template"
TEMPLATE_DST="$VOID_PACKAGES_DIR/srcpkgs/$APP_NAME"
rm -rf "$TEMPLATE_DST"
cp -r "$TEMPLATE_SRC" "$TEMPLATE_DST"
echo "  copied template to $TEMPLATE_DST"

# Read version from template
VERSION="$(grep '^version=' "$TEMPLATE_DST/template" | head -1 | sed 's/version=//; s/\"//g')"
PKGVER="${APP_NAME}-${VERSION}"
TARBALL_NAME="${PKGVER}.tar.gz"

# ---------------------------------------------------------------------------
# Step 4: Create a source tarball and place it in xbps-src's distfiles dir.
#
# xbps-src with `distfiles="filename.tar.gz"` looks for the file in:
#   $XBPS_SRCDISTDIR/<pkgname>-<version>/<filename>
# which by default is:
#   void-packages/hostdir/sources/<pkgname>-<version>/<filename>
#
# IMPORTANT: xbps-src does NOT accept checksum=SKIP for local files —
# it purges them and then fails to fetch. We must compute the actual
# sha256 and inject it into the template before building.
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

# Create tarball from project root, excluding build artifacts.
# The tarball contains the source as-is (Cargo.toml at the root).
# tar is portable and supports --exclude; no rsync needed.
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

# Compute sha256 of the tarball and inject it into the template.
# xbps-src requires the real checksum; SKIP doesn't work for local files.
SHA256="$(sha256sum "$TARBALL_PATH" | awk '{print $1}')"
echo "  sha256: $SHA256"
sed -i "s|__SHA256_PLACEHOLDER__|${SHA256}|" "$TEMPLATE_DST/template"
echo "  injected checksum into template"

# ---------------------------------------------------------------------------
# Step 5: Build the package with xbps-src (as normal user — NO root!)
#
# We use -f (force) to ensure a clean rebuild from scratch. Without -f,
# xbps-src may reuse a stale ${DESTDIR} from a previous failed build,
# causing "binary already exists in destination" errors during cargo install.
# ---------------------------------------------------------------------------
echo ""
echo "=== Building package with xbps-src ==="
echo "  (this takes ~3-5 minutes for a Rust project)"
echo "  running as user: $(whoami)"
echo "  using -f (force clean rebuild)"
./xbps-src -H "$VOID_PACKAGES_DIR/hostdir" -f pkg "$APP_NAME"

# ---------------------------------------------------------------------------
# Step 6: Find and copy the resulting .xbps (as normal user)
# ---------------------------------------------------------------------------
XBPS_FILE="$(ls "$VOID_PACKAGES_DIR/hostdir/binpkgs/${APP_NAME}"-*.${ARCH}.xbps 2>/dev/null | head -1 || true)"
if [ -z "$XBPS_FILE" ]; then
    # Try without arch suffix
    XBPS_FILE="$(ls "$VOID_PACKAGES_DIR/hostdir/binpkgs/${APP_NAME}"-*.xbps 2>/dev/null | head -1 || true)"
fi
if [ -z "$XBPS_FILE" ]; then
    echo "ERROR: .xbps file not found after build."
    echo "Check $VOID_PACKAGES_DIR/hostdir/binpkgs/"
    exit 1
fi

mkdir -p "$BUILD_DIR"
cp "$XBPS_FILE" "$BUILD_DIR/"
PKG_FILE="$BUILD_DIR/$(basename "$XBPS_FILE")"

echo ""
echo "=== Package built ==="
ls -la "$PKG_FILE"

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
        echo "  # or install doas/sudo and re-run with --install"
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
echo "Package: $PKG_FILE"
echo ""
echo "To install on any Void Linux x86_64 system:"
echo "  doas xbps-install $PKG_FILE"
echo "  # or with sudo:"
echo "  sudo xbps-install $PKG_FILE"
echo ""
echo "To remove the void-packages clone (saves ~2 GB):"
echo "  ./make_xbps.sh --clean"
