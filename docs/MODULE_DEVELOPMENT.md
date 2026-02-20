# Module Development

## Overview

There are three ways to add tools to flipper-mcp:

1. **TOML config modules** — define tools in a file on the Flipper's SD card, refresh at runtime (no reflash)
2. **FAP discovery** — install a FAP app on the Flipper; it auto-discovers as a launchable tool
3. **Built-in Rust modules** — compile a new module into the firmware (requires reflash)

---

## Option 1: TOML Config Modules (recommended for custom tools)

Create `/ext/apps_data/flipper_mcp/modules.toml` on the Flipper's SD card.

See [../config/modules.example.toml](../config/modules.example.toml) for a full example.

### Minimal example

```toml
[[module]]
name = "subghz_tools"
description = "Custom SubGHz tools"

  [[module.tool]]
  name = "jam_433"
  description = "Transmit noise on 433.92 MHz"
  command_template = "subghz tx 433920000"
```

### Parametric tool

```toml
  [[module.tool]]
  name = "tx_frequency"
  description = "Transmit on a custom frequency"
  command_template = "subghz tx {frequency}"

    [[module.tool.params]]
    name = "frequency"
    type = "integer"
    required = true
    description = "Frequency in Hz (e.g. 433920000)"
```

### Parameter types

| Type | JSON Schema type | Notes |
|------|-----------------|-------|
| `string` | `"string"` | Default |
| `integer` | `"integer"` | Substituted as decimal |
| `number` | `"number"` | Floating point |
| `boolean` | `"boolean"` | Substituted as `true`/`false` |

### Reload without reflashing

```bash
curl -X POST http://flipper-mcp.local:8080/mcp \
  -H 'Content-Type: application/json' \
  -d '{"jsonrpc":"2.0","id":1,"method":"modules/refresh","params":{}}'
```

---

## Option 2: FAP App Auto-Discovery

Any `.fap` file in `/ext/apps/` or a subdirectory is automatically discovered as a launchable tool.

Tool name format: `app_launch_{filename_without_extension}` (lowercase, spaces and dashes replaced with underscores).

For example, `SubGhzFreqAnalyzer.fap` → tool `app_launch_subghzfreqanalyzer`.

The tool sends `loader open {filename}.fap` to the Flipper CLI.

To add new FAPs without reloading firmware:
1. Copy the `.fap` file to `/ext/apps/` on the Flipper SD card
2. Call `modules/refresh` or reboot the device

---

## Option 3: Built-in Rust Modules

For frequently used tools that need argument validation or output parsing, add a Rust module.

### Step 1: Create the module file

Create `firmware/src/modules/builtin/mymodule.rs`:

```rust
use serde_json::{json, Value};

use crate::mcp::types::{ToolDefinition, ToolResult};
use crate::uart::FlipperProtocol;

use super::super::traits::FlipperModule;

pub struct MyModule;

impl FlipperModule for MyModule {
    fn name(&self) -> &str { "my_module" }
    fn description(&self) -> &str { "My custom module" }

    fn tools(&self) -> Vec<ToolDefinition> {
        vec![ToolDefinition {
            name: "my_tool".to_string(),
            description: "Does something useful".to_string(),
            input_schema: json!({
                "type": "object",
                "properties": {
                    "value": {
                        "type": "string",
                        "description": "Input value"
                    }
                },
                "required": ["value"]
            }),
        }]
    }

    fn execute(
        &self,
        tool: &str,
        args: &Value,
        protocol: &mut dyn FlipperProtocol,
    ) -> ToolResult {
        match tool {
            "my_tool" => {
                let value = args["value"].as_str().unwrap_or("");
                match protocol.execute_command(&format!("myapp do {}", value)) {
                    Ok(output) => ToolResult::success(json!({ "output": output })),
                    Err(e) => ToolResult::error(format!("Command failed: {}", e)),
                }
            }
            _ => ToolResult::error(format!("Unknown tool: {}", tool)),
        }
    }
}
```

### Step 2: Register in `builtin/mod.rs`

```rust
mod mymodule;

pub fn register_all() -> Vec<Box<dyn FlipperModule>> {
    vec![
        // ... existing modules ...
        Box::new(mymodule::MyModule),
    ]
}
```

### Step 3: Build and flash

```bash
./scripts/flash.sh
```

---

## ToolResult Format

```rust
// Success with structured data
ToolResult::success(json!({
    "detected": true,
    "uid": "01:23:AB:CD",
    "type": "MIFARE Classic 1K"
}))

// Success with text output
ToolResult::success(json!({ "output": raw_cli_output }))

// Error
ToolResult::error("Tag not detected — ensure NFC tag is in range")
```

The `ToolResult` is serialized as the MCP `tools/call` response content.

---

## Command Template Substitution

Config module tools use `{param_name}` placeholders in `command_template`:

- `"subghz tx {frequency}"` + `{"frequency": 433920000}` → `"subghz tx 433920000"`
- `"storage read {path}"` + `{"path": "/ext/test.txt"}` → `"storage read /ext/test.txt"`
- Multiple params: `"subghz tx {freq} {protocol} {key}"` substitutes all named params

Missing required params cause a `INVALID_PARAMS` JSON-RPC error before the command is sent.
