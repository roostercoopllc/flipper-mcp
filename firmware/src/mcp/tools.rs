use std::sync::{Arc, Mutex};

use log::warn;
use serde_json::{json, Value};

use crate::uart::FlipperProtocol;

use super::types::{ToolDefinition, ToolResult};

pub struct ToolRegistry {
    tools: Vec<ToolDefinition>,
    protocol: Arc<Mutex<dyn FlipperProtocol>>,
}

impl ToolRegistry {
    pub fn new(protocol: Arc<Mutex<dyn FlipperProtocol>>) -> Self {
        let mut registry = Self {
            tools: Vec::new(),
            protocol,
        };
        registry.register_builtin_tools();
        registry
    }

    fn register_builtin_tools(&mut self) {
        self.tools.push(ToolDefinition {
            name: "system_info".to_string(),
            description: "Get Flipper Zero device information (hardware, firmware, etc.)".to_string(),
            input_schema: json!({
                "type": "object",
                "properties": {},
                "required": []
            }),
        });

        self.tools.push(ToolDefinition {
            name: "execute_command".to_string(),
            description: "Execute a raw CLI command on the Flipper Zero and return the output".to_string(),
            input_schema: json!({
                "type": "object",
                "properties": {
                    "command": {
                        "type": "string",
                        "description": "The CLI command to execute (e.g. 'power info', 'ps', 'free')"
                    }
                },
                "required": ["command"]
            }),
        });
    }

    pub fn list_tools(&self) -> Value {
        json!({ "tools": self.tools })
    }

    pub fn call_tool(&self, name: &str, args: &Value) -> ToolResult {
        match name {
            "system_info" => self.tool_system_info(),
            "execute_command" => self.tool_execute_command(args),
            _ => {
                warn!("Unknown tool: {}", name);
                ToolResult::error(format!("Unknown tool: {}", name))
            }
        }
    }

    fn tool_system_info(&self) -> ToolResult {
        let mut protocol = self.protocol.lock().unwrap();
        match protocol.get_device_info() {
            Ok(info) => ToolResult::success(info),
            Err(e) => ToolResult::error(format!("Failed to get device info: {}", e)),
        }
    }

    fn tool_execute_command(&self, args: &Value) -> ToolResult {
        let command = match args.get("command").and_then(|v| v.as_str()) {
            Some(cmd) => cmd,
            None => return ToolResult::error("Missing required parameter: command"),
        };

        let mut protocol = self.protocol.lock().unwrap();
        match protocol.execute_command(command) {
            Ok(output) => ToolResult::success(output),
            Err(e) => ToolResult::error(format!("Command failed: {}", e)),
        }
    }
}
