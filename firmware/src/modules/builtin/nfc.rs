use serde_json::{json, Value};

use crate::mcp::types::{ToolDefinition, ToolResult};
use crate::modules::traits::FlipperModule;
use crate::uart::FlipperProtocol;

pub struct NfcModule;

impl FlipperModule for NfcModule {
    fn name(&self) -> &str {
        "nfc"
    }

    fn description(&self) -> &str {
        "NFC tag detection and emulation (ISO14443-A/B, MIFARE, NTAG, Felica, ISO15693)"
    }

    fn tools(&self) -> Vec<ToolDefinition> {
        vec![
            ToolDefinition {
                name: "nfc_detect".to_string(),
                description: "Detect and identify an NFC tag held near the Flipper. Returns protocol types detected. Times out after 10 seconds."
                    .to_string(),
                input_schema: json!({ "type": "object", "properties": {}, "required": [] }),
            },
            ToolDefinition {
                name: "nfc_emulate".to_string(),
                description: "Emulate an NFC tag from a saved file. The Flipper will respond as this tag for 30 seconds when an NFC reader is presented."
                    .to_string(),
                input_schema: json!({
                    "type": "object",
                    "properties": {
                        "path": { "type": "string", "description": "Path to .nfc file on Flipper SD card (e.g. '/ext/nfc/my_tag.nfc')" }
                    },
                    "required": ["path"]
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
            "nfc_detect" => "nfc detect".to_string(),
            "nfc_emulate" => match args.get("path").and_then(|v| v.as_str()) {
                Some(path) => format!("nfc emulate {}", path),
                None => return ToolResult::error("Missing required parameter: path"),
            },
            _ => return ToolResult::error(format!("Unknown nfc tool: {}", tool)),
        };

        let timeout_ms: u32 = match tool {
            "nfc_emulate" => 32_000,
            _ => 12_000,
        };

        match protocol.execute_command_with_timeout(&command, timeout_ms) {
            Ok(output) => ToolResult::success(output),
            Err(e) => ToolResult::error(format!("{} failed: {}", tool, e)),
        }
    }
}
