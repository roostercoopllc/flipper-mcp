use serde_json::{json, Value};

use crate::mcp::types::{ToolDefinition, ToolResult};
use crate::modules::traits::FlipperModule;
use crate::uart::FlipperProtocol;

pub struct C2Module;

impl FlipperModule for C2Module {
    fn name(&self) -> &str {
        "c2"
    }

    fn description(&self) -> &str {
        "SubGHz C2 command-and-control for Flipper-to-Flipper communication (433 MHz)"
    }

    fn tools(&self) -> Vec<ToolDefinition> {
        vec![
            ToolDefinition {
                name: "c2_send".to_string(),
                description: "Send a C2 command to the client Flipper over SubGHz radio. The command is encoded as a binary frame and transmitted at the configured frequency. Returns the client's response or times out."
                    .to_string(),
                input_schema: json!({
                    "type": "object",
                    "properties": {
                        "command": {
                            "type": "string",
                            "description": "Command type: 'ble_hid_start', 'ble_hid_type', 'ble_hid_press', 'ble_hid_mouse', 'ble_hid_stop', 'ble_beacon_start', 'ble_beacon_stop'"
                        },
                        "payload": {
                            "type": "string",
                            "description": "Command payload (varies by command type). For ble_hid_type: the text to type. For ble_hid_press: key combo string. For ble_beacon_start: hex adv data. For ble_hid_start: device name (optional)."
                        },
                        "timeout": {
                            "type": "integer",
                            "description": "Response timeout in ms (default 5000, max 30000)",
                            "minimum": 500,
                            "maximum": 30000,
                            "default": 5000
                        }
                    },
                    "required": ["command"]
                }),
            },
            ToolDefinition {
                name: "c2_recv".to_string(),
                description: "Poll for a pending response from the client Flipper. Blocks until a response is received or the timeout expires."
                    .to_string(),
                input_schema: json!({
                    "type": "object",
                    "properties": {
                        "timeout": {
                            "type": "integer",
                            "description": "Timeout in ms (default 5000, max 30000)",
                            "minimum": 500,
                            "maximum": 30000,
                            "default": 5000
                        }
                    },
                    "required": []
                }),
            },
            ToolDefinition {
                name: "c2_ping".to_string(),
                description: "Ping the client Flipper over SubGHz to verify connectivity. Returns PONG if the client is listening on the configured frequency."
                    .to_string(),
                input_schema: json!({
                    "type": "object",
                    "properties": {},
                    "required": []
                }),
            },
            ToolDefinition {
                name: "c2_status".to_string(),
                description: "Get the C2 radio status: whether it's active, the current frequency, last sequence number, and last client response."
                    .to_string(),
                input_schema: json!({
                    "type": "object",
                    "properties": {},
                    "required": []
                }),
            },
            ToolDefinition {
                name: "c2_configure".to_string(),
                description: "Configure the C2 SubGHz radio. Sets frequency and optionally starts/stops the radio. The radio must be started before sending commands."
                    .to_string(),
                input_schema: json!({
                    "type": "object",
                    "properties": {
                        "action": {
                            "type": "string",
                            "description": "Action: 'start' (start radio), 'stop' (stop radio), 'set_freq' (change frequency)",
                            "enum": ["start", "stop", "set_freq"]
                        },
                        "frequency": {
                            "type": "integer",
                            "description": "SubGHz frequency in Hz (e.g., 433920000 for 433.92 MHz). Only used with 'start' or 'set_freq' actions.",
                            "default": 433920000
                        }
                    },
                    "required": ["action"]
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
            "c2_send" => {
                let cmd_type = match args.get("command").and_then(|v| v.as_str()) {
                    Some(c) => c,
                    None => return ToolResult::error("Missing required parameter: command"),
                };
                let payload = args
                    .get("payload")
                    .and_then(|v| v.as_str())
                    .unwrap_or("");
                let timeout = args
                    .get("timeout")
                    .and_then(|v| v.as_i64())
                    .unwrap_or(5000);
                format!("c2 send {} {} {}", cmd_type, payload, timeout)
            }
            "c2_recv" => {
                let timeout = args
                    .get("timeout")
                    .and_then(|v| v.as_i64())
                    .unwrap_or(5000);
                format!("c2 recv {}", timeout)
            }
            "c2_ping" => "c2 ping".to_string(),
            "c2_status" => "c2 status".to_string(),
            "c2_configure" => {
                let action = match args.get("action").and_then(|v| v.as_str()) {
                    Some(a) => a,
                    None => return ToolResult::error("Missing required parameter: action"),
                };
                let freq = args
                    .get("frequency")
                    .and_then(|v| v.as_i64())
                    .unwrap_or(433920000);
                format!("c2 configure {} {}", action, freq)
            }
            _ => return ToolResult::error(format!("Unknown c2 tool: {}", tool)),
        };

        // C2 operations can take a while (SubGHz round-trip + BLE execution)
        let timeout_ms: u32 = match tool {
            "c2_send" => {
                let t = args
                    .get("timeout")
                    .and_then(|v| v.as_i64())
                    .unwrap_or(5000);
                (t as u32).min(30_000) + 2_000 // add 2s buffer for UART overhead
            }
            "c2_recv" => {
                let t = args
                    .get("timeout")
                    .and_then(|v| v.as_i64())
                    .unwrap_or(5000);
                (t as u32).min(30_000) + 2_000
            }
            "c2_ping" => 8_000,
            "c2_configure" => 5_000,
            _ => 5_000,
        };

        match protocol.execute_command_with_timeout(&command, timeout_ms) {
            Ok(output) => ToolResult::success(output),
            Err(e) => ToolResult::error(format!("{} failed: {}", tool, e)),
        }
    }
}
