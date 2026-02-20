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

echo "Building firmware..."
cd "$FIRMWARE_DIR"
cargo build --release --target xtensa-esp32s2-espidf

echo ""
echo "Flashing to device (opens serial monitor after flash)..."
echo "Press Ctrl+R to reset, Ctrl+C to exit monitor."
cargo run --release --target xtensa-esp32s2-espidf
