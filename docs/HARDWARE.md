# Hardware Guide

## Required Hardware

### Flipper Zero
- Any firmware (Official, Unleashed, RogueMaster, etc.)
- Firmware must support the CLI (all current versions do)
- SD card recommended (required for FAP discovery, TOML modules, SD config)

### WiFi Dev Board v1
- Product: [Flipper Zero WiFi Dev Board](https://shop.flipperzero.one/products/wifi-devboard)
- SoC: ESP32-S2-WROVER (Xtensa LX7, 4MB flash, 2MB PSRAM)
- Connects via the Flipper Zero's GPIO expansion header
- Has its own USB-C port for programming and serial monitoring

---

## Connection Overview

```
Flipper Zero GPIO Header
        ↕ (auto-connected when board is attached)
WiFi Dev Board v1 (ESP32-S2)
        ↕
USB-C → Your computer (for flashing and serial monitor)
```

When the WiFi Dev Board is seated on the Flipper's expansion header, all necessary connections (UART, power, ground) are made automatically.

---

## UART Pin Mapping

The firmware uses these pins for Flipper ↔ ESP32-S2 communication:

| ESP32-S2 GPIO | Direction | Flipper Header Pin | Function |
|--------------|-----------|-------------------|---------|
| GPIO 43 | TX → | Pin 14 (RX) | UART transmit |
| GPIO 44 | ← RX | Pin 13 (TX) | UART receive |
| GND | — | GND | Common ground |
| 3.3V / 5V | ← | Power | Supplied by Flipper |

Note: GPIO 1/2 on the WiFi Dev Board v1 are SWD debug pins (SWCLK/SWDIO), not UART.

Baud rate: **115200** (configurable via `uart_baud_rate` in config)

These are the standard pins for the WiFi Dev Board v1 expansion header. They are hardcoded in `firmware/src/uart/transport.rs`.

---

## USB Connections

| Task | Connect to | Notes |
|------|-----------|-------|
| Flash firmware | **WiFi Dev Board USB-C** | Board has its own USB connector |
| Serial monitor | **WiFi Dev Board USB-C** | Same port as flashing |
| SD card config | **Flipper Zero** or SD reader | Mount SD card directly |
| Normal use | Neither (WiFi only) | After setup, no USB needed |

The Flipper Zero's USB-C port is for Flipper firmware, not for this project.

---

## Power

The WiFi Dev Board draws power from the Flipper Zero via the expansion header when attached. The ESP32-S2 can also be powered from USB-C independently.

Battery impact: WiFi + ESP32 active adds noticeable drain. Expect ~30-40% reduced battery life with WiFi active.

---

## WiFi Dev Board v1 vs v2

This firmware targets the **v1 board** (ESP32-S2). The v2 board uses ESP32-S3 and is not supported (requires different target and pin definitions). Check the product page to confirm your board version.

---

## Physical Setup

1. Power off the Flipper Zero
2. Seat the WiFi Dev Board onto the expansion header (top of Flipper)
3. Power on the Flipper — the ESP32-S2 boots automatically
4. **Disable Expansion Modules:** Go to **Settings → System → Expansion Modules → None**.
   Without this, the Flipper's expansion protocol handler intercepts UART data
   and the ESP32 cannot communicate with the CLI. See [TROUBLESHOOTING.md](TROUBLESHOOTING.md#no-status-file-despite-esp32-running--expansion-modules-setting) for details.
5. Connect the WiFi Dev Board USB-C to your computer for flashing
6. After flashing, USB is optional — the board runs on Flipper power
