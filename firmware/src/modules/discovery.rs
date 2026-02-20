use serde_json::{json, Value};

use crate::mcp::types::{ToolDefinition, ToolResult};
use crate::uart::FlipperProtocol;

use super::traits::FlipperModule;

// ─── Shared dynamic module types (also used by config.rs) ────────────────────

pub(super) struct DynamicTool {
    pub definition: ToolDefinition,
    /// Command template with optional {param} placeholders.
    /// For FAP launchers this is fully static (e.g., "loader open BadApple.fap").
    /// For config tools it may contain substitutions (e.g., "subghz rx {frequency}").
    pub command_template: String,
    pub required_params: Vec<String>,
    /// Optional UART read timeout override in milliseconds.
    /// Useful for long-running commands (subghz rx, nfc detect, ir rx).
    /// Falls back to the default 2 s when None.
    pub timeout_ms: Option<u32>,
}

pub(super) struct DynamicModule {
    pub module_name: String,
    #[allow(dead_code)]
    pub module_description: String,
    pub tools: Vec<DynamicTool>,
}

impl FlipperModule for DynamicModule {
    fn name(&self) -> &str {
        &self.module_name
    }

    fn description(&self) -> &str {
        &self.module_description
    }

    fn tools(&self) -> Vec<ToolDefinition> {
        self.tools.iter().map(|t| t.definition.clone()).collect()
    }

    fn execute(
        &self,
        tool: &str,
        args: &Value,
        protocol: &mut dyn FlipperProtocol,
    ) -> ToolResult {
        let dt = match self.tools.iter().find(|t| t.definition.name == tool) {
            Some(t) => t,
            None => return ToolResult::error(format!("Unknown tool in module {}: {}", self.module_name, tool)),
        };

        match substitute_params(&dt.command_template, args, &dt.required_params) {
            Ok(cmd) => {
                let result = match dt.timeout_ms {
                    Some(t) => protocol.execute_command_with_timeout(&cmd, t),
                    None => protocol.execute_command(&cmd),
                };
                match result {
                    Ok(output) => ToolResult::success(output),
                    Err(e) => ToolResult::error(format!("{} failed: {}", tool, e)),
                }
            }
            Err(msg) => ToolResult::error(msg),
        }
    }
}

fn substitute_params(template: &str, args: &Value, required: &[String]) -> Result<String, String> {
    for param in required {
        if args.get(param).is_none() {
            return Err(format!("Missing required parameter: {}", param));
        }
    }

    let mut result = template.to_string();
    if let Some(obj) = args.as_object() {
        for (k, v) in obj {
            let placeholder = format!("{{{}}}", k);
            let value = match v {
                Value::String(s) => s.clone(),
                Value::Number(n) => n.to_string(),
                Value::Bool(b) => b.to_string(),
                other => other.to_string(),
            };
            result = result.replace(&placeholder, &value);
        }
    }
    Ok(result)
}

// ─── FAP Discovery ────────────────────────────────────────────────────────────

/// Scan `/ext/apps` on the Flipper SD card for FAP apps (2 levels deep).
/// Returns one DynamicModule per FAP file, each with a single `app_launch_*` tool.
pub fn scan_fap_apps(protocol: &mut dyn FlipperProtocol) -> Vec<Box<dyn FlipperModule>> {
    let mut modules: Vec<Box<dyn FlipperModule>> = Vec::new();

    // First level: list directories under /ext/apps
    let top_entries = match protocol.execute_command("storage list /ext/apps") {
        Ok(output) => parse_storage_list(&output),
        Err(e) => {
            log::warn!("FAP discovery: could not list /ext/apps: {}", e);
            return modules;
        }
    };

    // Also check .fap files at the top level
    for (is_dir, name) in &top_entries {
        if !is_dir && name.ends_with(".fap") {
            if let Some(m) = make_fap_module(name) {
                modules.push(Box::new(m));
            }
        }
    }

    // Second level: list each directory
    for (is_dir, dir_name) in &top_entries {
        if !is_dir {
            continue;
        }
        let path = format!("/ext/apps/{}", dir_name);
        let entries = match protocol.execute_command(&format!("storage list {}", path)) {
            Ok(output) => parse_storage_list(&output),
            Err(_) => continue,
        };
        for (is_file_dir, filename) in entries {
            if !is_file_dir && filename.ends_with(".fap") {
                if let Some(m) = make_fap_module(&filename) {
                    modules.push(Box::new(m));
                }
            }
        }
    }

    log::info!("FAP discovery: found {} app(s)", modules.len());
    modules
}

fn make_fap_module(filename: &str) -> Option<DynamicModule> {
    let tool_name = tool_name_from_fap(filename);
    let description = format!(
        "Launch the {} FAP application",
        filename.trim_end_matches(".fap")
    );
    let command = format!("loader open {}", filename);

    Some(DynamicModule {
        module_name: tool_name.clone(),
        module_description: description.clone(),
        tools: vec![DynamicTool {
            definition: ToolDefinition {
                name: tool_name,
                description,
                input_schema: json!({ "type": "object", "properties": {}, "required": [] }),
            },
            command_template: command,
            required_params: vec![],
            timeout_ms: None,
        }],
    })
}

/// Parse `storage list` output into `(is_directory, name)` pairs.
/// Flipper format: "[D] DirectoryName" or "[F] filename.ext"
fn parse_storage_list(output: &str) -> Vec<(bool, String)> {
    let mut entries = Vec::new();
    for line in output.lines() {
        let line = line.trim();
        if let Some(name) = line.strip_prefix("[D] ") {
            let name = name.trim().to_string();
            if !name.is_empty() {
                entries.push((true, name));
            }
        } else if let Some(name) = line.strip_prefix("[F] ") {
            let name = name.trim().to_string();
            if !name.is_empty() {
                entries.push((false, name));
            }
        }
    }
    entries
}

fn tool_name_from_fap(filename: &str) -> String {
    let stem = filename.trim_end_matches(".fap");
    let sanitized = stem
        .to_lowercase()
        .chars()
        .map(|c| if c.is_alphanumeric() || c == '_' { c } else { '_' })
        .collect::<String>();
    format!("app_launch_{}", sanitized)
}
