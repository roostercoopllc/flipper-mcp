use serde_json::{json, Value};

use crate::mcp::types::{ToolDefinition, ToolResult};
use crate::modules::traits::FlipperModule;
use crate::uart::FlipperProtocol;

pub struct StorageModule;

impl FlipperModule for StorageModule {
    fn name(&self) -> &str {
        "storage"
    }

    fn description(&self) -> &str {
        "Flipper Zero SD card and internal storage operations"
    }

    fn tools(&self) -> Vec<ToolDefinition> {
        vec![
            ToolDefinition {
                name: "storage_list".to_string(),
                description: "List files and directories at the given path".to_string(),
                input_schema: json!({
                    "type": "object",
                    "properties": {
                        "path": { "type": "string", "description": "Directory path (e.g. '/ext', '/int', '/ext/subghz')" }
                    },
                    "required": ["path"]
                }),
            },
            ToolDefinition {
                name: "storage_read".to_string(),
                description: "Read the contents of a file from the Flipper storage".to_string(),
                input_schema: json!({
                    "type": "object",
                    "properties": {
                        "path": { "type": "string", "description": "File path (e.g. '/ext/subghz/captures/signal.sub')" }
                    },
                    "required": ["path"]
                }),
            },
            ToolDefinition {
                name: "storage_write".to_string(),
                description: "Write data to a file on the Flipper storage".to_string(),
                input_schema: json!({
                    "type": "object",
                    "properties": {
                        "path": { "type": "string", "description": "File path to write to" },
                        "data": { "type": "string", "description": "Content to write to the file" }
                    },
                    "required": ["path", "data"]
                }),
            },
            ToolDefinition {
                name: "storage_remove".to_string(),
                description: "Remove a file or directory from the Flipper storage".to_string(),
                input_schema: json!({
                    "type": "object",
                    "properties": {
                        "path": { "type": "string", "description": "Path of file or directory to remove" }
                    },
                    "required": ["path"]
                }),
            },
            ToolDefinition {
                name: "storage_stat".to_string(),
                description: "Get file/directory information (size, type)".to_string(),
                input_schema: json!({
                    "type": "object",
                    "properties": {
                        "path": { "type": "string", "description": "Path to stat" }
                    },
                    "required": ["path"]
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
            "storage_list" => match args.get("path").and_then(|v| v.as_str()) {
                Some(p) => format!("storage list {}", p),
                None => return ToolResult::error("Missing required parameter: path"),
            },
            "storage_read" => match args.get("path").and_then(|v| v.as_str()) {
                Some(p) => format!("storage read {}", p),
                None => return ToolResult::error("Missing required parameter: path"),
            },
            "storage_write" => {
                let path = args.get("path").and_then(|v| v.as_str());
                let data = args.get("data").and_then(|v| v.as_str());
                match (path, data) {
                    (Some(p), Some(d)) => format!("storage write {} {}", p, d),
                    _ => return ToolResult::error("Missing required parameters: path, data"),
                }
            }
            "storage_remove" => match args.get("path").and_then(|v| v.as_str()) {
                Some(p) => format!("storage remove {}", p),
                None => return ToolResult::error("Missing required parameter: path"),
            },
            "storage_stat" => match args.get("path").and_then(|v| v.as_str()) {
                Some(p) => format!("storage stat {}", p),
                None => return ToolResult::error("Missing required parameter: path"),
            },
            _ => return ToolResult::error(format!("Unknown storage tool: {}", tool)),
        };

        match protocol.execute_command(&command) {
            Ok(output) => ToolResult::success(output),
            Err(e) => ToolResult::error(format!("{} failed: {}", tool, e)),
        }
    }
}
