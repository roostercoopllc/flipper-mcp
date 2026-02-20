use serde::Deserialize;
use serde_json::{json, Value};

use crate::mcp::types::ToolDefinition;
use crate::uart::FlipperProtocol;

use super::discovery::DynamicModule;
use super::traits::FlipperModule;

const MODULES_CONFIG_PATH: &str = "/ext/apps_data/flipper_mcp/modules.toml";

use super::c_tool::CUSTOM_CODE_DIR;

// ─── TOML schema ─────────────────────────────────────────────────────────────

#[derive(Deserialize)]
struct ModulesConfig {
    #[serde(default)]
    module: Vec<ModuleDef>,
}

#[derive(Deserialize)]
struct ModuleDef {
    name: String,
    description: String,
    #[serde(default)]
    tool: Vec<ToolDef>,
}

#[derive(Deserialize)]
struct ToolDef {
    name: String,
    description: String,
    command_template: String,
    #[serde(default)]
    params: Vec<ParamDef>,
    /// Optional UART timeout in ms for long-running commands (e.g. subghz rx, nfc detect).
    timeout_ms: Option<u32>,
}

#[derive(Deserialize)]
struct ParamDef {
    name: String,
    #[serde(rename = "type")]
    type_: String,
    #[serde(default)]
    required: bool,
    description: String,
}

// ─── Loader ───────────────────────────────────────────────────────────────────

/// Load config-driven modules from the TOML file on the Flipper SD card.
/// Returns an empty Vec (non-fatal) if the file doesn't exist or fails to parse.
pub fn load_config_modules(protocol: &mut dyn FlipperProtocol) -> Vec<Box<dyn FlipperModule>> {
    let raw = match read_config_file(protocol) {
        Some(text) => text,
        None => return Vec::new(),
    };

    let config: ModulesConfig = match toml::from_str(&raw) {
        Ok(c) => c,
        Err(e) => {
            log::warn!("Config modules: failed to parse {}: {}", MODULES_CONFIG_PATH, e);
            return Vec::new();
        }
    };

    let modules: Vec<Box<dyn FlipperModule>> = config
        .module
        .into_iter()
        .map(|m| Box::new(build_dynamic_module(m)) as Box<dyn FlipperModule>)
        .collect();

    log::info!(
        "Config modules: loaded {} module(s) with {} tool(s) total",
        modules.len(),
        modules.iter().map(|m| m.tools().len()).sum::<usize>()
    );
    modules
}

/// Load per-tool TOML files from the `custom_code/` directory on the Flipper SD card.
/// Each `*.toml` file was written by `c_tool::save_c_tool` and follows the same
/// `[[module]]` format as `modules.toml`, so `build_dynamic_module` handles it directly.
pub fn load_custom_code_modules(protocol: &mut dyn FlipperProtocol) -> Vec<Box<dyn FlipperModule>> {
    let list_output = match protocol
        .execute_command(&format!("storage list {}", CUSTOM_CODE_DIR))
        .ok()
    {
        Some(out) => out,
        None => return Vec::new(),
    };

    let trimmed = list_output.trim();
    if trimmed.is_empty()
        || trimmed.contains("Storage error")
        || trimmed.contains("File not found")
    {
        return Vec::new();
    }

    let mut modules: Vec<Box<dyn FlipperModule>> = Vec::new();

    for line in trimmed.lines() {
        let line = line.trim();
        // Flipper format: "[F] filename.toml"
        let filename = match line.strip_prefix("[F] ") {
            Some(n) => n.trim(),
            None => continue,
        };
        if !filename.ends_with(".toml") {
            continue;
        }

        let path = format!("{}/{}", CUSTOM_CODE_DIR, filename);
        let raw = match protocol.execute_command(&format!("storage read {}", path)) {
            Ok(out) => out,
            Err(_) => continue,
        };
        let raw = raw.trim();
        if raw.is_empty() || raw.contains("Storage error") || raw.contains("Error") {
            continue;
        }

        let config: ModulesConfig = match toml::from_str(raw) {
            Ok(c) => c,
            Err(e) => {
                log::warn!("Custom code: failed to parse {}: {}", path, e);
                continue;
            }
        };
        for m in config.module {
            modules.push(Box::new(build_dynamic_module(m)));
        }
    }

    log::info!("Custom code modules: loaded {} module(s)", modules.len());
    modules
}

fn read_config_file(protocol: &mut dyn FlipperProtocol) -> Option<String> {
    let response = protocol
        .execute_command(&format!("storage read {}", MODULES_CONFIG_PATH))
        .ok()?;

    let trimmed = response.trim();
    if trimmed.is_empty()
        || trimmed.contains("Storage error")
        || trimmed.contains("Error")
        || trimmed.contains("File not found")
    {
        log::info!("Config modules: {} not found, skipping", MODULES_CONFIG_PATH);
        return None;
    }

    Some(response)
}

fn build_dynamic_module(def: ModuleDef) -> DynamicModule {
    use super::discovery::DynamicTool;

    let tools = def
        .tool
        .into_iter()
        .map(|t| {
            let required_params: Vec<String> = t
                .params
                .iter()
                .filter(|p| p.required)
                .map(|p| p.name.clone())
                .collect();

            let input_schema = build_schema(&t.params);

            DynamicTool {
                definition: ToolDefinition {
                    name: t.name,
                    description: t.description,
                    input_schema,
                },
                command_template: t.command_template,
                required_params,
                timeout_ms: t.timeout_ms,
            }
        })
        .collect();

    DynamicModule {
        module_name: def.name,
        module_description: def.description,
        tools,
    }
}

fn build_schema(params: &[ParamDef]) -> Value {
    let mut properties = serde_json::Map::new();
    let mut required: Vec<Value> = Vec::new();

    for p in params {
        let json_type = match p.type_.as_str() {
            "integer" | "number" => "integer",
            "boolean" | "bool" => "boolean",
            _ => "string",
        };

        properties.insert(
            p.name.clone(),
            json!({
                "type": json_type,
                "description": p.description
            }),
        );

        if p.required {
            required.push(Value::String(p.name.clone()));
        }
    }

    json!({
        "type": "object",
        "properties": properties,
        "required": required
    })
}
