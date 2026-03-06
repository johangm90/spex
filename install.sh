#!/bin/sh
# spex installer — macOS and Linux
#
# Usage:
#   curl -fsSL https://github.com/johangm90/spex/releases/latest/download/install.sh | sh
#
# Options:
#   --prefix <dir>   Install to <dir>/bin  (default: ~/.local/bin, no sudo needed)
#
# Environment:
#   SPEX_VERSION     Pin a specific version  (default: latest)
#   SPEX_REPO        Override GitHub repo    (default: johangm90/spex)
#
# Examples:
#   # No-permission-needed install (default):
#   curl -fsSL .../install.sh | sh
#
#   # System-wide install (requires sudo):
#   curl -fsSL .../install.sh | sh -s -- --prefix /usr/local

set -e

REPO="${SPEX_REPO:-johangm90/spex}"
BINARY="spex"
PREFIX=""

# ── Parse arguments ───────────────────────────────────────────────────────────

while [ $# -gt 0 ]; do
  case "$1" in
    --prefix=*) PREFIX="${1#--prefix=}" ;;
    --prefix)   shift; PREFIX="$1" ;;
  esac
  shift
done

# ── Detect platform ───────────────────────────────────────────────────────────

OS="$(uname -s)"
ARCH="$(uname -m)"

case "${OS}" in
  Linux)
    case "${ARCH}" in
      x86_64)        TARGET="x86_64-unknown-linux-gnu" ;;
      aarch64|arm64) TARGET="aarch64-unknown-linux-gnu" ;;
      *) echo "error: unsupported Linux architecture: ${ARCH}" >&2; exit 1 ;;
    esac
    ;;
  Darwin)
    case "${ARCH}" in
      x86_64) TARGET="x86_64-apple-darwin" ;;
      arm64)  TARGET="aarch64-apple-darwin" ;;
      *) echo "error: unsupported macOS architecture: ${ARCH}" >&2; exit 1 ;;
    esac
    ;;
  *)
    echo "error: unsupported operating system: ${OS}" >&2
    exit 1
    ;;
esac

# ── Resolve install directory ─────────────────────────────────────────────────
#
# Priority:
#   1. --prefix <dir> supplied by user  → <dir>/bin  (user's choice, no fallback)
#   2. No --prefix                      → ~/.local/bin  (no sudo needed)
#
# System-wide installs (/usr/local/bin, /usr/bin) require the user to pass
# --prefix explicitly; we never call sudo automatically.

if [ -n "${PREFIX}" ]; then
  BIN_DIR="${PREFIX}/bin"
  USE_SUDO=false
  # If the directory isn't writable, warn and let the cp fail naturally.
  if [ ! -w "${PREFIX}" ] && [ -d "${PREFIX}" ]; then
    echo "warning: ${PREFIX} is not writable by the current user." >&2
    echo "         Re-run with sudo, or omit --prefix to install to ~/.local/bin" >&2
  fi
else
  BIN_DIR="${HOME}/.local/bin"
  USE_SUDO=false
fi

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

# ── Download ──────────────────────────────────────────────────────────────────

ARCHIVE="spex-${VERSION}-${TARGET}.tar.gz"
URL="https://github.com/${REPO}/releases/download/${VERSION}/${ARCHIVE}"
CHECKSUM_URL="${URL}.sha256"

TMPDIR="$(mktemp -d)"
trap 'rm -rf "${TMPDIR}"' EXIT

echo "Downloading spex ${VERSION} for ${TARGET}…"
curl -fsSL "${URL}" -o "${TMPDIR}/${ARCHIVE}"

echo "Verifying checksum…"
curl -fsSL "${CHECKSUM_URL}" -o "${TMPDIR}/${ARCHIVE}.sha256"
(cd "${TMPDIR}" && sha256sum -c "${ARCHIVE}.sha256" 2>/dev/null) || \
(cd "${TMPDIR}" && shasum -a 256 -c "${ARCHIVE}.sha256")

# ── Install ───────────────────────────────────────────────────────────────────

echo "Installing to ${BIN_DIR}…"
tar -xzf "${TMPDIR}/${ARCHIVE}" -C "${TMPDIR}"
mkdir -p "${BIN_DIR}"
cp "${TMPDIR}/spex-${VERSION}-${TARGET}/${BINARY}" "${BIN_DIR}/${BINARY}"
chmod +x "${BIN_DIR}/${BINARY}"

# ── PATH hint ─────────────────────────────────────────────────────────────────

IN_PATH=false
case ":${PATH}:" in
  *":${BIN_DIR}:"*) IN_PATH=true ;;
esac

echo ""
echo "✓ spex ${VERSION} installed to ${BIN_DIR}/${BINARY}"

if [ "${IN_PATH}" = "false" ]; then
  echo ""
  echo "  Add ${BIN_DIR} to your PATH:"
  echo ""

  SHELL_NAME="$(basename "${SHELL:-sh}")"
  case "${SHELL_NAME}" in
    zsh)  RC_FILE="\$HOME/.zshrc" ;;
    fish) RC_FILE="\$HOME/.config/fish/config.fish" ;;
    *)    RC_FILE="\$HOME/.bashrc" ;;
  esac

  echo "    echo 'export PATH=\"${BIN_DIR}:\$PATH\"' >> ${RC_FILE}"
  echo "    source ${RC_FILE}"
fi

# ── Get started ───────────────────────────────────────────────────────────────

echo ""
echo "Get started:"
echo "  spex setup        # install agent skills (one-time)"
echo "  spex init         # initialise spex in an existing project"
echo "  spex new myapp    # create a new project"
