#!/bin/bash
set -euo pipefail

# Mithril installer — downloads the latest release binary for your platform

REPO="GiacomoSaccaggi/mithril"
INSTALL_DIR="${MITHRIL_INSTALL_DIR:-${HOME}/.local/bin}"

# Detect OS and architecture
OS=$(uname -s | tr '[:upper:]' '[:lower:]')
ARCH=$(uname -m)

case "${OS}" in
    darwin) PLATFORM="macos" ;;
    linux)  PLATFORM="linux" ;;
    *)      echo "Unsupported OS: ${OS}"; exit 1 ;;
esac

case "${ARCH}" in
    arm64|aarch64) ARTIFACT="mithril-${PLATFORM}-arm64" ;;
    x86_64|amd64)  ARTIFACT="mithril-${PLATFORM}-x64" ;;
    *)             echo "Unsupported architecture: ${ARCH}"; exit 1 ;;
esac

echo "  ⚒️  Installing Mithril (${PLATFORM}/${ARCH})..."

# Get latest release URL
DOWNLOAD_URL="https://github.com/${REPO}/releases/latest/download/${ARTIFACT}.tar.gz"

# Create install directory
mkdir -p "${INSTALL_DIR}"

# Download and extract
TMPDIR=$(mktemp -d)
trap "rm -rf ${TMPDIR}" EXIT

echo "  ↓  Downloading from ${DOWNLOAD_URL}..."
curl -fsSL "${DOWNLOAD_URL}" -o "${TMPDIR}/mithril.tar.gz"

echo "  ✓  Extracting..."
tar xzf "${TMPDIR}/mithril.tar.gz" -C "${TMPDIR}"
chmod +x "${TMPDIR}/mithril"
mv "${TMPDIR}/mithril" "${INSTALL_DIR}/mithril"

echo ""
echo "  ✅ Mithril installed to ${INSTALL_DIR}/mithril"
echo ""

# Check if install dir is in PATH
if ! echo "${PATH}" | grep -q "${INSTALL_DIR}"; then
    echo "  ⚠️  ${INSTALL_DIR} is not in your PATH."
    echo "  Add this to your shell profile:"
    echo ""
    echo "    export PATH=\"${INSTALL_DIR}:\${PATH}\""
    echo ""
fi

# Verify
if command -v mithril &> /dev/null; then
    echo "  $(mithril --version)"
else
    echo "  Run: ${INSTALL_DIR}/mithril --version"
fi
