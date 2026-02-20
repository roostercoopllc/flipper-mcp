#!/usr/bin/env bash
# build.sh — Build the firmware (release profile).
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

BIN="$FIRMWARE_DIR/target/xtensa-esp32s2-espidf/release/flipper-mcp"
SIZE=$(wc -c < "$BIN" 2>/dev/null || echo "?")
echo ""
echo "Build successful. Binary: $BIN ($SIZE bytes)"
