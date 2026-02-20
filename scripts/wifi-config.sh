#!/usr/bin/env bash
# wifi-config.sh — Write WiFi credentials to the ESP32-S2 NVS partition.
#
# This generates an NVS image and flashes it to the nvs partition.
# Run BEFORE flashing the firmware on a fresh device.
# If the firmware is already on the device, it will read the new credentials on next boot.
#
# Usage:
#   scripts/wifi-config.sh --ssid MyNetwork --password MyPassword
#   scripts/wifi-config.sh --ssid MyNetwork --password MyPassword --relay ws://relay.example.com:9090/tunnel
#   scripts/wifi-config.sh --erase     # erase WiFi creds (returns to AP mode)
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
FIRMWARE_DIR="$SCRIPT_DIR/../firmware"

# ---- Argument parsing ----
SSID=""
PASSWORD=""
RELAY_URL=""
DEVICE_NAME="flipper-mcp"
ERASE=false

while [[ $# -gt 0 ]]; do
    case "$1" in
        --ssid)       SSID="$2";        shift 2 ;;
        --password)   PASSWORD="$2";    shift 2 ;;
        --relay)      RELAY_URL="$2";   shift 2 ;;
        --name)       DEVICE_NAME="$2"; shift 2 ;;
        --erase)      ERASE=true;       shift   ;;
        -h|--help)
            grep '^#' "$0" | head -12 | sed 's/^# \?//'
            exit 0 ;;
        *) echo "Unknown option: $1" >&2; exit 1 ;;
    esac
done

# ---- Check dependencies ----
if ! command -v python3 &>/dev/null; then
    echo "ERROR: python3 required" >&2; exit 1
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

# Find nvs_partition_gen.py in the ESP-IDF installation
NVS_TOOL=""
for candidate in \
    "$HOME/.espressif/python_env/idf5.2_py3.*/bin/python3" \
    "/opt/esp-idf/tools/nvs_flash/nvs_partition_gen/nvs_partition_gen.py" \
    "$HOME/.embuild/espressif/esp-idf/v5.2.5/components/nvs_flash/nvs_partition_generator/nvs_partition_gen.py"
do
    # shellcheck disable=SC2086
    files=( $candidate )  # expand glob
    if [[ -f "${files[0]:-}" ]]; then
        NVS_TOOL="${files[0]}"
        break
    fi
done

# Fallback: look in .embuild
if [[ -z "$NVS_TOOL" ]]; then
    NVS_TOOL=$(find "$FIRMWARE_DIR/../.embuild" -name "nvs_partition_gen.py" 2>/dev/null | head -1)
fi

if [[ -z "$NVS_TOOL" ]]; then
    echo "ERROR: nvs_partition_gen.py not found." >&2
    echo "Alternative: use the AP captive portal — boot the device with no WiFi credentials," >&2
    echo "connect to FlipperMCP-XXXX, and open http://192.168.4.1" >&2
    exit 1
fi

TMPDIR=$(mktemp -d)
trap 'rm -rf "$TMPDIR"' EXIT

if $ERASE; then
    echo "Erasing WiFi credentials from NVS..."
    "$ESPFLASH" erase-parts --no-stub --partitions nvs
    echo "Done. Device will boot into AP mode on next start."
    exit 0
fi

if [[ -z "$SSID" ]]; then
    echo "ERROR: --ssid is required (or --erase to clear credentials)" >&2
    exit 1
fi

# ---- Generate NVS CSV ----
NVS_CSV="$TMPDIR/nvs.csv"
cat > "$NVS_CSV" <<EOF
key,type,encoding,value
flipper,namespace,,
wifi_ssid,data,string,$SSID
wifi_password,data,string,$PASSWORD
device_name,data,string,$DEVICE_NAME
relay_url,data,string,$RELAY_URL
EOF

# ---- Generate NVS binary image ----
NVS_BIN="$TMPDIR/nvs.bin"
python3 "$NVS_TOOL" generate "$NVS_CSV" "$NVS_BIN" 0x6000

# ---- Flash the NVS partition ----
echo "Flashing NVS partition with WiFi credentials..."
echo "  SSID:        $SSID"
echo "  Device name: $DEVICE_NAME"
[[ -n "$RELAY_URL" ]] && echo "  Relay URL:   $RELAY_URL"

"$ESPFLASH" write-bin --no-stub 0x9000 "$NVS_BIN"
echo ""
echo "Done. Reset the device to apply changes."
echo "If no firmware is flashed yet, run: scripts/flash.sh"
