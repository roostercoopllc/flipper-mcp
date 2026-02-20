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
        "NFC tag detection, emulation, and field output"
    }

    fn tools(&self) -> Vec<ToolDefinition> {
        vec![
            ToolDefinition {
                name: "nfc_detect".to_string(),
                description: "Detect and read an NFC tag held near the Flipper".to_string(),
                input_schema: json!({ "type": "object", "properties": {}, "required": [] }),
            },
            ToolDefinition {
                name: "nfc_emulate".to_string(),
                description: "Emulate the last read NFC tag".to_string(),
                input_schema: json!({ "type": "object", "properties": {}, "required": [] }),
            },
            ToolDefinition {
                name: "nfc_field".to_string(),
                description: "Enable NFC field output (for powering passive tags)".to_string(),
                input_schema: json!({ "type": "object", "properties": {}, "required": [] }),
            },
        ]
    }

    fn execute(
        &self,
        tool: &str,
        _args: &Value,
        protocol: &mut dyn FlipperProtocol,
    ) -> ToolResult {
        let command = match tool {
            "nfc_detect" => "nfc detect",
            "nfc_emulate" => "nfc emulate",
            "nfc_field" => "nfc field",
            _ => return ToolResult::error(format!("Unknown nfc tool: {}", tool)),
        };

        match protocol.execute_command(command) {
            Ok(output) => ToolResult::success(output),
            Err(e) => ToolResult::error(format!("{} failed: {}", tool, e)),
        }
    }
}
