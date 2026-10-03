#!/usr/bin/env bash
#
# Build and package the headless pointer-cli CLI as a self-contained static
# binary, plus the offline installer, into a distributable tarball.
#
#   crates/pointer-cli/packaging/build-dist.sh
#
# Output (crates/pointer-cli/dist/):
#   pointer-cli-linux-x64.tar.gz          distributable archive
#   pointer-cli-linux-x64.tar.gz.sha256   checksum of the archive
#
# The archive contains:
#   pointer-cli-linux-x64/pointer-cli   stripped, statically linked binary
#   pointer-cli-linux-x64/install.sh      offline installer
#   pointer-cli-linux-x64/README.md       usage notes
#   pointer-cli-linux-x64/LICENSE         Apache-2.0
#   pointer-cli-linux-x64/SHA256SUMS      checksum of the binary
#
# Prerequisites (one-time):
#   rustup target add x86_64-unknown-linux-musl
#   apt-get install -y musl-tools        # provides musl-gcc

set -euo pipefail

SCRIPT_DIR=$(CDPATH= cd -- "$(dirname -- "${BASH_SOURCE[0]}")" && pwd)
ROOT=$(cd -- "$SCRIPT_DIR/../../.." && pwd)
PKG_DIR="$SCRIPT_DIR"

TARGET="x86_64-unknown-linux-musl"
ARCH="linux-x64"
NAME="pointer-cli-$ARCH"
DIST_DIR="$ROOT/crates/pointer-cli/dist"
STAGE_DIR="$DIST_DIR/$NAME"
ARCHIVE="$DIST_DIR/$NAME.tar.gz"

# The musl C toolchain is needed for the bundled SQLite and the CJK FTS
# extension; export the linker explicitly so the build does not depend on a
# committed .cargo/config.toml.
export CARGO_TARGET_X86_64_UNKNOWN_LINUX_MUSL_LINKER="${CARGO_TARGET_X86_64_UNKNOWN_LINUX_MUSL_LINKER:-musl-gcc}"
export CC_x86_64_unknown_linux_musl="${CC_x86_64_unknown_linux_musl:-musl-gcc}"
export AR_x86_64_unknown_linux_musl="${AR_x86_64_unknown_linux_musl:-ar}"

if ! command -v cargo >/dev/null 2>&1; then
  echo "build-dist: cargo not found on PATH" >&2
  exit 1
fi

if ! rustup target list --installed 2>/dev/null | grep -qx "$TARGET"; then
  echo "build-dist: rust target $TARGET is not installed." >&2
  echo "            run: rustup target add $TARGET" >&2
  exit 1
fi

if ! command -v musl-gcc >/dev/null 2>&1; then
  echo "build-dist: musl-gcc not found — install musl-tools (Debian/Ubuntu:" >&2
  echo "            apt-get install -y musl-tools)." >&2
  exit 1
fi

echo "build-dist: building pointer-cli ($TARGET, release, no-default-features)"
(
  cd "$ROOT"
  cargo build --release --target "$TARGET" -p pointer-cli --no-default-features
)

BIN="$ROOT/target/$TARGET/release/pointer-cli"
if [ ! -f "$BIN" ]; then
  echo "build-dist: expected binary missing: $BIN" >&2
  exit 1
fi

# Strip debug info to shrink the ~45 MB binary. Fall back to a no-op when no
# strip tool is available (the archive is still valid, just larger).
STRIP_BIN=""
if command -v musl-strip >/dev/null 2>&1; then
  STRIP_BIN=musl-strip
elif command -v strip >/dev/null 2>&1; then
  STRIP_BIN=strip
fi
if [ -n "$STRIP_BIN" ]; then
  "$STRIP_BIN" --strip-all "$BIN" 2>/dev/null || "$STRIP_BIN" -s "$BIN" 2>/dev/null || true
else
  echo "build-dist: no strip tool found — shipping an unstripped binary" >&2
fi

# The whole point of this package is that it runs anywhere: refuse to ship a
# dynamically linked binary.
LDD_OUT=$(ldd "$BIN" 2>&1 || true)
case "$LDD_OUT" in
  *"statically linked"*|*"not a dynamic executable"*) ;;
  *)
    echo "build-dist: refusing to package a non-static binary:" >&2
    printf '%s\n' "$LDD_OUT" >&2
    exit 1
    ;;
esac

VERSION=$("$BIN" --version)
echo "build-dist: $VERSION — static, verified"

rm -rf "$STAGE_DIR"
mkdir -p "$STAGE_DIR"
install -m 0755 "$BIN" "$STAGE_DIR/pointer-cli"
install -m 0755 "$PKG_DIR/install.sh" "$STAGE_DIR/install.sh"
install -m 0644 "$PKG_DIR/README.md" "$STAGE_DIR/README.md"
if [ -f "$ROOT/LICENSE" ]; then
  install -m 0644 "$ROOT/LICENSE" "$STAGE_DIR/LICENSE"
fi

# SHA256SUMS is relative to the archive root so install.sh can verify in place.
(
  cd "$STAGE_DIR"
  if command -v sha256sum >/dev/null 2>&1; then
    sha256sum pointer-cli > SHA256SUMS
  else
    shasum -a 256 pointer-cli > SHA256SUMS
  fi
)

rm -f "$ARCHIVE" "$ARCHIVE.sha256"
tar -czf "$ARCHIVE" -C "$DIST_DIR" "$NAME"

if command -v sha256sum >/dev/null 2>&1; then
  ( cd "$DIST_DIR" && sha256sum "$NAME.tar.gz" > "$NAME.tar.gz.sha256" )
else
  ( cd "$DIST_DIR" && shasum -a 256 "$NAME.tar.gz" > "$NAME.tar.gz.sha256" )
fi

echo
echo "build-dist: packaged"
ls -l "$ARCHIVE" "$ARCHIVE.sha256"
echo
echo "  archive  $ARCHIVE"
echo "  checksum $ARCHIVE.sha256"
