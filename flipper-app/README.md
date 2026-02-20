# Flipper MCP — Flipper Zero Companion App

Appears in **Apps → Tools → Flipper MCP** on the Flipper Zero.

Provides an on-device UI to view the WiFi Dev Board status and manage the MCP HTTP server lifecycle — no computer required after initial setup.

## Screens

| Screen | Navigation |
|--------|-----------|
| **Menu** | Up/Down to select, OK to enter, Back to exit app |
| **Status** | Shows `ip`, `ssid`, `server`, `device`, `ver` from the ESP32 status file |
| **Start / Stop / Restart** | Writes `server.cmd` on the SD card; ESP32 picks it up within 5 s |

Status is refreshed by the ESP32 every 30 seconds and immediately after any command is processed.

---

## Prerequisites

### 1. Python 3.8+
```bash
python3 --version  # must be ≥ 3.8
```

### 2. ufbt (micro Flipper Build Tool)
```bash
# Kali / Debian / Ubuntu (managed Python environment — use pipx):
pipx install ufbt
pipx ensurepath && source ~/.zshrc   # add ~/.local/bin to PATH

# Other systems:
pip3 install ufbt
```

### 3. USB connection to the Flipper Zero (for wireless deploy)
Or an SD card reader if deploying manually.

---

## Build

```bash
cd flipper-app
ufbt          # downloads SDK if needed, builds flipper_mcp.fap
```

On first run, ufbt downloads the latest stable Flipper firmware SDK (~200 MB). Subsequent builds are fast.

Output: `flipper-app/dist/flipper_mcp.fap`

To build against a specific firmware channel:
```bash
ufbt update --channel=release   # stable (default)
ufbt update --channel=rc        # release candidate
ufbt update --channel=dev       # bleeding edge
ufbt                            # rebuild after channel switch
```

---

## Deploy

### Option A — USB (Flipper connected to PC)

```bash
cd flipper-app

# Build + deploy + launch in one command:
ufbt launch

# Or just deploy without launching:
ufbt deploy
```

`ufbt launch` copies the FAP to `SD:/apps/Tools/flipper_mcp.fap` and starts it immediately.

### Option B — SD card (manual)

1. Remove the SD card from the Flipper and mount it on your PC
2. Copy the built FAP to the SD card:
   ```bash
   cp flipper-app/dist/flipper_mcp.fap /path/to/sd/apps/Tools/flipper_mcp.fap
   ```
3. Eject and reinsert the SD card in the Flipper
4. The app appears in **Apps → Tools → Flipper MCP**

### Option C — qFlipper

1. Open qFlipper and connect your Flipper via USB
2. Go to the **File manager** tab
3. Navigate to `SD Card → apps → Tools`
4. Drag and drop `flipper-app/dist/flipper_mcp.fap` into the folder

---

## Verify

After installing, open the app on the Flipper:

1. **Apps → Tools → Flipper MCP**
2. Select **Status**
3. If the ESP32 is running and has been up for at least 30 seconds, you should see:
   ```
   ip: 192.168.x.xxx
   ssid: YourNetwork
   server: running
   device: flipper-mcp
   ver: 0.1.0
   ```

If Status shows "No status file found", the ESP32 hasn't written the status file yet. Wait 30 seconds and check again, or power-cycle the WiFi Dev Board.

---

## Troubleshooting

### `ufbt: command not found`
```bash
pip3 install ufbt
# or
python3 -m pip install ufbt
# then ensure pip's bin dir is in PATH:
export PATH="$HOME/.local/bin:$PATH"
```

### Build fails with missing SDK
```bash
ufbt update   # re-download SDK
ufbt          # rebuild
```

### App not visible in Apps → Tools
- Confirm the `.fap` is at `SD:/apps/Tools/flipper_mcp.fap` (not a subdirectory)
- The Flipper rescans the SD card on boot — reboot after copying the file manually

### Status screen always shows "No status file"
The ESP32 writes `status.txt` via the `storage write_chunk` Flipper CLI command.
- Confirm the WiFi Dev Board is connected to the Flipper's GPIO header
- Confirm the ESP32 firmware is flashed and running (LED should be solid or blinking)
- Check the serial monitor: `./scripts/monitor.sh` — look for "Status file write" log lines

### Commands (Start/Stop/Restart) have no effect
- The ESP32 polls `server.cmd` every 5 seconds — wait up to 5 seconds
- Confirm the SD card is inserted in the Flipper
- Check the serial monitor for "Server control command from Flipper:" log lines
