use std::sync::{Arc, Mutex};

use log::{info, warn};
use serde_json::{json, Value};

use crate::mcp::types::{ToolDefinition, ToolResult};
use crate::uart::FlipperProtocol;

use super::builtin;
use super::config;
use super::discovery;
use super::traits::FlipperModule;

pub struct ModuleRegistry {
    /// Built-in modules — set once at construction, never change.
    static_modules: Vec<Box<dyn FlipperModule>>,
    /// FAP-discovered + config-driven modules — replaced on refresh.
    /// Lock ordering: always acquire `protocol` BEFORE `dynamic_modules` to avoid deadlock
    /// (refresh() also acquires protocol first).
    dynamic_modules: Mutex<Vec<Box<dyn FlipperModule>>>,
    protocol: Arc<Mutex<dyn FlipperProtocol>>,
}

impl ModuleRegistry {
    pub fn new(protocol: Arc<Mutex<dyn FlipperProtocol>>) -> Self {
        let static_modules = builtin::register_all();
        info!(
            "Registered {} built-in modules ({} tools)",
            static_modules.len(),
            static_modules.iter().map(|m| m.tools().len()).sum::<usize>()
        );

        let registry = Self {
            static_modules,
            dynamic_modules: Mutex::new(Vec::new()),
            protocol,
        };

        // Run initial dynamic discovery at startup
        registry.refresh();
        registry
    }

    /// Re-scan FAP apps and reload config modules.
    /// Lock order: protocol → dynamic_modules (same as call_tool for dynamic).
    pub fn refresh(&self) {
        // Acquire protocol first so all UART communication is done before updating the list
        let mut proto = self.protocol.lock().unwrap();
        let mut new_dynamic: Vec<Box<dyn FlipperModule>> = Vec::new();

        new_dynamic.extend(discovery::scan_fap_apps(&mut *proto));
        new_dynamic.extend(config::load_config_modules(&mut *proto));

        info!(
            "Dynamic modules refreshed: {} module(s), {} tool(s)",
            new_dynamic.len(),
            new_dynamic.iter().map(|m| m.tools().len()).sum::<usize>()
        );

        *self.dynamic_modules.lock().unwrap() = new_dynamic;
    }

    pub fn list_all_tools(&self) -> Vec<ToolDefinition> {
        let mut tools: Vec<ToolDefinition> = self
            .static_modules
            .iter()
            .flat_map(|m| m.tools())
            .collect();

        // Include dynamic (FAP + config) tools
        let dynamic = self.dynamic_modules.lock().unwrap();
        tools.extend(dynamic.iter().flat_map(|m| m.tools()));
        drop(dynamic);

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
        // Passthrough tool — no UART needed for param validation, but execute_command needs proto
        if name == "execute_command" {
            return self.execute_passthrough(args);
        }

        // Search static modules (immutable, no dynamic lock needed)
        for module in &self.static_modules {
            if module.tools().iter().any(|t| t.name == name) {
                let mut protocol = self.protocol.lock().unwrap();
                return module.execute(name, args, &mut *protocol);
            }
        }

        // Search dynamic modules.
        // Lock order: protocol first, then dynamic_modules — same as refresh() — prevents deadlock.
        {
            let mut protocol = self.protocol.lock().unwrap();
            let dynamic = self.dynamic_modules.lock().unwrap();
            for module in dynamic.iter() {
                if module.tools().iter().any(|t| t.name == name) {
                    return module.execute(name, args, &mut *protocol);
                }
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
