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
        "125kHz RFID tag read, emulate, and write"
    }

    fn tools(&self) -> Vec<ToolDefinition> {
        vec![
            ToolDefinition {
                name: "rfid_read".to_string(),
                description: "Read a 125kHz RFID tag held near the Flipper".to_string(),
                input_schema: json!({ "type": "object", "properties": {}, "required": [] }),
            },
            ToolDefinition {
                name: "rfid_emulate".to_string(),
                description: "Emulate a 125kHz RFID tag with the specified type and data"
                    .to_string(),
                input_schema: json!({
                    "type": "object",
                    "properties": {
                        "type": { "type": "string", "description": "Tag type (e.g. 'EM4100', 'HIDProx', 'Indala')" },
                        "data": { "type": "string", "description": "Tag data (hex string)" }
                    },
                    "required": ["type", "data"]
                }),
            },
            ToolDefinition {
                name: "rfid_write".to_string(),
                description: "Write data to a 125kHz RFID tag".to_string(),
                input_schema: json!({
                    "type": "object",
                    "properties": {
                        "type": { "type": "string", "description": "Tag type (e.g. 'EM4100', 'T5577')" },
                        "data": { "type": "string", "description": "Data to write (hex string)" }
                    },
                    "required": ["type", "data"]
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
            "rfid_emulate" => {
                let tag_type = args.get("type").and_then(|v| v.as_str());
                let data = args.get("data").and_then(|v| v.as_str());
                match (tag_type, data) {
                    (Some(t), Some(d)) => format!("rfid emulate {} {}", t, d),
                    _ => return ToolResult::error("Missing required parameters: type, data"),
                }
            }
            "rfid_write" => {
                let tag_type = args.get("type").and_then(|v| v.as_str());
                let data = args.get("data").and_then(|v| v.as_str());
                match (tag_type, data) {
                    (Some(t), Some(d)) => format!("rfid write {} {}", t, d),
                    _ => return ToolResult::error("Missing required parameters: type, data"),
                }
            }
            _ => return ToolResult::error(format!("Unknown rfid tool: {}", tool)),
        };

        match protocol.execute_command(&command) {
            Ok(output) => ToolResult::success(output),
            Err(e) => ToolResult::error(format!("{} failed: {}", tool, e)),
        }
    }
}
