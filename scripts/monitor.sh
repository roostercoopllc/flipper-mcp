#!/usr/bin/env bash
# monitor.sh — Open the serial monitor for the Flipper WiFi Dev Board.
# Connect the WiFi Dev Board via USB-C before running this.
set -euo pipefail

PORT="${1:-}"  # optional: override port, e.g. scripts/monitor.sh /dev/ttyUSB1

# Find espflash — prefer PATH, fall back to ~/.cargo/bin
if command -v espflash &>/dev/null; then
    ESPFLASH="espflash"
elif [[ -x "$HOME/.cargo/bin/espflash" ]]; then
    ESPFLASH="$HOME/.cargo/bin/espflash"
else
    echo "ERROR: espflash not found. Install with: cargo +stable install espflash" >&2
    exit 1
fi

if [[ -n "$PORT" ]]; then
    "$ESPFLASH" monitor "$PORT"
else
    "$ESPFLASH" monitor
fi
