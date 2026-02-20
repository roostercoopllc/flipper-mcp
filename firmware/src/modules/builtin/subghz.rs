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
        "Sub-GHz radio transmit, receive, and decode"
    }

    fn tools(&self) -> Vec<ToolDefinition> {
        vec![
            ToolDefinition {
                name: "subghz_tx".to_string(),
                description: "Transmit a Sub-GHz signal with the specified protocol, key, and frequency".to_string(),
                input_schema: json!({
                    "type": "object",
                    "properties": {
                        "protocol": { "type": "string", "description": "Protocol name (e.g. 'Princeton', 'Nice FLO')" },
                        "key": { "type": "string", "description": "Key/data to transmit (hex string)" },
                        "frequency": { "type": "integer", "description": "Frequency in Hz (e.g. 433920000)" }
                    },
                    "required": ["protocol", "key", "frequency"]
                }),
            },
            ToolDefinition {
                name: "subghz_rx".to_string(),
                description: "Receive and decode Sub-GHz signals at the specified frequency".to_string(),
                input_schema: json!({
                    "type": "object",
                    "properties": {
                        "frequency": { "type": "integer", "description": "Frequency in Hz (e.g. 433920000)" }
                    },
                    "required": ["frequency"]
                }),
            },
            ToolDefinition {
                name: "subghz_rx_raw".to_string(),
                description: "Receive raw Sub-GHz signal data at the specified frequency".to_string(),
                input_schema: json!({
                    "type": "object",
                    "properties": {
                        "frequency": { "type": "integer", "description": "Frequency in Hz (e.g. 433920000)" }
                    },
                    "required": ["frequency"]
                }),
            },
            ToolDefinition {
                name: "subghz_decode_raw".to_string(),
                description: "Decode a raw Sub-GHz capture file from the SD card".to_string(),
                input_schema: json!({
                    "type": "object",
                    "properties": {
                        "file": { "type": "string", "description": "Path to the .sub file on the Flipper SD card" }
                    },
                    "required": ["file"]
                }),
            },
            ToolDefinition {
                name: "subghz_chat".to_string(),
                description: "Start Sub-GHz chat mode at the specified frequency".to_string(),
                input_schema: json!({
                    "type": "object",
                    "properties": {
                        "frequency": { "type": "integer", "description": "Frequency in Hz (e.g. 433920000)" }
                    },
                    "required": ["frequency"]
                }),
            },
            ToolDefinition {
                name: "subghz_tx_from_file".to_string(),
                description: "Transmit a Sub-GHz signal from a .sub file on the SD card".to_string(),
                input_schema: json!({
                    "type": "object",
                    "properties": {
                        "file": { "type": "string", "description": "Path to the .sub file on the Flipper SD card" }
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
                let protocol_name = require_str(args, "protocol");
                let key = require_str(args, "key");
                let frequency = require_int(args, "frequency");
                match (protocol_name, key, frequency) {
                    (Some(p), Some(k), Some(f)) => format!("subghz tx {} {} {}", p, k, f),
                    _ => return ToolResult::error("Missing required parameters: protocol, key, frequency"),
                }
            }
            "subghz_rx" => match require_int(args, "frequency") {
                Some(f) => format!("subghz rx {}", f),
                None => return ToolResult::error("Missing required parameter: frequency"),
            },
            "subghz_rx_raw" => match require_int(args, "frequency") {
                Some(f) => format!("subghz rx_raw {}", f),
                None => return ToolResult::error("Missing required parameter: frequency"),
            },
            "subghz_decode_raw" => match require_str(args, "file") {
                Some(f) => format!("subghz decode_raw {}", f),
                None => return ToolResult::error("Missing required parameter: file"),
            },
            "subghz_chat" => match require_int(args, "frequency") {
                Some(f) => format!("subghz chat {}", f),
                None => return ToolResult::error("Missing required parameter: frequency"),
            },
            "subghz_tx_from_file" => match require_str(args, "file") {
                Some(f) => format!("subghz tx_from_file {}", f),
                None => return ToolResult::error("Missing required parameter: file"),
            },
            _ => return ToolResult::error(format!("Unknown subghz tool: {}", tool)),
        };

        match protocol.execute_command(&command) {
            Ok(output) => ToolResult::success(output),
            Err(e) => ToolResult::error(format!("{} failed: {}", tool, e)),
        }
    }
}

fn require_str<'a>(args: &'a Value, key: &str) -> Option<&'a str> {
    args.get(key).and_then(|v| v.as_str())
}

fn require_int(args: &Value, key: &str) -> Option<i64> {
    args.get(key).and_then(|v| v.as_i64())
}
