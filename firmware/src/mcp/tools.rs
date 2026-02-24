use std::sync::{Arc, Mutex};

use serde_json::Value;

use crate::modules::ModuleRegistry;
use crate::uart::FlipperProtocol;

use super::types::ToolResult;

pub struct ToolRegistry {
    modules: ModuleRegistry,
}

impl ToolRegistry {
    pub fn new(protocol: Arc<Mutex<dyn FlipperProtocol>>) -> Self {
        Self {
            modules: ModuleRegistry::new(protocol),
        }
    }

    pub fn call_tool(&self, name: &str, args: &Value) -> ToolResult {
        self.modules.call_tool(name, args)
    }

    pub fn refresh_dynamic(&self) {
        self.modules.refresh();
    }

    /// Return full tool definitions (for OpenAPI spec generation).
    pub fn list_tool_definitions(&self) -> Vec<super::types::ToolDefinition> {
        self.modules.list_all_tools()
    }

    /// Return all tool names, sorted alphabetically, for pushing to the FAP over UART.
    pub fn list_tool_names(&self) -> Vec<String> {
        let mut names: Vec<String> =
            self.modules.list_all_tools().into_iter().map(|t| t.name).collect();
        names.sort();
        names
    }
}
