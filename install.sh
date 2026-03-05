#!/bin/sh
# spex installer — macOS and Linux
# Usage:
#   curl -fsSL https://github.com/OWNER/spex/releases/latest/download/install.sh | sh
#   curl -fsSL https://github.com/OWNER/spex/releases/latest/download/install.sh | sh -s -- --prefix /usr/local

set -e

REPO="${SPEX_REPO:-OWNER/spex}"
BINARY="spex"
PREFIX="${1:-}"

# Parse --prefix argument
while [ $# -gt 0 ]; do
  case "$1" in
    --prefix=*) PREFIX="${1#--prefix=}" ;;
    --prefix)   shift; PREFIX="$1" ;;
  esac
  shift
done

PREFIX="${PREFIX:-/usr/local}"
BIN_DIR="${PREFIX}/bin"

# ── Detect platform ───────────────────────────────────────────────────────────

OS="$(uname -s)"
ARCH="$(uname -m)"

case "${OS}" in
  Linux)
    case "${ARCH}" in
      x86_64)          TARGET="x86_64-unknown-linux-gnu" ;;
      aarch64|arm64)   TARGET="aarch64-unknown-linux-gnu" ;;
      *)
        echo "error: unsupported Linux architecture: ${ARCH}" >&2
        exit 1
        ;;
    esac
    ;;
  Darwin)
    case "${ARCH}" in
      x86_64)  TARGET="x86_64-apple-darwin" ;;
      arm64)   TARGET="aarch64-apple-darwin" ;;
      *)
        echo "error: unsupported macOS architecture: ${ARCH}" >&2
        exit 1
        ;;
    esac
    ;;
  *)
    echo "error: unsupported operating system: ${OS}" >&2
    exit 1
    ;;
esac

# ── Resolve version ───────────────────────────────────────────────────────────

if [ -n "${SPEX_VERSION:-}" ]; then
  VERSION="${SPEX_VERSION}"
else
  echo "Fetching latest release…"
  VERSION="$(curl -fsSL "https://api.github.com/repos/${REPO}/releases/latest" \
    | grep '"tag_name"' \
    | sed -E 's/.*"tag_name": *"([^"]+)".*/\1/')"
fi

if [ -z "${VERSION}" ]; then
  echo "error: could not determine latest version. Set SPEX_VERSION to override." >&2
  exit 1
fi

# ── Download and install ──────────────────────────────────────────────────────

ARCHIVE="spex-${VERSION}-${TARGET}.tar.gz"
URL="https://github.com/${REPO}/releases/download/${VERSION}/${ARCHIVE}"
CHECKSUM_URL="${URL}.sha256"

TMPDIR="$(mktemp -d)"
trap 'rm -rf "${TMPDIR}"' EXIT

echo "Downloading spex ${VERSION} for ${TARGET}…"
curl -fsSL "${URL}" -o "${TMPDIR}/${ARCHIVE}"

echo "Verifying checksum…"
curl -fsSL "${CHECKSUM_URL}" -o "${TMPDIR}/${ARCHIVE}.sha256"
(cd "${TMPDIR}" && sha256sum -c "${ARCHIVE}.sha256") || \
(cd "${TMPDIR}" && shasum -a 256 -c "${ARCHIVE}.sha256")

echo "Installing to ${BIN_DIR}…"
tar -xzf "${TMPDIR}/${ARCHIVE}" -C "${TMPDIR}"
mkdir -p "${BIN_DIR}"

# Try direct copy; fall back to sudo
if cp "${TMPDIR}/spex-${VERSION}-${TARGET}/${BINARY}" "${BIN_DIR}/${BINARY}" 2>/dev/null; then
  chmod +x "${BIN_DIR}/${BINARY}"
else
  sudo cp "${TMPDIR}/spex-${VERSION}-${TARGET}/${BINARY}" "${BIN_DIR}/${BINARY}"
  sudo chmod +x "${BIN_DIR}/${BINARY}"
fi

# ── Verify ────────────────────────────────────────────────────────────────────

if command -v spex >/dev/null 2>&1; then
  INSTALLED="$(spex --version 2>/dev/null || true)"
  echo ""
  echo "✓ ${INSTALLED:-spex} installed at ${BIN_DIR}/${BINARY}"
else
  echo ""
  echo "✓ spex ${VERSION} installed to ${BIN_DIR}/${BINARY}"
  echo "  Make sure ${BIN_DIR} is in your PATH."
fi

echo ""
echo "Get started:"
echo "  spex setup        # install agent skills (one-time)"
echo "  spex init         # initialise spex in an existing project"
echo "  spex new myapp    # create a new project"
