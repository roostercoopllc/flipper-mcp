use serde_json::{json, Value};

use crate::mcp::types::{ToolDefinition, ToolResult};
use crate::modules::traits::FlipperModule;
use crate::uart::FlipperProtocol;

pub struct InfraredModule;

impl FlipperModule for InfraredModule {
    fn name(&self) -> &str {
        "infrared"
    }

    fn description(&self) -> &str {
        "Infrared signal transmission (NEC, Samsung, RC5, RC6, SIRC, etc.)"
    }

    fn tools(&self) -> Vec<ToolDefinition> {
        vec![
            ToolDefinition {
                name: "ir_tx".to_string(),
                description: "Transmit an infrared signal with the specified protocol, address, and command. Supports NEC, NECext, NEC42, Samsung32, RC5, RC5X, RC6, SIRC, SIRC15, SIRC20, Kaseikyo, RCA, Pioneer."
                    .to_string(),
                input_schema: json!({
                    "type": "object",
                    "properties": {
                        "protocol": { "type": "string", "description": "IR protocol name (e.g. 'NEC', 'Samsung32', 'RC5', 'RC6', 'SIRC')" },
                        "address": { "type": "string", "description": "Device address as hex (e.g. '04' or '0x04')" },
                        "command": { "type": "string", "description": "Command code as hex (e.g. '08' or '0x08')" },
                        "repeat": { "type": "integer", "description": "Number of times to send (1-20, default 1)", "minimum": 1, "maximum": 20, "default": 1 }
                    },
                    "required": ["protocol", "address", "command"]
                }),
            },
            ToolDefinition {
                name: "ir_tx_raw".to_string(),
                description: "Transmit raw infrared timing data. Provide carrier frequency, duty cycle, and alternating mark/space durations in microseconds."
                    .to_string(),
                input_schema: json!({
                    "type": "object",
                    "properties": {
                        "frequency": { "type": "integer", "description": "Carrier frequency in Hz (typically 38000)", "default": 38000 },
                        "duty_cycle": { "type": "number", "description": "Duty cycle 0.0-1.0 (typically 0.33)", "default": 0.33 },
                        "timings": { "type": "string", "description": "Space-separated timing values in microseconds (alternating mark/space, e.g. '9000 4500 560 560 560 1690')" }
                    },
                    "required": ["timings"]
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
            "ir_tx" => {
                let ir_protocol = args.get("protocol").and_then(|v| v.as_str());
                let address = args.get("address").and_then(|v| v.as_str());
                let command_code = args.get("command").and_then(|v| v.as_str());
                let repeat = args.get("repeat").and_then(|v| v.as_i64()).unwrap_or(1);
                match (ir_protocol, address, command_code) {
                    (Some(p), Some(a), Some(c)) => {
                        format!("ir tx {} {} {} {}", p, a, c, repeat)
                    }
                    _ => {
                        return ToolResult::error(
                            "Missing required parameters: protocol, address, command",
                        )
                    }
                }
            }
            "ir_tx_raw" => {
                let freq = args
                    .get("frequency")
                    .and_then(|v| v.as_i64())
                    .unwrap_or(38000);
                let duty = args
                    .get("duty_cycle")
                    .and_then(|v| v.as_f64())
                    .unwrap_or(0.33);
                let timings = match args.get("timings").and_then(|v| v.as_str()) {
                    Some(t) => t,
                    None => return ToolResult::error("Missing required parameter: timings"),
                };
                format!("ir tx_raw {} {} {}", freq, duty, timings)
            }
            _ => return ToolResult::error(format!("Unknown infrared tool: {}", tool)),
        };

        match protocol.execute_command_with_timeout(&command, 5_000) {
            Ok(output) => ToolResult::success(output),
            Err(e) => ToolResult::error(format!("{} failed: {}", tool, e)),
        }
    }
}
