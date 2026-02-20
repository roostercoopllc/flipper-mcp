#!/usr/bin/env bash
# flash.sh — Build and flash the firmware to the Flipper WiFi Dev Board.
#
# Connect the WiFi Dev Board via USB-C (NOT the Flipper's USB port).
# The board should appear as /dev/ttyUSB0 or /dev/ttyACM0.
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
FIRMWARE_DIR="$SCRIPT_DIR/../firmware"

# Source ESP toolchain environment
if [[ -f ~/export-esp.sh ]]; then
    # shellcheck disable=SC1090
    source ~/export-esp.sh
else
    echo "ERROR: ~/export-esp.sh not found. Run scripts/setup-toolchain.sh first." >&2
    exit 1
fi

# Find espflash — prefer PATH, fall back to ~/.cargo/bin
if command -v espflash &>/dev/null; then
    ESPFLASH="espflash"
elif [[ -x "$HOME/.cargo/bin/espflash" ]]; then
    ESPFLASH="$HOME/.cargo/bin/espflash"
else
    echo "ERROR: espflash not found. Install with: cargo +stable install espflash" >&2
    exit 1
fi

echo "Building firmware..."
cd "$FIRMWARE_DIR"
cargo build --release --target xtensa-esp32s2-espidf

BIN="$(dirname "$FIRMWARE_DIR")/target/xtensa-esp32s2-espidf/release/flipper-mcp"

echo ""
echo "Build complete. Now enter bootloader mode on the WiFi Dev Board:"
echo "  Hold BOOT → tap RESET → release BOOT"
echo ""
read -r -p "Press Enter once /dev/ttyACM0 is ready (or Ctrl+C to cancel)..."
echo ""
echo "Flashing... (press Ctrl+C to exit monitor after flash)"
"$ESPFLASH" flash --no-stub --monitor "$BIN"
