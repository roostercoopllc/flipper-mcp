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
        "BLE Extra Beacon broadcasting and HID (keyboard/mouse) injection via Flipper STM32WB"
    }

    fn tools(&self) -> Vec<ToolDefinition> {
        vec![
            ToolDefinition {
                name: "ble_info".to_string(),
                description: "Query Flipper BLE radio status: stack version, alive/active state, extra beacon status, HID profile state".to_string(),
                input_schema: json!({
                    "type": "object",
                    "properties": {},
                    "required": []
                }),
            },
            ToolDefinition {
                name: "ble_beacon".to_string(),
                description: "Start BLE Extra Beacon broadcasting arbitrary advertisement data. Runs alongside normal BLE. Useful for beacon spoofing, BLE tracking research, and proximity testing.".to_string(),
                input_schema: json!({
                    "type": "object",
                    "properties": {
                        "data": {
                            "type": "string",
                            "description": "Hex-encoded advertisement payload (up to 31 bytes, e.g. '0201061AFF4C000215...')"
                        },
                        "mac": {
                            "type": "string",
                            "description": "Spoofed MAC address in AA:BB:CC:DD:EE:FF format (optional, random if omitted)"
                        },
                        "interval": {
                            "type": "integer",
                            "description": "Advertisement interval in ms (20-10000, default 100)",
                            "minimum": 20,
                            "maximum": 10000,
                            "default": 100
                        }
                    },
                    "required": ["data"]
                }),
            },
            ToolDefinition {
                name: "ble_beacon_stop".to_string(),
                description: "Stop the BLE Extra Beacon broadcast".to_string(),
                input_schema: json!({
                    "type": "object",
                    "properties": {},
                    "required": []
                }),
            },
            ToolDefinition {
                name: "ble_hid_start".to_string(),
                description: "Start BLE HID profile (wireless keyboard/mouse). Replaces the normal Flipper BLE profile — the Flipper mobile app will disconnect. The Flipper appears as a Bluetooth keyboard/mouse to nearby devices.".to_string(),
                input_schema: json!({
                    "type": "object",
                    "properties": {
                        "name": {
                            "type": "string",
                            "description": "BLE device name advertised to targets (default 'Flipper MCP')",
                            "default": "Flipper MCP"
                        }
                    },
                    "required": []
                }),
            },
            ToolDefinition {
                name: "ble_hid_type".to_string(),
                description: "Type a string as HID keyboard input over BLE. Requires ble_hid_start first. Use \\n for Enter key. Supports printable ASCII (US keyboard layout).".to_string(),
                input_schema: json!({
                    "type": "object",
                    "properties": {
                        "text": {
                            "type": "string",
                            "description": "Text to type (printable ASCII, max ~450 chars, use \\n for Enter)"
                        },
                        "delay": {
                            "type": "integer",
                            "description": "Delay between keystrokes in ms (default 30)",
                            "minimum": 5,
                            "maximum": 500,
                            "default": 30
                        }
                    },
                    "required": ["text"]
                }),
            },
            ToolDefinition {
                name: "ble_hid_press".to_string(),
                description: "Press a key combination over BLE HID. Supports modifiers (CTRL, SHIFT, ALT, GUI/WIN) combined with keys using '+' separator. Examples: 'GUI+r', 'CTRL+SHIFT+ESC', 'ENTER', 'F5'.".to_string(),
                input_schema: json!({
                    "type": "object",
                    "properties": {
                        "key": {
                            "type": "string",
                            "description": "Key combo with '+' separators. Modifiers: CTRL, SHIFT, ALT, GUI, WIN, LCTRL, RCTRL, LSHIFT, RSHIFT, LALT, RALT, LGUI, RGUI. Special: ENTER, TAB, ESC, SPACE, BACKSPACE, DELETE, UP, DOWN, LEFT, RIGHT, HOME, END, PAGEUP, PAGEDOWN, F1-F12, CAPSLOCK, PRINTSCREEN, INSERT."
                        }
                    },
                    "required": ["key"]
                }),
            },
            ToolDefinition {
                name: "ble_hid_mouse".to_string(),
                description: "Control mouse over BLE HID — move cursor, click buttons, scroll wheel. Requires ble_hid_start first.".to_string(),
                input_schema: json!({
                    "type": "object",
                    "properties": {
                        "dx": {
                            "type": "integer",
                            "description": "Horizontal mouse movement (-127 to 127, positive = right)",
                            "minimum": -127,
                            "maximum": 127
                        },
                        "dy": {
                            "type": "integer",
                            "description": "Vertical mouse movement (-127 to 127, positive = down)",
                            "minimum": -127,
                            "maximum": 127
                        },
                        "button": {
                            "type": "string",
                            "description": "Mouse button: 'left', 'right', or 'middle'",
                            "enum": ["left", "right", "middle"]
                        },
                        "action": {
                            "type": "string",
                            "description": "Button action: 'click' (default), 'press', or 'release'",
                            "enum": ["click", "press", "release"],
                            "default": "click"
                        },
                        "scroll": {
                            "type": "integer",
                            "description": "Scroll wheel delta (-127 to 127, positive = down)",
                            "minimum": -127,
                            "maximum": 127
                        }
                    },
                    "required": []
                }),
            },
            ToolDefinition {
                name: "ble_hid_stop".to_string(),
                description: "Stop BLE HID profile, release all keys/buttons, and restore normal Flipper BLE profile".to_string(),
                input_schema: json!({
                    "type": "object",
                    "properties": {},
                    "required": []
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
            "ble_info" => "ble info".to_string(),
            "ble_beacon" => {
                let data = match args.get("data").and_then(|v| v.as_str()) {
                    Some(d) => d,
                    None => return ToolResult::error("Missing required parameter: data"),
                };
                let mut cmd = format!("ble beacon {}", data);
                if let Some(mac) = args.get("mac").and_then(|v| v.as_str()) {
                    cmd.push_str(&format!(" --mac {}", mac));
                }
                if let Some(interval) = args.get("interval").and_then(|v| v.as_i64()) {
                    cmd.push_str(&format!(" --interval {}", interval));
                }
                cmd
            }
            "ble_beacon_stop" => "ble beacon_stop".to_string(),
            "ble_hid_start" => {
                let mut cmd = "ble hid_start".to_string();
                if let Some(name) = args.get("name").and_then(|v| v.as_str()) {
                    cmd.push_str(&format!(" --name {}", name));
                }
                cmd
            }
            "ble_hid_type" => {
                let text = match args.get("text").and_then(|v| v.as_str()) {
                    Some(t) => t,
                    None => return ToolResult::error("Missing required parameter: text"),
                };
                // Truncate to ~450 chars to fit FAP's 512-byte LINE_BUF_SIZE
                let truncated = if text.len() > 450 { &text[..450] } else { text };
                let mut cmd = format!("ble hid_type {}", truncated);
                if let Some(delay) = args.get("delay").and_then(|v| v.as_i64()) {
                    cmd.push_str(&format!(" --delay {}", delay));
                }
                cmd
            }
            "ble_hid_press" => match args.get("key").and_then(|v| v.as_str()) {
                Some(key) => format!("ble hid_press {}", key),
                None => return ToolResult::error("Missing required parameter: key"),
            },
            "ble_hid_mouse" => {
                let mut parts = vec!["ble".to_string(), "hid_mouse".to_string()];
                // dx and dy are positional
                let dx = args.get("dx").and_then(|v| v.as_i64()).unwrap_or(0);
                let dy = args.get("dy").and_then(|v| v.as_i64()).unwrap_or(0);
                parts.push(dx.to_string());
                parts.push(dy.to_string());
                if let Some(button) = args.get("button").and_then(|v| v.as_str()) {
                    parts.push(format!("--button {}", button));
                    if let Some(action) = args.get("action").and_then(|v| v.as_str()) {
                        parts.push(format!("--action {}", action));
                    }
                }
                if let Some(scroll) = args.get("scroll").and_then(|v| v.as_i64()) {
                    parts.push(format!("--scroll {}", scroll));
                }
                parts.join(" ")
            }
            "ble_hid_stop" => "ble hid_stop".to_string(),
            _ => return ToolResult::error(format!("Unknown ble tool: {}", tool)),
        };

        // Per-tool timeouts: quick queries 3s, profile switch 5s, text typing 30s
        let timeout_ms: u32 = match tool {
            "ble_hid_type" => 30_000,
            "ble_hid_start" | "ble_hid_stop" => 5_000,
            _ => 3_000,
        };

        match protocol.execute_command_with_timeout(&command, timeout_ms) {
            Ok(output) => ToolResult::success(output),
            Err(e) => ToolResult::error(format!("{} failed: {}", tool, e)),
        }
    }
}
