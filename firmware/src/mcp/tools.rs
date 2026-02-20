use std::sync::{Arc, Mutex};

use serde_json::{json, Value};

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

    pub fn list_tools(&self) -> Value {
        json!({ "tools": self.modules.list_all_tools() })
    }

    pub fn call_tool(&self, name: &str, args: &Value) -> ToolResult {
        self.modules.call_tool(name, args)
    }

    pub fn refresh_dynamic(&self) {
        self.modules.refresh();
    }

    /// Return all tool names, sorted alphabetically, for writing to tools.txt on the Flipper SD.
    pub fn list_tool_names(&self) -> Vec<String> {
        let mut names: Vec<String> =
            self.modules.list_all_tools().into_iter().map(|t| t.name).collect();
        names.sort();
        names
    }
}
