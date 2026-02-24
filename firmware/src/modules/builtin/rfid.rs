use serde_json::{json, Value};

use crate::mcp::types::{ToolDefinition, ToolResult};
use crate::modules::traits::FlipperModule;
use crate::uart::FlipperProtocol;

pub struct RfidModule;

impl FlipperModule for RfidModule {
    fn name(&self) -> &str {
        "rfid"
    }

    fn description(&self) -> &str {
        "125kHz RFID tag read, save, and emulate (EM4100, HID Prox, Indala, etc.)"
    }

    fn tools(&self) -> Vec<ToolDefinition> {
        vec![
            ToolDefinition {
                name: "rfid_read".to_string(),
                description: "Read a 125kHz RFID tag held near the Flipper. Auto-detects protocol (EM4100, HID Prox, Indala, etc.). Times out after 10 seconds."
                    .to_string(),
                input_schema: json!({ "type": "object", "properties": {}, "required": [] }),
            },
            ToolDefinition {
                name: "rfid_read_and_save".to_string(),
                description: "Read a 125kHz RFID tag and save it to a file on the Flipper SD card. The saved file can later be used with rfid_emulate."
                    .to_string(),
                input_schema: json!({
                    "type": "object",
                    "properties": {
                        "path": { "type": "string", "description": "Save path on Flipper SD card (e.g. '/ext/lfrfid/my_tag.rfid')" }
                    },
                    "required": ["path"]
                }),
            },
            ToolDefinition {
                name: "rfid_emulate".to_string(),
                description: "Emulate a 125kHz RFID tag from a saved file. The Flipper's coil will broadcast this tag for 10 seconds."
                    .to_string(),
                input_schema: json!({
                    "type": "object",
                    "properties": {
                        "path": { "type": "string", "description": "Path to .rfid file on Flipper SD card (e.g. '/ext/lfrfid/my_tag.rfid')" }
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
            "rfid_read" => "rfid read".to_string(),
            "rfid_read_and_save" => match args.get("path").and_then(|v| v.as_str()) {
                Some(path) => format!("rfid read_and_save {}", path),
                None => return ToolResult::error("Missing required parameter: path"),
            },
            "rfid_emulate" => match args.get("path").and_then(|v| v.as_str()) {
                Some(path) => format!("rfid emulate {}", path),
                None => return ToolResult::error("Missing required parameter: path"),
            },
            _ => return ToolResult::error(format!("Unknown rfid tool: {}", tool)),
        };

        let timeout_ms: u32 = match tool {
            "rfid_emulate" => 12_000,
            _ => 12_000,
        };

        match protocol.execute_command_with_timeout(&command, timeout_ms) {
            Ok(output) => ToolResult::success(output),
            Err(e) => ToolResult::error(format!("{} failed: {}", tool, e)),
        }
    }
}
