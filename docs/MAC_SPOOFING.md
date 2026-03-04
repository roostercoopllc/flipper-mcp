# WiFi MAC Address Spoofing

The flipper-mcp firmware supports spoofing the WiFi MAC address to impersonate legitimate network hardware during authorized penetration testing. Combined with hostname and HTTP banner spoofing, the device appears indistinguishable from commercial IoT equipment at every network visibility layer.

## Overview

By configuring a custom MAC address, the ESP32-S2 broadcasts a spoofed OUI that is attributed to a known vendor by the OS and network tools. The full identity stack — MAC OUI, DHCP hostname, mDNS service name, HTTP Server header, and API response bodies — are all set consistently so that nmap, Shodan, Wireshark, and SIEM enrichment tools all agree on the same fake identity.

**Use case**: Authorized penetration testing to verify if network monitoring/IDS detects spoofed devices.

## Configuration

### Via Flipper FAP (Runtime)

In the flipper-mcp app, set the WiFi config with MAC address:

```
SSID: your_network
Password: your_password
Auth: wpa2
MAC: 00:14:4F:AA:BB:CC
```

The FAP syntax accepts `wifi_mac` or `mac` key:
```
ssid=your_network|password=your_password|auth=wpa2|wifi_mac=00:14:4F:AA:BB:CC
```

### Via SD Card (Persistent across reboots)

Edit `/ext/apps_data/flipper_mcp/config.txt`:

```
wifi_ssid=your_network
wifi_password=your_password
wifi_auth=wpa2
wifi_mac=00:14:4F:AA:BB:CC
```

The MAC address is stored in NVS and applied on every WiFi initialization.

## MAC Address Format

- Format: `AA:BB:CC:DD:EE:FF` (6 octets, colon-separated, hex digits)
- Case-insensitive: `00:14:4F:AA:BB:CC` same as `00:14:4f:aa:bb:cc`
- Invalid formats will be logged as configuration errors

### Common OUIs for IoT and Infrastructure Impersonation

| Vendor / Scenario | OUI | Example MAC |
|---|---|---|
| **Philips Hue** (smart lighting — Delos PoC) | `00:17:88` | `00:17:88:A3:F1:2C` |
| Philips Hue (alternate block) | `EC:B5:FA` | `EC:B5:FA:12:34:56` |
| Nest / Google Home | `18:B4:30` | `18:B4:30:12:34:56` |
| Amazon Echo | `FC:65:DE` | `FC:65:DE:12:34:56` |
| Honeywell BMS / thermostat | `00:D0:2D` | `00:D0:2D:12:34:56` |
| Siemens building automation | `00:1B:1B` | `00:1B:1B:12:34:56` |
| Dell PowerEdge (server) | `00:14:4F` | `00:14:4F:12:34:56` |
| HP ProLiant (server) | `00:30:6E` | `00:30:6E:12:34:56` |

Replace the last 3 octets with any values to avoid collisions with real devices on the network.

**Delos PoC config** (`config.txt`):
```ini
wifi_mac=00:17:88:A3:F1:2C
device_name=Delos-Thermostat-4F
```

## How It Works

1. **At startup**: `create_wifi()` initializes the WiFi driver
2. **Config applied**: Settings loaded from NVS (if set) or default to empty
3. **MAC address set**: If `wifi_mac` is not empty, `apply_mac_address()` calls `esp_wifi_set_mac()` before connection
4. **Persistent**: Stored in NVS under key `"wifi_mac"`, survives reboot

## Logging

When MAC address spoofing is applied, the firmware logs:

```
[INFO] WiFi MAC address set to: 00:14:4F:12:34:56
```

Check the UART logs or Flipper "View Logs" to verify the MAC was applied.

## Verification

To verify the spoofed identity is active at every network layer:

### Layer 2 — MAC and ARP

```bash
# ARP scan — confirms Philips Hue OUI attribution
sudo arp-scan 192.168.0.0/24 | grep -i "00:17:88"
# Expected:
# 192.168.0.58    00:17:88:a3:f1:2c    Philips Lighting BV

# Passive ARP table
arp -n | grep "00:17:88"

# tcpdump — capture frames from the spoofed MAC
sudo tcpdump -i eth0 -e 'ether src 00:17:88:a3:f1:2c' -c 10
```

### Layer 3/4 — nmap service fingerprint

```bash
FLIPPER=192.168.0.58

# Service + script scan: confirms Server header, HTTP title, and MAC OUI in one shot
sudo nmap -sV --script http-title,http-headers -p 8080 $FLIPPER
# Expected:
# PORT     STATE SERVICE VERSION
# 8080/tcp open  http    Delos-BMS/2.1.4
# | http-title: Delos Building Management System
# | http-headers:
# |   Server: Delos-BMS/2.1.4
# MAC Address: 00:17:88:A3:F1:2C (Philips Lighting BV)

# Aggressive scan — OS detection, traceroute, all scripts
sudo nmap -A -p 8080 $FLIPPER

# Confirm no "Flipper" or "ESP32" strings survive in any response
sudo nmap -sV --script http-title,http-headers,http-auth-finder -p 8080 $FLIPPER \
  | grep -iE "flipper|esp32|espressif"
# Expected: no output (nothing reveals the real hardware)
```

### mDNS — service discovery

```bash
# avahi-browse: shows _delos-bms._tcp service and instance name
avahi-browse -a -t 2>/dev/null | grep -i delos
# Expected:
# +  eth0 IPv4 Delos Building Management System  _delos-bms._tcp  local
# +  eth0 IPv4 Delos Building Management System  _http._tcp       local

# Resolve hostname
avahi-resolve -n Delos-Thermostat-4F.local
# Expected:
# Delos-Thermostat-4F.local    192.168.0.58

# Inspect TXT records (model, zone, vendor)
avahi-browse -r _delos-bms._tcp 2>/dev/null
# Expected TXT: model=BMS-v2.1.4  zone=4F  vendor=Delos

# macOS
dns-sd -B _delos-bms._tcp local
dns-sd -L "Delos Building Management System" _delos-bms._tcp local
```

### Application layer — HTTP + MCP

```bash
# Server header on every response
curl -sI http://$FLIPPER:8080/health | grep Server
# Expected:  Server: Delos-BMS/2.1.4

# Health endpoint body
curl -s http://$FLIPPER:8080/health | python3 -m json.tool
# Expected:
# {"status":"ok","service":"Delos Building Management System","model":"BMS-v2.1.4","zone":"4F","controller":"online"}

# Root page HTML title
curl -s http://$FLIPPER:8080/ | grep title
# Expected:  <title>Delos Building Management System</title>

# MCP serverInfo (no "flipper-mcp" visible)
curl -s -X POST http://$FLIPPER:8080/mcp \
  -H "Content-Type: application/json" \
  -d '{"jsonrpc":"2.0","id":0,"method":"initialize","params":{"protocolVersion":"2025-03-26","capabilities":{}}}' \
  | python3 -c "import sys,json; print(json.load(sys.stdin)['result']['serverInfo'])"
# Expected:  {'name': 'delos-bms', 'version': '...'}
```

## Limitations

- MAC address must be set **before** WiFi radio starts
- MAC only affects WiFi; Bluetooth (if used) is unaffected
- Some access points may block unknown OUIs
- Extremely sophisticated network monitoring may detect spoofing via other attributes (vendor-specific response patterns, timing)

## Troubleshooting

### "Invalid MAC address format"
Ensure format is exactly `AA:BB:CC:DD:EE:FF` with colons. Check logs for which octet failed parsing.

### MAC doesn't appear to apply
Check that the configuration was persisted:
- Verify `config.txt` on SD card has the `wifi_mac` line
- Restart the firmware to reload from NVS
- Check UART logs for the "WiFi MAC address set" confirmation

### WiFi fails to connect after spoofing
Some routers may filter by MAC. Try:
- Using a locally-administered MAC (bit 1 of first octet = 1, e.g., `02:14:4F:...`)
- Using a different OUI prefix
- Disabling MAC filtering on the test network (if authorized)

## Security Notes

**For authorized testing only.** MAC spoofing is:
- Trivially detectable with proper network monitoring
- Useful for testing if your IDS/monitoring detects spoofed infrastructure
- NOT a substitute for real penetration testing tools

Use this feature as part of a comprehensive authorized security assessment, with proper documentation and scope agreements.

## API Reference

### Configuration Key
- **Key**: `wifi_mac` or `mac`
- **Type**: String
- **Format**: `AA:BB:CC:DD:EE:FF`
- **Default**: Empty string (use hardware MAC)
- **Persistence**: NVS (survives reboot)

### Implementation Details

**File**: `firmware/src/wifi/station.rs`

```rust
extern "C" {
    fn esp_wifi_set_mac(ifx: u32, mac: *const u8) -> i32;
}

fn apply_mac_address(wifi: &mut BlockingWifi<EspWifi<'static>>, mac_str: &str) -> Result<()>
fn parse_mac_address(mac_str: &str) -> Result<[u8; 6]>
```

The `apply_mac_address()` function:
1. Parses the MAC address string
2. Calls ESP-IDF's `esp_wifi_set_mac()` C API
3. Logs confirmation with the applied MAC

## Example Scenario — Delos Smart Thermostat PoC

**Objective**: Confirm that the ESP32-S2 appears as a Philips Hue / Delos BMS device at every visibility layer a SIEM or SOC analyst would check.

```
1. Write config.txt to Flipper SD card:
      wifi_mac=00:17:88:A3:F1:2C
      device_name=Delos-Thermostat-4F

2. Power on Flipper-A + WiFi board — connects to network with spoofed OUI

3. From router admin panel: device listed as "Philips Lighting BV" at .58

4. nmap -sV --script http-title -p 8080 192.168.0.58
   → Shows: "Delos-BMS/2.1.4", title "Delos Building Management System"

5. avahi-browse -a: shows "_delos-bms._tcp — Delos Building Management System"

6. MCP client connects: serverInfo = {"name":"delos-bms"}
   Tools listed: change_temperature, read_occupancy_sensor, check_air_quality, hvac_zone_status

7. SIEM enrichment: MAC OUI = Philips Lighting → tagged as "IoT/Smart Home"
   No "Flipper", "ESP32", or "Espressif" strings appear anywhere in the traffic
```

This tests whether your monitoring's IoT device tagging and anomaly detection can distinguish a compromised smart home device from a legitimate one when all identity signals are consistently spoofed.
