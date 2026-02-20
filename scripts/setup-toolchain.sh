#!/usr/bin/env bash
# setup-toolchain.sh — Install the Xtensa Rust toolchain and ESP32 build tools.
# Run once on a new machine before building the firmware.
# Tested on: Ubuntu 22.04+, Debian 12+, Kali Linux.
set -euo pipefail

echo "=== Flipper MCP toolchain setup ==="

# 1. Install espup (manages the Xtensa Rust toolchain)
if ! command -v espup &>/dev/null; then
    echo "Installing espup..."
    cargo install espup
else
    echo "espup already installed: $(espup --version)"
fi

# 2. Install the Xtensa toolchain (ESP32/ESP32-S2/ESP32-S3)
echo "Installing Xtensa Rust toolchain (this takes a few minutes)..."
espup install

# 3. Source the env vars (required for the current shell session)
# shellcheck disable=SC1090
source ~/export-esp.sh
echo "Xtensa toolchain installed. Add the following to your shell profile:"
echo '  source ~/export-esp.sh'

# 4. Install espflash (flash + monitor tool)
if ! command -v espflash &>/dev/null; then
    echo "Installing espflash..."
    cargo install espflash
else
    echo "espflash already installed: $(espflash --version)"
fi

# 5. Install ldproxy (linker wrapper required by esp-idf-sys)
if ! command -v ldproxy &>/dev/null; then
    echo "Installing ldproxy..."
    cargo install ldproxy
else
    echo "ldproxy already installed"
fi

# 6. Check for libxml2 (ESP-IDF clang needs libxml2.so.2)
if ! ldconfig -p 2>/dev/null | grep -q "libxml2.so.2"; then
    echo "WARNING: libxml2.so.2 not found."
    echo "If the build fails with 'cannot find -lxml2', run:"
    echo "  sudo apt install libxml2-dev"
    echo "  # or, if only libxml2.so.16 is available (Kali Linux):"
    echo "  sudo ln -s /usr/lib/x86_64-linux-gnu/libxml2.so.16 /usr/lib/x86_64-linux-gnu/libxml2.so.2"
fi

echo ""
echo "=== Setup complete ==="
echo "Next steps:"
echo "  1. source ~/export-esp.sh"
echo "  2. scripts/wifi-config.sh --ssid YourSSID --password YourPass"
echo "  3. scripts/flash.sh"
