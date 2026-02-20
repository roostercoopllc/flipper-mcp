use std::sync::{Arc, Mutex};

use log::{info, warn};
use serde_json::{json, Value};

use crate::mcp::types::{ToolDefinition, ToolResult};
use crate::uart::FlipperProtocol;

use super::builtin;
use super::traits::FlipperModule;

pub struct ModuleRegistry {
    modules: Vec<Box<dyn FlipperModule>>,
    protocol: Arc<Mutex<dyn FlipperProtocol>>,
}

impl ModuleRegistry {
    pub fn new(protocol: Arc<Mutex<dyn FlipperProtocol>>) -> Self {
        let modules = builtin::register_all();
        info!("Registered {} modules with {} tools total",
            modules.len(),
            modules.iter().map(|m| m.tools().len()).sum::<usize>()
        );
        Self { modules, protocol }
    }

    pub fn list_all_tools(&self) -> Vec<ToolDefinition> {
        let mut tools: Vec<ToolDefinition> = self
            .modules
            .iter()
            .flat_map(|m| m.tools())
            .collect();

        // Add the execute_command passthrough tool
        tools.push(ToolDefinition {
            name: "execute_command".to_string(),
            description: "Execute a raw CLI command on the Flipper Zero and return the output"
                .to_string(),
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

        tools
    }

    pub fn call_tool(&self, name: &str, args: &Value) -> ToolResult {
        // Handle the passthrough tool
        if name == "execute_command" {
            return self.execute_passthrough(args);
        }

        // Find which module owns this tool
        for module in &self.modules {
            let owns_tool = module.tools().iter().any(|t| t.name == name);
            if owns_tool {
                let mut protocol = self.protocol.lock().unwrap();
                return module.execute(name, args, &mut *protocol);
            }
        }

        warn!("Unknown tool: {}", name);
        ToolResult::error(format!("Unknown tool: {}", name))
    }

    fn execute_passthrough(&self, args: &Value) -> ToolResult {
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
