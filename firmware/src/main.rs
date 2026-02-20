mod config;
mod mcp;
mod modules;
mod uart;
mod wifi;

use std::sync::{Arc, Mutex};
use std::thread;
use std::time::Duration;

use anyhow::Result;
use esp_idf_svc::eventloop::EspSystemEventLoop;
use esp_idf_svc::hal::peripherals::Peripherals;
use esp_idf_svc::log::EspLogger;
use esp_idf_svc::nvs::EspDefaultNvsPartition;
use log::{error, info, warn};

use config::NvsStorage;
use mcp::transport::HttpServerManager;
use uart::{CliProtocol, FlipperProtocol, UartTransport};

const SERVER_CMD_PATH: &str = "/ext/apps_data/flipper_mcp/server.cmd";
const POLL_INTERVAL: Duration = Duration::from_secs(5);

fn main() -> Result<()> {
    // Step 1: ESP-IDF patches and logging
    esp_idf_svc::sys::link_patches();
    EspLogger::initialize_default();

    info!("=== Flipper MCP Firmware v{} ===", env!("CARGO_PKG_VERSION"));

    // Step 2: Take hardware peripherals and system services
    let peripherals = Peripherals::take()?;
    let sys_loop = EspSystemEventLoop::take()?;
    let nvs_partition = EspDefaultNvsPartition::take()?;

    // Step 3: Load settings from NVS (defaults if empty)
    let nvs_storage = NvsStorage::new(nvs_partition.clone())?;
    let mut settings = nvs_storage.load_settings();

    // Step 4: Init UART transport (uses hardcoded pins — these are fixed by hardware)
    info!("Initializing UART at {} baud", settings.uart_baud_rate);
    let transport = UartTransport::new(
        peripherals.uart1,
        peripherals.pins.gpio1,
        peripherals.pins.gpio2,
        settings.uart_baud_rate,
    )?;
    let mut protocol = CliProtocol::new(transport);

    // Step 5: Try to load config from Flipper SD card (overrides NVS values)
    if let Err(e) = settings.load_from_sd(&mut protocol) {
        info!("SD card config load skipped: {}", e);
    }

    // Step 6: Connect WiFi STA
    let _wifi = wifi::connect_wifi(
        peripherals.modem,
        sys_loop,
        nvs_partition,
        &settings,
    )?;

    // Step 7: Smoke tests — verify UART communication with Flipper
    run_smoke_tests(&mut protocol);

    // Step 8: Create MCP server with module registry, start HTTP server
    let shared_protocol: Arc<Mutex<dyn FlipperProtocol>> = Arc::new(Mutex::new(protocol));
    let mcp_server = Arc::new(mcp::McpServer::new(shared_protocol.clone()));

    let mut manager = HttpServerManager::new(mcp_server);
    manager.start()?;

    // Step 9: Main loop — poll Flipper SD card for server control commands
    info!("Firmware ready. MCP server listening on :8080");
    loop {
        thread::sleep(POLL_INTERVAL);

        // Poll SD card for server control commands from the Flipper
        let cmd = {
            let mut proto = shared_protocol.lock().unwrap();
            read_server_command(&mut *proto)
        };

        if let Some(cmd) = cmd {
            info!("Server control command from Flipper: {}", cmd);
            match cmd.as_str() {
                "stop" => manager.stop(),
                "start" => {
                    if let Err(e) = manager.start() {
                        error!("Failed to start HTTP server: {}", e);
                    }
                }
                "restart" => {
                    if let Err(e) = manager.restart() {
                        error!("Failed to restart HTTP server: {}", e);
                    }
                }
                _ => warn!("Unknown server command: {}", cmd),
            }

            // Remove the command file after processing
            let mut proto = shared_protocol.lock().unwrap();
            let _ = proto.execute_command(&format!("storage remove {}", SERVER_CMD_PATH));
        }
    }
}

/// Read and validate a server control command from the Flipper's SD card.
/// Returns None if no valid command file exists.
fn read_server_command(protocol: &mut dyn FlipperProtocol) -> Option<String> {
    let response = protocol
        .execute_command(&format!("storage read {}", SERVER_CMD_PATH))
        .ok()?;

    let cmd = response.trim().to_string();

    // Flipper CLI returns error messages for missing files
    if cmd.is_empty()
        || cmd.contains("Storage error")
        || cmd.contains("Error")
        || cmd.contains("File not found")
    {
        return None;
    }

    Some(cmd)
}

fn run_smoke_tests(protocol: &mut dyn FlipperProtocol) {
    info!("--- Running UART smoke tests ---");

    match protocol.get_device_info() {
        Ok(info) => info!("Device info:\n{}", info),
        Err(e) => error!("device_info failed: {}", e),
    }

    match protocol.execute_command("power info") {
        Ok(info) => info!("Power info:\n{}", info),
        Err(e) => error!("power info failed: {}", e),
    }

    match protocol.execute_command("ps") {
        Ok(info) => info!("Process list:\n{}", info),
        Err(e) => error!("ps failed: {}", e),
    }

    info!("--- Smoke tests complete ---");
}
