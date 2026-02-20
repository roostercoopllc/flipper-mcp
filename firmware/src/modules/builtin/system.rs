use serde_json::{json, Value};

use crate::mcp::types::{ToolDefinition, ToolResult};
use crate::modules::traits::FlipperModule;
use crate::uart::FlipperProtocol;

pub struct SystemModule;

impl FlipperModule for SystemModule {
    fn name(&self) -> &str {
        "system"
    }

    fn description(&self) -> &str {
        "System information and power management"
    }

    fn tools(&self) -> Vec<ToolDefinition> {
        vec![
            ToolDefinition {
                name: "system_device_info".to_string(),
                description: "Get Flipper Zero device information (hardware, firmware, etc.)"
                    .to_string(),
                input_schema: json!({ "type": "object", "properties": {}, "required": [] }),
            },
            ToolDefinition {
                name: "system_power_info".to_string(),
                description: "Get battery and power supply status".to_string(),
                input_schema: json!({ "type": "object", "properties": {}, "required": [] }),
            },
            ToolDefinition {
                name: "system_power_off".to_string(),
                description: "Power off the Flipper Zero".to_string(),
                input_schema: json!({ "type": "object", "properties": {}, "required": [] }),
            },
            ToolDefinition {
                name: "system_power_reboot".to_string(),
                description: "Reboot the Flipper Zero".to_string(),
                input_schema: json!({ "type": "object", "properties": {}, "required": [] }),
            },
            ToolDefinition {
                name: "system_ps".to_string(),
                description: "List running processes/threads on the Flipper Zero".to_string(),
                input_schema: json!({ "type": "object", "properties": {}, "required": [] }),
            },
            ToolDefinition {
                name: "system_free".to_string(),
                description: "Show memory usage (heap free/total)".to_string(),
                input_schema: json!({ "type": "object", "properties": {}, "required": [] }),
            },
            ToolDefinition {
                name: "system_uptime".to_string(),
                description: "Show device uptime".to_string(),
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
            "system_device_info" => "device_info",
            "system_power_info" => "power info",
            "system_power_off" => "power off",
            "system_power_reboot" => "power reboot",
            "system_ps" => "ps",
            "system_free" => "free",
            "system_uptime" => "uptime",
            _ => return ToolResult::error(format!("Unknown system tool: {}", tool)),
        };

        match protocol.execute_command(command) {
            Ok(output) => ToolResult::success(output),
            Err(e) => ToolResult::error(format!("{} failed: {}", tool, e)),
        }
    }
}
