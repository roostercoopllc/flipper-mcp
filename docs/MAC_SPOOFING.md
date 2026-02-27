# WiFi MAC Address Spoofing

The flipper-mcp firmware supports spoofing the WiFi MAC address to impersonate legitimate server hardware during authorized penetration testing.

## Overview

By configuring a custom MAC address, the ESP32-S2 can broadcast as infrastructure hardware matching a Red Hat/RHEL server or other systems. The spoofed MAC is persistent across reboots via NVS (Non-Volatile Storage).

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

### Common Server Hardware OUIs

For Red Hat/RHEL penetration testing, spoof common server manufacturer OUIs:

| Vendor | OUI | Example MAC |
|--------|-----|-------------|
| Dell PowerEdge | `00:14:4F` | `00:14:4F:12:34:56` |
| Dell | `00:1A:64` | `00:1A:64:12:34:56` |
| HP ProLiant | `00:30:6E` | `00:30:6E:12:34:56` |
| HP | `00:07:AA` | `00:07:AA:12:34:56` |
| IBM/Lenovo | `00:21:5E` | `00:21:5E:12:34:56` |

Replace the last 3 octets (` 12:34:56`) with any values to avoid address collisions.

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

To verify the spoofed MAC is active:

1. **From ESP32**: Check UART logs for confirmation message
2. **From router**: WiFi MAC will show the spoofed address
3. **From network tools**: ARP/DHCP assignments will show the spoofed MAC

```bash
# On nearby Linux/Mac:
sudo arp-scan -l | grep 00:14:4F
# or
tcpdump -i en0 'ether src 00:14:4F:12:34:56'
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

## Example Scenario

**Objective**: Test if network monitoring detects spoofed Dell server appearing on the network.

```
1. Configure ESP32-S2 with Dell OUI: 00:14:4F:AA:BB:CC
2. Power on, connect to test network
3. From router admin: See device with spoofed Dell MAC
4. Run network detection tools: Monitor if spoofing is detected
5. Check IDS/SIEM: Did it flag unusual MAC + network behavior?
```

This tests your monitoring's sensitivity to infrastructure anomalies.
