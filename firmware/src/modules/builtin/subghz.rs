use serde_json::{json, Value};

use crate::mcp::types::{ToolDefinition, ToolResult};
use crate::modules::traits::FlipperModule;
use crate::uart::FlipperProtocol;

pub struct SubGhzModule;

impl FlipperModule for SubGhzModule {
    fn name(&self) -> &str {
        "subghz"
    }

    fn description(&self) -> &str {
        "Sub-GHz radio transmit, receive, and replay (315/433/868 MHz, CC1101)"
    }

    fn tools(&self) -> Vec<ToolDefinition> {
        vec![
            ToolDefinition {
                name: "subghz_tx".to_string(),
                description: "Transmit a Sub-GHz signal with the specified protocol, key, and frequency. Supports Princeton, Nice FLO, CAME, Linear, and other static protocols."
                    .to_string(),
                input_schema: json!({
                    "type": "object",
                    "properties": {
                        "protocol": { "type": "string", "description": "Protocol name (e.g. 'Princeton', 'Nice FLO', 'CAME', 'Linear')" },
                        "key": { "type": "string", "description": "Key/data to transmit (hex string, e.g. '000001')" },
                        "frequency": { "type": "integer", "description": "Frequency in Hz (e.g. 433920000 for 433.92 MHz)" }
                    },
                    "required": ["protocol", "key", "frequency"]
                }),
            },
            ToolDefinition {
                name: "subghz_rx".to_string(),
                description: "Listen for Sub-GHz signals at the specified frequency and decode any recognized protocols. Returns the first decoded signal or times out."
                    .to_string(),
                input_schema: json!({
                    "type": "object",
                    "properties": {
                        "frequency": { "type": "integer", "description": "Frequency in Hz (e.g. 433920000)" },
                        "duration": { "type": "integer", "description": "Listen duration in ms (1000-30000, default 5000)", "minimum": 1000, "maximum": 30000, "default": 5000 }
                    },
                    "required": ["frequency"]
                }),
            },
            ToolDefinition {
                name: "subghz_tx_from_file".to_string(),
                description: "Transmit a Sub-GHz signal from a .sub file on the Flipper SD card. The file contains frequency, preset, and signal data."
                    .to_string(),
                input_schema: json!({
                    "type": "object",
                    "properties": {
                        "file": { "type": "string", "description": "Path to the .sub file on the Flipper SD card (e.g. '/ext/subghz/my_signal.sub')" }
                    },
                    "required": ["file"]
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
            "subghz_tx" => {
                let protocol_name = args.get("protocol").and_then(|v| v.as_str());
                let key = args.get("key").and_then(|v| v.as_str());
                let frequency = args.get("frequency").and_then(|v| v.as_i64());
                match (protocol_name, key, frequency) {
                    (Some(p), Some(k), Some(f)) => format!("subghz tx {} {} {}", p, k, f),
                    _ => {
                        return ToolResult::error(
                            "Missing required parameters: protocol, key, frequency",
                        )
                    }
                }
            }
            "subghz_rx" => {
                let freq = match args.get("frequency").and_then(|v| v.as_i64()) {
                    Some(f) => f,
                    None => return ToolResult::error("Missing required parameter: frequency"),
                };
                let duration = args.get("duration").and_then(|v| v.as_i64()).unwrap_or(5000);
                format!("subghz rx {} {}", freq, duration)
            }
            "subghz_tx_from_file" => match args.get("file").and_then(|v| v.as_str()) {
                Some(f) => format!("subghz tx_from_file {}", f),
                None => return ToolResult::error("Missing required parameter: file"),
            },
            _ => return ToolResult::error(format!("Unknown subghz tool: {}", tool)),
        };

        // RX can block for up to 30s, TX from file up to 10s, quick TX 5s
        let timeout_ms: u32 = match tool {
            "subghz_rx" => 35_000,
            "subghz_tx_from_file" => 12_000,
            _ => 7_000,
        };

        match protocol.execute_command_with_timeout(&command, timeout_ms) {
            Ok(output) => ToolResult::success(output),
            Err(e) => ToolResult::error(format!("{} failed: {}", tool, e)),
        }
    }
}
