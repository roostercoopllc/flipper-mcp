use serde_json::{json, Value};

use crate::mcp::types::{ToolDefinition, ToolResult};
use crate::modules::traits::FlipperModule;
use crate::uart::FlipperProtocol;

pub struct BleModule;

impl FlipperModule for BleModule {
    fn name(&self) -> &str {
        "ble"
    }

    fn description(&self) -> &str {
        "BLE scanning, connection, and GATT operations (via Flipper STM32WB)"
    }

    fn tools(&self) -> Vec<ToolDefinition> {
        vec![
            ToolDefinition {
                name: "ble_scan".to_string(),
                description: "Scan for nearby BLE devices. Note: temporarily disconnects the Flipper mobile app."
                    .to_string(),
                input_schema: json!({
                    "type": "object",
                    "properties": {
                        "duration": {
                            "type": "integer",
                            "description": "Scan duration in seconds (1-30, default 5)",
                            "minimum": 1,
                            "maximum": 30,
                            "default": 5
                        }
                    },
                    "required": []
                }),
            },
            ToolDefinition {
                name: "ble_connect".to_string(),
                description: "Connect to a BLE device by MAC address".to_string(),
                input_schema: json!({
                    "type": "object",
                    "properties": {
                        "mac": {
                            "type": "string",
                            "description": "BLE MAC address (e.g. 'AA:BB:CC:DD:EE:FF')"
                        }
                    },
                    "required": ["mac"]
                }),
            },
            ToolDefinition {
                name: "ble_disconnect".to_string(),
                description: "Disconnect from the currently connected BLE device".to_string(),
                input_schema: json!({
                    "type": "object",
                    "properties": {},
                    "required": []
                }),
            },
            ToolDefinition {
                name: "ble_gatt_discover".to_string(),
                description: "Discover GATT services and characteristics on a connected BLE device"
                    .to_string(),
                input_schema: json!({
                    "type": "object",
                    "properties": {},
                    "required": []
                }),
            },
            ToolDefinition {
                name: "ble_gatt_read".to_string(),
                description: "Read a GATT characteristic value by handle".to_string(),
                input_schema: json!({
                    "type": "object",
                    "properties": {
                        "handle": {
                            "type": "integer",
                            "description": "GATT characteristic handle (from ble_gatt_discover)"
                        }
                    },
                    "required": ["handle"]
                }),
            },
            ToolDefinition {
                name: "ble_gatt_write".to_string(),
                description: "Write data to a GATT characteristic by handle".to_string(),
                input_schema: json!({
                    "type": "object",
                    "properties": {
                        "handle": {
                            "type": "integer",
                            "description": "GATT characteristic handle (from ble_gatt_discover)"
                        },
                        "data": {
                            "type": "string",
                            "description": "Hex-encoded data to write (e.g. '0102FF')"
                        }
                    },
                    "required": ["handle", "data"]
                }),
            },
        ]
    }

    fn execute(
        &self,
        tool: &str,
        args: &Value,
        protocol: &mut dyn FlipperProtocol,
    ) -> ToolResult {
        let command = match tool {
            "ble_scan" => {
                let duration = args
                    .get("duration")
                    .and_then(|v| v.as_i64())
                    .unwrap_or(5);
                format!("ble scan --duration {}", duration)
            }
            "ble_connect" => match args.get("mac").and_then(|v| v.as_str()) {
                Some(mac) => format!("ble connect {}", mac),
                None => return ToolResult::error("Missing required parameter: mac"),
            },
            "ble_disconnect" => "ble disconnect".to_string(),
            "ble_gatt_discover" => "ble gatt_discover".to_string(),
            "ble_gatt_read" => match args.get("handle").and_then(|v| v.as_i64()) {
                Some(handle) => format!("ble gatt_read {}", handle),
                None => return ToolResult::error("Missing required parameter: handle"),
            },
            "ble_gatt_write" => {
                let handle = args.get("handle").and_then(|v| v.as_i64());
                let data = args.get("data").and_then(|v| v.as_str());
                match (handle, data) {
                    (Some(h), Some(d)) => format!("ble gatt_write {} {}", h, d),
                    _ => {
                        return ToolResult::error("Missing required parameters: handle, data")
                    }
                }
            }
            _ => return ToolResult::error(format!("Unknown ble tool: {}", tool)),
        };

        match protocol.execute_command_with_timeout(&command, 35_000) {
            Ok(output) => ToolResult::success(output),
            Err(e) => ToolResult::error(format!("{} failed: {}", tool, e)),
        }
    }
}
