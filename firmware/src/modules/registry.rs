use std::sync::{Arc, Mutex};

use log::{info, warn};
use serde_json::{json, Value};

use crate::mcp::types::{ToolDefinition, ToolResult};
use crate::uart::FlipperProtocol;

use super::builtin;
use super::c_tool;
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
        new_dynamic.extend(config::load_custom_code_modules(&mut *proto));

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

        // Add the register_c_tool meta-tool
        tools.push(ToolDefinition {
            name: "register_c_tool".to_string(),
            description: concat!(
                "Register a new MCP tool by providing a pseudo-C function definition. ",
                "The tool is saved to the Flipper SD card and immediately available. ",
                "Format:\n",
                "  // description: What the tool does\n",
                "  void tool_name(string param1, integer param2) {\n",
                "      // exec: cli command {param1} {param2}\n",
                "      // optional: param2\n",
                "  }\n",
                "Supported param types: string, integer, boolean. ",
                "All params are required unless marked with '// optional: name'. ",
                "The '// exec:' line is the CLI command template with {param} placeholders."
            ).to_string(),
            input_schema: json!({
                "type": "object",
                "properties": {
                    "code": {
                        "type": "string",
                        "description": "The pseudo-C function definition"
                    }
                },
                "required": ["code"]
            }),
        });

        tools
    }

    pub fn call_tool(&self, name: &str, args: &Value) -> ToolResult {
        // Special-dispatch tools — handled at registry level (need &self access to protocol + dynamic_modules)
        if name == "execute_command" {
            return self.execute_passthrough(args);
        }
        if name == "register_c_tool" {
            return self.handle_register_c_tool(args);
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

    /// Parse a pseudo-C function, save it to the Flipper SD card, and refresh the registry.
    fn handle_register_c_tool(&self, args: &Value) -> ToolResult {
        let code = match args.get("code").and_then(|v| v.as_str()) {
            Some(c) => c,
            None => return ToolResult::error("Missing required parameter: code"),
        };

        let parsed = match c_tool::parse_c_tool(code) {
            Ok(p) => p,
            Err(e) => return ToolResult::error(format!("Parse error: {}", e)),
        };

        let tool_name = parsed.name.clone();
        let param_count = parsed.params.len();
        let cmd_template = parsed.command_template.clone();

        {
            let mut protocol = self.protocol.lock().unwrap();
            match c_tool::save_c_tool(&mut *protocol, &parsed, code) {
                Ok((src, toml)) => {
                    info!("Registered custom tool '{}': src={} toml={}", tool_name, src, toml);
                }
                Err(e) => return ToolResult::error(format!("Save failed: {}", e)),
            }
        }

        // Refresh picks up the new TOML from custom_code/
        self.refresh();

        ToolResult::success(format!(
            "Tool '{}' registered and active.\nCommand template: {}\nParameters: {}\nCall it like any other MCP tool.",
            tool_name, cmd_template, param_count
        ))
    }
}
