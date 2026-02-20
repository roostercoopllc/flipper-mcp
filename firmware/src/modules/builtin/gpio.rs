use serde_json::{json, Value};

use crate::mcp::types::{ToolDefinition, ToolResult};
use crate::modules::traits::FlipperModule;
use crate::uart::FlipperProtocol;

pub struct GpioModule;

impl FlipperModule for GpioModule {
    fn name(&self) -> &str {
        "gpio"
    }

    fn description(&self) -> &str {
        "GPIO pin control (set, read, mode)"
    }

    fn tools(&self) -> Vec<ToolDefinition> {
        vec![
            ToolDefinition {
                name: "gpio_set".to_string(),
                description: "Set a GPIO pin to high (1) or low (0)".to_string(),
                input_schema: json!({
                    "type": "object",
                    "properties": {
                        "pin": { "type": "string", "description": "Pin name (e.g. 'PC3', 'PB2', 'PA4')" },
                        "value": { "type": "integer", "description": "Pin value: 0 (low) or 1 (high)", "enum": [0, 1] }
                    },
                    "required": ["pin", "value"]
                }),
            },
            ToolDefinition {
                name: "gpio_read".to_string(),
                description: "Read the current value of a GPIO pin".to_string(),
                input_schema: json!({
                    "type": "object",
                    "properties": {
                        "pin": { "type": "string", "description": "Pin name (e.g. 'PC3', 'PB2', 'PA4')" }
                    },
                    "required": ["pin"]
                }),
            },
            ToolDefinition {
                name: "gpio_mode".to_string(),
                description: "Set the mode of a GPIO pin (input, output, etc.)".to_string(),
                input_schema: json!({
                    "type": "object",
                    "properties": {
                        "pin": { "type": "string", "description": "Pin name (e.g. 'PC3', 'PB2', 'PA4')" },
                        "mode": { "type": "string", "description": "Pin mode (e.g. '0' for input, '1' for output)" }
                    },
                    "required": ["pin", "mode"]
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
            "gpio_set" => {
                let pin = args.get("pin").and_then(|v| v.as_str());
                let value = args.get("value").and_then(|v| v.as_i64());
                match (pin, value) {
                    (Some(p), Some(v)) => format!("gpio set {} {}", p, v),
                    _ => return ToolResult::error("Missing required parameters: pin, value"),
                }
            }
            "gpio_read" => match args.get("pin").and_then(|v| v.as_str()) {
                Some(p) => format!("gpio read {}", p),
                None => return ToolResult::error("Missing required parameter: pin"),
            },
            "gpio_mode" => {
                let pin = args.get("pin").and_then(|v| v.as_str());
                let mode = args.get("mode").and_then(|v| v.as_str());
                match (pin, mode) {
                    (Some(p), Some(m)) => format!("gpio mode {} {}", p, m),
                    _ => return ToolResult::error("Missing required parameters: pin, mode"),
                }
            }
            _ => return ToolResult::error(format!("Unknown gpio tool: {}", tool)),
        };

        match protocol.execute_command(&command) {
            Ok(output) => ToolResult::success(output),
            Err(e) => ToolResult::error(format!("{} failed: {}", tool, e)),
        }
    }
}
