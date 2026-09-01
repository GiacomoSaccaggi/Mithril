#!/bin/bash
set -euo pipefail

echo "Running install.sh verification tests..."

TEST_TMP=$(mktemp -d)
trap "rm -rf ${TEST_TMP}" EXIT

# Create a mock mithril binary and tarball
MOCK_BIN_DIR="${TEST_TMP}/mock_bin"
mkdir -p "${MOCK_BIN_DIR}"
cat << 'EOF' > "${MOCK_BIN_DIR}/mithril"
#!/bin/sh
if [ "${1:-}" = "--version" ]; then
    echo "mithril 0.4.0"
    exit 0
fi
echo "mock mithril"
EOF
chmod +x "${MOCK_BIN_DIR}/mithril"

MOCK_TAR="${TEST_TMP}/mock_asset.tar.gz"
tar -czf "${MOCK_TAR}" -C "${MOCK_BIN_DIR}" mithril

# Test 1: Bash shell profile update
echo "--- Test 1: Auto PATH configuration for Bash ---"
FAKE_HOME="${TEST_TMP}/home_bash"
mkdir -p "${FAKE_HOME}"
touch "${FAKE_HOME}/.bashrc"

# We mock curl to extract our mock tarball
MOCK_CURL_DIR="${TEST_TMP}/curl_mock"
mkdir -p "${MOCK_CURL_DIR}"
cat << EOF > "${MOCK_CURL_DIR}/curl"
#!/bin/sh
cp "${MOCK_TAR}" "\$4"
EOF
chmod +x "${MOCK_CURL_DIR}/curl"

PATH="${MOCK_CURL_DIR}:/usr/bin:/bin" \
HOME="${FAKE_HOME}" \
SHELL="/bin/bash" \
bash ./install.sh

if ! grep -q "export PATH=\"${FAKE_HOME}/.local/bin:\${PATH}\"" "${FAKE_HOME}/.bashrc"; then
    echo "FAILED: .bashrc did not contain the export PATH command"
    exit 1
fi
echo "✓ Test 1 passed!"

# Test 2: Zsh shell profile update
echo "--- Test 2: Auto PATH configuration for Zsh ---"
FAKE_HOME_ZSH="${TEST_TMP}/home_zsh"
mkdir -p "${FAKE_HOME_ZSH}"
touch "${FAKE_HOME_ZSH}/.zshrc"

PATH="${MOCK_CURL_DIR}:/usr/bin:/bin" \
HOME="${FAKE_HOME_ZSH}" \
SHELL="/usr/local/bin/zsh" \
bash ./install.sh

if ! grep -q "export PATH=\"${FAKE_HOME_ZSH}/.local/bin:\${PATH}\"" "${FAKE_HOME_ZSH}/.zshrc"; then
    echo "FAILED: .zshrc did not contain the export PATH command"
    exit 1
fi
echo "✓ Test 2 passed!"

# Test 3: Opt-out with MITHRIL_NO_MODIFY_PATH=1
echo "--- Test 3: MITHRIL_NO_MODIFY_PATH=1 opt-out ---"
FAKE_HOME_NOMOD="${TEST_TMP}/home_nomod"
mkdir -p "${FAKE_HOME_NOMOD}"
touch "${FAKE_HOME_NOMOD}/.bashrc"

PATH="${MOCK_CURL_DIR}:/usr/bin:/bin" \
HOME="${FAKE_HOME_NOMOD}" \
SHELL="/bin/bash" \
MITHRIL_NO_MODIFY_PATH=1 \
bash ./install.sh

if grep -q "export PATH" "${FAKE_HOME_NOMOD}/.bashrc"; then
    echo "FAILED: .bashrc was modified despite MITHRIL_NO_MODIFY_PATH=1"
    exit 1
fi
echo "✓ Test 3 passed!"

# Test 4: Custom MITHRIL_INSTALL_DIR
echo "--- Test 4: Custom MITHRIL_INSTALL_DIR ---"
FAKE_HOME_CUSTOM="${TEST_TMP}/home_custom"
CUSTOM_DIR="${TEST_TMP}/custom_bin"
mkdir -p "${FAKE_HOME_CUSTOM}"
touch "${FAKE_HOME_CUSTOM}/.bashrc"

PATH="${MOCK_CURL_DIR}:/usr/bin:/bin" \
HOME="${FAKE_HOME_CUSTOM}" \
SHELL="/bin/bash" \
MITHRIL_INSTALL_DIR="${CUSTOM_DIR}" \
bash ./install.sh

if [ ! -x "${CUSTOM_DIR}/mithril" ]; then
    echo "FAILED: mithril binary not found in custom directory"
    exit 1
fi

if ! grep -q "export PATH=\"${CUSTOM_DIR}:\${PATH}\"" "${FAKE_HOME_CUSTOM}/.bashrc"; then
    echo "FAILED: custom directory was not added to .bashrc"
    exit 1
fi
echo "✓ Test 4 passed!"

echo "All install.sh tests passed successfully! 🎉"
