#!/bin/sh
# pointer-cli installer.
#
# Fully offline: this script never downloads anything. It expects to sit next to
# the `pointer-cli` binary, exactly as shipped inside
# pointer-cli-linux-x64.tar.gz:
#
#   pointer-cli-linux-x64/
#     pointer-cli   statically linked musl binary
#     install.sh      this script
#     README.md
#     LICENSE
#     SHA256SUMS      checksum for ./pointer-cli
#
# Usage:
#   ./install.sh [--prefix DIR] [--bindir DIR]
#
# Environment:
#   PREFIX    install prefix; the binary goes to $PREFIX/bin (default /usr/local)
#   BINDIR    install the binary straight into DIR (overrides $PREFIX/bin)
#   DESTDIR   staging root prepended to the final path (packaging use)

set -eu

PREFIX="${PREFIX:-/usr/local}"
BINDIR="${BINDIR:-}"
DESTDIR="${DESTDIR:-}"

usage() {
  cat <<'EOF'
pointer-cli installer (offline)

Usage: ./install.sh [options]

Options:
  --prefix DIR   Install prefix; the binary goes to DIR/bin (default: /usr/local)
  --bindir DIR   Install the binary into DIR directly (overrides --prefix)
  -h, --help     Show this help

Environment:
  PREFIX    same as --prefix
  BINDIR    same as --bindir
  DESTDIR   staging root prepended to the final path

Examples:
  sudo ./install.sh
  ./install.sh --prefix "$HOME/.local"
  DESTDIR=/tmp/pkg ./install.sh --prefix /usr
EOF
}

while [ "$#" -gt 0 ]; do
  case "$1" in
    --prefix)
      [ "$#" -ge 2 ] || { echo "install.sh: --prefix requires a value" >&2; exit 2; }
      PREFIX="$2"; shift 2 ;;
    --prefix=*) PREFIX="${1#*=}"; shift ;;
    --bindir)
      [ "$#" -ge 2 ] || { echo "install.sh: --bindir requires a value" >&2; exit 2; }
      BINDIR="$2"; shift 2 ;;
    --bindir=*) BINDIR="${1#*=}"; shift ;;
    -h|--help) usage; exit 0 ;;
    *) echo "install.sh: unknown argument: $1" >&2; usage >&2; exit 2 ;;
  esac
done

SRC_DIR=$(CDPATH= cd -- "$(dirname -- "$0")" && pwd)
BIN_SRC="$SRC_DIR/pointer-cli"

if [ ! -f "$BIN_SRC" ]; then
  echo "install.sh: pointer-cli binary not found next to this script ($BIN_SRC)" >&2
  exit 1
fi

# Verify the binary against the shipped SHA256SUMS. A missing checksum tool is
# tolerated (minimal images may not have one); a mismatch never is.
if [ -f "$SRC_DIR/SHA256SUMS" ]; then
  if command -v sha256sum >/dev/null 2>&1; then
    _sum_out=$(cd "$SRC_DIR" && sha256sum -c SHA256SUMS 2>&1) || {
      printf '%s\n' "$_sum_out" >&2
      echo "install.sh: checksum mismatch for pointer-cli" >&2
      exit 1
    }
    echo "install.sh: checksum OK"
  elif command -v shasum >/dev/null 2>&1; then
    _sum_out=$(cd "$SRC_DIR" && shasum -a 256 -c SHA256SUMS 2>&1) || {
      printf '%s\n' "$_sum_out" >&2
      echo "install.sh: checksum mismatch for pointer-cli" >&2
      exit 1
    }
    echo "install.sh: checksum OK"
  else
    echo "install.sh: no sha256sum/shasum on PATH — skipping checksum verification" >&2
  fi
fi

if [ -n "$BINDIR" ]; then
  TARGET_DIR="$BINDIR"
else
  TARGET_DIR="$PREFIX/bin"
fi
DEST_DIR="$DESTDIR$TARGET_DIR"

if ! mkdir -p "$DEST_DIR" 2>/dev/null; then
  echo "install.sh: cannot create $DEST_DIR — re-run with sudo, or pass --prefix/--bindir" >&2
  exit 1
fi

if ! install -m 0755 "$BIN_SRC" "$DEST_DIR/pointer-cli" 2>/dev/null; then
  cp "$BIN_SRC" "$DEST_DIR/pointer-cli"
  chmod 0755 "$DEST_DIR/pointer-cli"
fi

INSTALLED="$DEST_DIR/pointer-cli"
echo "install.sh: installed $INSTALLED"

# Sanity check: the shipped binary must be able to report its own version.
VERSION=$("$INSTALLED" --version 2>/dev/null) || VERSION=""
if [ -n "$VERSION" ]; then
  echo "install.sh: $VERSION ready"
else
  echo "install.sh: warning: the installed binary did not report a version" >&2
fi

case ":${PATH}:" in
  *":${DEST_DIR}:"*) ;;
  *)
    if [ -z "$DESTDIR" ]; then
      echo "install.sh: note: add $DEST_DIR to PATH to run 'pointer-cli' directly"
    fi
    ;;
esac
