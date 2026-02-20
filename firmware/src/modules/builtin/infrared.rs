use serde_json::{json, Value};

use crate::mcp::types::{ToolDefinition, ToolResult};
use crate::modules::traits::FlipperModule;
use crate::uart::FlipperProtocol;

pub struct InfraredModule;

impl FlipperModule for InfraredModule {
    fn name(&self) -> &str {
        "infrared"
    }

    fn description(&self) -> &str {
        "Infrared signal transmission"
    }

    fn tools(&self) -> Vec<ToolDefinition> {
        vec![ToolDefinition {
            name: "ir_tx".to_string(),
            description: "Transmit an infrared signal with the specified protocol, address, and command".to_string(),
            input_schema: json!({
                "type": "object",
                "properties": {
                    "protocol": { "type": "string", "description": "IR protocol (e.g. 'NEC', 'Samsung', 'RC5', 'RC6')" },
                    "address": { "type": "string", "description": "Device address (hex string, e.g. '0x04')" },
                    "command": { "type": "string", "description": "Command code (hex string, e.g. '0x08')" }
                },
                "required": ["protocol", "address", "command"]
            }),
        }]
    }

    fn execute(
        &self,
        tool: &str,
        args: &Value,
        protocol: &mut dyn FlipperProtocol,
    ) -> ToolResult {
        match tool {
            "ir_tx" => {
                let ir_protocol = args.get("protocol").and_then(|v| v.as_str());
                let address = args.get("address").and_then(|v| v.as_str());
                let command = args.get("command").and_then(|v| v.as_str());
                match (ir_protocol, address, command) {
                    (Some(p), Some(a), Some(c)) => {
                        let cmd = format!("ir tx {} {} {}", p, a, c);
                        match protocol.execute_command(&cmd) {
                            Ok(output) => ToolResult::success(output),
                            Err(e) => ToolResult::error(format!("ir_tx failed: {}", e)),
                        }
                    }
                    _ => ToolResult::error("Missing required parameters: protocol, address, command"),
                }
            }
            _ => ToolResult::error(format!("Unknown infrared tool: {}", tool)),
        }
    }
}
