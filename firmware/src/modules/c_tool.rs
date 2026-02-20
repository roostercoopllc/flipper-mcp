use crate::uart::FlipperProtocol;

pub(super) const CUSTOM_CODE_DIR: &str = "/ext/apps_data/flipper_mcp/custom_code";

// ─── Parsed types ─────────────────────────────────────────────────────────────

pub struct ParsedCTool {
    pub name: String,
    pub description: String,
    pub command_template: String,
    pub params: Vec<ParsedParam>,
    /// Optional UART read timeout override in ms. Parse with `// timeout: 8000`.
    pub timeout_ms: Option<u32>,
}

pub struct ParsedParam {
    pub name: String,
    pub type_: String,
    pub required: bool,
    pub description: String,
}

// ─── Parser ───────────────────────────────────────────────────────────────────

/// Parse a pseudo-C function string into a tool definition.
///
/// Expected format:
/// ```c
/// // description: What the tool does
/// void tool_name(string param1, integer param2) {
///     // exec: cli command {param1} {param2}
///     // optional: param2
/// }
/// ```
///
/// Rules:
/// - `// description:` sets the tool description (falls back to "Custom tool: <name>")
/// - Function signature extracts tool name and parameters
/// - `// exec:` (first match) sets the CLI command template; use `{param}` placeholders
/// - `// optional:` marks a parameter as non-required (all params required by default)
/// - Return type is ignored (use `void` or any other type)
pub fn parse_c_tool(code: &str) -> Result<ParsedCTool, String> {
    let mut description = String::new();
    let mut exec_template = String::new();
    let mut optional_params: Vec<String> = Vec::new();
    let mut func_name = String::new();
    let mut raw_params: Vec<(String, String)> = Vec::new(); // (type, name)
    let mut timeout_ms: Option<u32> = None;

    for line in code.lines() {
        let trimmed = line.trim();

        if let Some(rest) = trimmed.strip_prefix("// description:") {
            description = rest.trim().to_string();
        } else if let Some(rest) = trimmed.strip_prefix("// exec:") {
            if exec_template.is_empty() {
                exec_template = rest.trim().to_string();
            }
        } else if let Some(rest) = trimmed.strip_prefix("// optional:") {
            optional_params.push(rest.trim().to_string());
        } else if let Some(rest) = trimmed.strip_prefix("// timeout:") {
            if let Ok(ms) = rest.trim().parse::<u32>() {
                timeout_ms = Some(ms);
            }
        } else if !trimmed.starts_with("//")
            && !trimmed.is_empty()
            && trimmed != "{"
            && trimmed != "}"
            && func_name.is_empty()
            && trimmed.contains('(')
        {
            if let Some((name, params)) = parse_signature(trimmed) {
                func_name = name;
                raw_params = params;
            }
        }
    }

    if func_name.is_empty() {
        return Err(
            "No function signature found. Expected: void tool_name(type param, ...)".to_string(),
        );
    }
    if exec_template.is_empty() {
        return Err("No '// exec: <command>' line found in the function body".to_string());
    }
    if description.is_empty() {
        description = format!("Custom tool: {}", func_name);
    }

    let params = raw_params
        .into_iter()
        .map(|(type_, name)| {
            let required = !optional_params.contains(&name);
            ParsedParam { name, type_, required, description: String::new() }
        })
        .collect();

    Ok(ParsedCTool { name: func_name, description, command_template: exec_template, params, timeout_ms })
}

/// Parse a C-style function signature line.
/// Returns `(function_name, [(param_type, param_name)])`.
fn parse_signature(line: &str) -> Option<(String, Vec<(String, String)>)> {
    let open = line.find('(')?;
    let close = line.rfind(')')?;
    if close < open {
        return None;
    }

    // Function name: last whitespace-delimited token before '('
    let func_name: String = line[..open]
        .trim()
        .split_whitespace()
        .last()?
        .chars()
        .filter(|c| c.is_alphanumeric() || *c == '_')
        .collect();
    if func_name.is_empty() {
        return None;
    }

    let params_str = &line[open + 1..close];
    let mut params = Vec::new();

    for part in params_str.split(',') {
        let part = part.trim();
        if part.is_empty() {
            continue;
        }
        let tokens: Vec<&str> = part.split_whitespace().collect();
        match tokens.len() {
            0 => {}
            1 => {
                // Name only — default to "string"
                let name: String =
                    tokens[0].chars().filter(|c| c.is_alphanumeric() || *c == '_').collect();
                if !name.is_empty() {
                    params.push(("string".to_string(), name));
                }
            }
            _ => {
                let type_ = normalize_type(tokens[0]);
                let name: String =
                    tokens[1].chars().filter(|c| c.is_alphanumeric() || *c == '_').collect();
                if !name.is_empty() {
                    params.push((type_, name));
                }
            }
        }
    }

    Some((func_name, params))
}

fn normalize_type(t: &str) -> String {
    match t {
        "int" | "integer" | "long" | "short" | "uint8_t" | "uint16_t" | "uint32_t"
        | "uint64_t" | "int8_t" | "int16_t" | "int32_t" | "int64_t" => "integer".to_string(),
        "bool" | "boolean" => "boolean".to_string(),
        _ => "string".to_string(),
    }
}

// ─── Serializer ───────────────────────────────────────────────────────────────

/// Serialize a parsed tool to TOML text (compatible with the `config.rs` loader).
pub fn to_module_toml(tool: &ParsedCTool) -> String {
    let timeout_line = match tool.timeout_ms {
        Some(ms) => format!("timeout_ms = {}\n", ms),
        None => String::new(),
    };

    let mut out = format!(
        "[[module]]\nname = \"custom_{name}\"\ndescription = \"Custom: {desc}\"\n\n\
         [[module.tool]]\nname = \"{name}\"\ndescription = \"{desc}\"\n\
         command_template = \"{cmd}\"\n{timeout}",
        name = tool.name,
        desc = escape_toml(&tool.description),
        cmd = escape_toml(&tool.command_template),
        timeout = timeout_line,
    );

    for param in &tool.params {
        out.push_str(&format!(
            "\n[[module.tool.params]]\nname = \"{}\"\ntype = \"{}\"\nrequired = {}\n\
             description = \"{}\"\n",
            param.name,
            param.type_,
            param.required,
            escape_toml(&param.description),
        ));
    }

    out
}

fn escape_toml(s: &str) -> String {
    s.replace('\\', "\\\\").replace('"', "\\\"")
}

// ─── SD card persistence ──────────────────────────────────────────────────────

/// Write the source code and generated TOML descriptor to the Flipper SD card.
///
/// Paths:
/// - `custom_code/{name}.c`   — original source (for reference)
/// - `custom_code/{name}.toml` — TOML descriptor (loaded by `config::load_custom_code_modules`)
///
/// The parent directory is created automatically by `write_file`.
/// Returns `(source_path, toml_path)` on success.
pub fn save_c_tool(
    protocol: &mut dyn FlipperProtocol,
    tool: &ParsedCTool,
    source_code: &str,
) -> Result<(String, String), String> {
    let src_path = format!("{}/{}.c", CUSTOM_CODE_DIR, tool.name);
    let toml_path = format!("{}/{}.toml", CUSTOM_CODE_DIR, tool.name);

    protocol
        .write_file(&src_path, source_code)
        .map_err(|e| format!("Failed to write source file: {}", e))?;

    let toml = to_module_toml(tool);
    protocol
        .write_file(&toml_path, &toml)
        .map_err(|e| format!("Failed to write TOML descriptor: {}", e))?;

    Ok((src_path, toml_path))
}
