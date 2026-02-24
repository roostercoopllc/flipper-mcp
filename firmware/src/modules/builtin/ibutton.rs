use serde_json::{json, Value};

use crate::mcp::types::{ToolDefinition, ToolResult};
use crate::modules::traits::FlipperModule;
use crate::uart::FlipperProtocol;

pub struct IButtonModule;

impl FlipperModule for IButtonModule {
    fn name(&self) -> &str {
        "ibutton"
    }

    fn description(&self) -> &str {
        "iButton (1-Wire) key read, save, and emulate (Dallas, Cyfral, Metakom)"
    }

    fn tools(&self) -> Vec<ToolDefinition> {
        vec![
            ToolDefinition {
                name: "ibutton_read".to_string(),
                description: "Read an iButton key held against the Flipper's 1-Wire contact. Returns protocol type and UID. Times out after 10 seconds."
                    .to_string(),
                input_schema: json!({ "type": "object", "properties": {}, "required": [] }),
            },
            ToolDefinition {
                name: "ibutton_read_and_save".to_string(),
                description: "Read an iButton key and save it to a file on the Flipper SD card. The saved file can later be used with ibutton_emulate."
                    .to_string(),
                input_schema: json!({
                    "type": "object",
                    "properties": {
                        "path": { "type": "string", "description": "Save path on Flipper SD card (e.g. '/ext/ibutton/my_key.ibtn')" }
                    },
                    "required": ["path"]
                }),
            },
            ToolDefinition {
                name: "ibutton_emulate".to_string(),
                description: "Emulate an iButton key from a saved file. The Flipper will present this key on its 1-Wire contact for 10 seconds."
                    .to_string(),
                input_schema: json!({
                    "type": "object",
                    "properties": {
                        "path": { "type": "string", "description": "Path to .ibtn file on Flipper SD card (e.g. '/ext/ibutton/my_key.ibtn')" }
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
            "ibutton_read" => "ikey read".to_string(),
            "ibutton_read_and_save" => match args.get("path").and_then(|v| v.as_str()) {
                Some(path) => format!("ikey read_and_save {}", path),
                None => return ToolResult::error("Missing required parameter: path"),
            },
            "ibutton_emulate" => match args.get("path").and_then(|v| v.as_str()) {
                Some(path) => format!("ikey emulate {}", path),
                None => return ToolResult::error("Missing required parameter: path"),
            },
            _ => return ToolResult::error(format!("Unknown ibutton tool: {}", tool)),
        };

        // Read/emulate operations block for up to 10s on the Flipper
        let timeout_ms: u32 = match tool {
            "ibutton_emulate" => 12_000,
            _ => 12_000,
        };

        match protocol.execute_command_with_timeout(&command, timeout_ms) {
            Ok(output) => ToolResult::success(output),
            Err(e) => ToolResult::error(format!("{} failed: {}", tool, e)),
        }
    }
}
