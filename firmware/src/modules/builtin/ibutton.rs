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
        "iButton (1-Wire) key read and emulate"
    }

    fn tools(&self) -> Vec<ToolDefinition> {
        vec![
            ToolDefinition {
                name: "ibutton_read".to_string(),
                description: "Read an iButton key held against the Flipper".to_string(),
                input_schema: json!({ "type": "object", "properties": {}, "required": [] }),
            },
            ToolDefinition {
                name: "ibutton_emulate".to_string(),
                description: "Emulate an iButton key with the specified type and data".to_string(),
                input_schema: json!({
                    "type": "object",
                    "properties": {
                        "type": { "type": "string", "description": "Key type (e.g. 'Dallas', 'Cyfral', 'Metakom')" },
                        "data": { "type": "string", "description": "Key data (hex string)" }
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
            "ibutton_read" => "ikey read".to_string(),
            "ibutton_emulate" => {
                let key_type = args.get("type").and_then(|v| v.as_str());
                let data = args.get("data").and_then(|v| v.as_str());
                match (key_type, data) {
                    (Some(t), Some(d)) => format!("ikey emulate {} {}", t, d),
                    _ => return ToolResult::error("Missing required parameters: type, data"),
                }
            }
            _ => return ToolResult::error(format!("Unknown ibutton tool: {}", tool)),
        };

        match protocol.execute_command(&command) {
            Ok(output) => ToolResult::success(output),
            Err(e) => ToolResult::error(format!("{} failed: {}", tool, e)),
        }
    }
}
