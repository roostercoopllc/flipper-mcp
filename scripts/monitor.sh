#!/usr/bin/env bash
# monitor.sh — Open the serial monitor for the Flipper WiFi Dev Board.
# Connect the WiFi Dev Board via USB-C before running this.
set -euo pipefail

PORT="${1:-}"  # optional: override port, e.g. scripts/monitor.sh /dev/ttyUSB1

if [[ -n "$PORT" ]]; then
    espflash monitor "$PORT"
else
    espflash monitor
fi
