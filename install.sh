#!/bin/bash
set -euo pipefail

# Mithril installer — downloads the latest release binary for your platform

REPO="GiacomoSaccaggi/mithril"
INSTALL_DIR="${MITHRIL_INSTALL_DIR:-${HOME}/.local/bin}"
NO_MODIFY_PATH="${MITHRIL_NO_MODIFY_PATH:-0}"

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
case ":${PATH}:" in
    *":${INSTALL_DIR}:"*)
        IN_PATH=1
        ;;
    *)
        IN_PATH=0
        ;;
esac

detect_profile() {
    local shell_name
    shell_name=$(basename "${SHELL:-bash}")
    case "${shell_name}" in
        zsh)
            if [ -f "${HOME}/.zshrc" ] || [ ! -f "${HOME}/.zprofile" ]; then
                echo "${HOME}/.zshrc"
            else
                echo "${HOME}/.zprofile"
            fi
            ;;
        bash)
            if [ -f "${HOME}/.bashrc" ]; then
                echo "${HOME}/.bashrc"
            elif [ -f "${HOME}/.bash_profile" ]; then
                echo "${HOME}/.bash_profile"
            else
                echo "${HOME}/.bashrc"
            fi
            ;;
        *)
            if [ -f "${HOME}/.profile" ]; then
                echo "${HOME}/.profile"
            else
                echo "${HOME}/.bashrc"
            fi
            ;;
    esac
}

if [ "${IN_PATH}" -eq 0 ]; then
    EXPORT_LINE="export PATH=\"${INSTALL_DIR}:\${PATH}\""
    if [ "${NO_MODIFY_PATH}" = "1" ] || [ "${NO_MODIFY_PATH}" = "true" ]; then
        echo "  ⚠️  ${INSTALL_DIR} is not in your PATH (MITHRIL_NO_MODIFY_PATH is set)."
        echo "  Add this to your shell profile:"
        echo ""
        echo "    ${EXPORT_LINE}"
        echo ""
    else
        PROFILE_FILE=$(detect_profile)
        if [ -f "${PROFILE_FILE}" ] && grep -Fqs "${INSTALL_DIR}" "${PROFILE_FILE}"; then
            echo "  ℹ️  ${INSTALL_DIR} is already configured in ${PROFILE_FILE}"
        else
            if (mkdir -p "$(dirname "${PROFILE_FILE}")" && touch "${PROFILE_FILE}" && [ -w "${PROFILE_FILE}" ]) 2>/dev/null; then
                echo "" >> "${PROFILE_FILE}"
                echo "# Added by Mithril installer" >> "${PROFILE_FILE}"
                echo "${EXPORT_LINE}" >> "${PROFILE_FILE}"
                echo "  ✨ Added ${INSTALL_DIR} to PATH in ${PROFILE_FILE}"
                echo "  To start using Mithril immediately in your current terminal session, run:"
                echo ""
                echo "    ${EXPORT_LINE}"
                echo ""
            else
                echo "  ⚠️  ${INSTALL_DIR} is not in your PATH."
                echo "  Add this to your shell profile:"
                echo ""
                echo "    ${EXPORT_LINE}"
                echo ""
            fi
        fi
    fi
fi

# Verify
echo "  🔍 Verifying installation..."
if "${INSTALL_DIR}/mithril" --version > /dev/null 2>&1; then
    VERSION_OUTPUT=$("${INSTALL_DIR}/mithril" --version)
    echo "  🎉 ${VERSION_OUTPUT}"
elif command -v mithril &> /dev/null; then
    echo "  🎉 $(mithril --version)"
else
    echo "  Run: ${INSTALL_DIR}/mithril --version"
fi
