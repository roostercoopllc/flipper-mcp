mod config;
mod log_buffer;
mod mcp;
mod modules;
mod tunnel;
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

use config::Settings;
use log_buffer::LogBuffer;
use mcp::transport::HttpServerManager;
use uart::{CliProtocol, FlipperProtocol, UartTransport};

const SERVER_CMD_PATH: &str = "/ext/apps_data/flipper_mcp/server.cmd";
const STATUS_FILE_PATH: &str = "/ext/apps_data/flipper_mcp/status.txt";
const TOOLS_FILE_PATH: &str = "/ext/apps_data/flipper_mcp/tools.txt";
const POLL_INTERVAL: Duration = Duration::from_secs(5);
/// Write the status/log files every N poll cycles (N × POLL_INTERVAL = 30 s).
const STATUS_WRITE_EVERY: u32 = 6;

fn main() -> Result<()> {
    // Step 1: ESP-IDF patches and logging
    esp_idf_svc::sys::link_patches();
    EspLogger::initialize_default();

    info!("=== Flipper MCP Firmware v{} ===", env!("CARGO_PKG_VERSION"));

    // Step 2: Take hardware peripherals and system services
    let peripherals = Peripherals::take()?;
    let sys_loop = EspSystemEventLoop::take()?;
    // NVS partition is required by the ESP-IDF WiFi driver internally.
    // We no longer use it for our own config (config.txt on SD card is the sole store).
    let nvs_partition = EspDefaultNvsPartition::take()?;

    // Step 3: Init UART transport (uses hardcoded pins — these are fixed by hardware)
    let mut settings = Settings::default();
    info!("Initializing UART at {} baud", settings.uart_baud_rate);
    let transport = UartTransport::new(
        peripherals.uart1,
        peripherals.pins.gpio1,
        peripherals.pins.gpio2,
        settings.uart_baud_rate,
    )?;
    let mut protocol = CliProtocol::new(transport);

    // Step 4: Load config from Flipper SD card (sole persistent config store).
    // If config.txt is missing or has no SSID, wait in a patience loop until the
    // user creates it via the Flipper FAP "Configure WiFi" screen.
    if let Err(e) = settings.load_from_sd(&mut protocol) {
        info!("SD config initial load skipped: {}", e);
    }

    while settings.wifi_ssid.is_empty() {
        warn!("No WiFi SSID in config.txt — configure via Flipper FAP. Retrying in 30s.");
        write_needs_config_status(&mut protocol, &settings);
        thread::sleep(Duration::from_secs(30));
        if let Err(e) = settings.load_from_sd(&mut protocol) {
            info!("SD config retry: {}", e);
        }
    }

    // Step 5: Connect WiFi — STA mode only
    let wifi = wifi::connect_wifi(peripherals.modem, sys_loop, nvs_partition, &settings)?;

    // Step 6: Capture IP address for status reporting
    let device_ip = wifi
        .wifi()
        .sta_netif()
        .get_ip_info()
        .map(|i| i.ip.to_string())
        .unwrap_or_else(|_| "unknown".to_string());
    info!("Device IP: {}", device_ip);

    // Step 7: Smoke tests — verify UART communication with Flipper
    run_smoke_tests(&mut protocol);

    // Step 8: Create MCP server with module registry, start HTTP server
    let shared_protocol: Arc<Mutex<dyn FlipperProtocol>> = Arc::new(Mutex::new(protocol));
    let mcp_server = Arc::new(mcp::McpServer::new(shared_protocol.clone()));

    let mut manager = HttpServerManager::new(mcp_server.clone());
    manager.start()?;

    // Step 9: Advertise on local network via mDNS ({device_name}.local)
    let _mdns = tunnel::start_mdns_if_available(&settings.device_name);

    // Step 10: Start reverse WebSocket tunnel to relay (if relay_url is set)
    tunnel::start_tunnel_if_available(&settings.relay_url, mcp_server.clone());

    // Step 11: Init log buffer and write initial status + log to Flipper SD
    let log_buf = Arc::new(LogBuffer::new());
    log_buf.push(&format!(
        "Firmware v{} started. IP: {}",
        env!("CARGO_PKG_VERSION"),
        device_ip
    ));
    log_buf.push("MCP server listening on :8080");
    {
        let mut proto = shared_protocol.lock().unwrap();
        write_status_file(&mut *proto, &device_ip, &settings, &manager);
        write_tools_file(&mut *proto, &mcp_server);
        log_buf.flush_to_sd(&mut *proto);
    }

    // Step 12: Main loop — poll Flipper SD card for server control commands
    info!("Firmware ready. MCP server listening on :8080");
    let mut poll_count: u32 = 0;
    loop {
        thread::sleep(POLL_INTERVAL);
        poll_count = poll_count.wrapping_add(1);

        let cmd = {
            let mut proto = shared_protocol.lock().unwrap();
            read_server_command(&mut *proto)
        };

        if let Some(cmd) = cmd {
            info!("Server control command from Flipper: {}", cmd);
            log_buf.push(&format!("Server control: {}", cmd));

            match cmd.as_str() {
                "stop" => manager.stop(),
                "start" => {
                    if let Err(e) = manager.start() {
                        error!("Failed to start HTTP server: {}", e);
                        log_buf.push(&format!("ERROR start: {}", e));
                    }
                }
                "restart" => {
                    if let Err(e) = manager.restart() {
                        error!("Failed to restart HTTP server: {}", e);
                        log_buf.push(&format!("ERROR restart: {}", e));
                    }
                }
                "reboot" => {
                    info!("Reboot command received — restarting device");
                    log_buf.push("Rebooting (Flipper FAP command)");
                    {
                        let mut proto = shared_protocol.lock().unwrap();
                        log_buf.flush_to_sd(&mut *proto);
                    }
                    unsafe { esp_idf_svc::sys::esp_restart() };
                }
                "refresh_modules" => {
                    // Refresh FAP app discovery + config modules, then write tools.txt.
                    let names = mcp_server.refresh_and_list_tools();
                    log_buf.push(&format!("Modules refreshed: {} tools", names.len()));
                    let mut proto = shared_protocol.lock().unwrap();
                    write_tools_file_from_names(&mut *proto, &names);
                }
                "status" => {
                    // On-demand refresh: Flipper FAP writes this when user opens Status screen.
                    // The status file is written unconditionally below, so no extra action needed.
                }
                _ => warn!("Unknown server command: {}", cmd),
            }

            // Remove the command file after processing
            {
                let mut proto = shared_protocol.lock().unwrap();
                let _ = proto.execute_command(&format!("storage remove {}", SERVER_CMD_PATH));
            }

            // Immediately refresh status + log after any command
            let mut proto = shared_protocol.lock().unwrap();
            write_status_file(&mut *proto, &device_ip, &settings, &manager);
            log_buf.flush_to_sd(&mut *proto);
            poll_count = 0;
        } else if poll_count % STATUS_WRITE_EVERY == 0 {
            // Periodic status + log refresh (every 30 s)
            let mut proto = shared_protocol.lock().unwrap();
            write_status_file(&mut *proto, &device_ip, &settings, &manager);
            log_buf.flush_to_sd(&mut *proto);
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

/// Write a key=value status file to the Flipper SD card.
fn write_status_file(
    protocol: &mut dyn FlipperProtocol,
    ip: &str,
    settings: &config::Settings,
    manager: &HttpServerManager,
) {
    let server_state = if manager.is_running() { "running" } else { "stopped" };
    let content = format!(
        "ip={}\nssid={}\nserver={}\ndevice={}\nver={}\n",
        ip,
        settings.wifi_ssid,
        server_state,
        settings.device_name,
        env!("CARGO_PKG_VERSION"),
    );
    if let Err(e) = protocol.write_file(STATUS_FILE_PATH, &content) {
        warn!("Status file write failed (non-fatal): {}", e);
    }
}

/// Write a minimal status file indicating the device is waiting for WiFi configuration.
/// The Flipper FAP shows "needs_config" on the Status screen to prompt the user.
fn write_needs_config_status(protocol: &mut dyn FlipperProtocol, settings: &config::Settings) {
    let content = format!(
        "status=needs_config\ndevice={}\nver={}\n",
        settings.device_name,
        env!("CARGO_PKG_VERSION"),
    );
    if let Err(e) = protocol.write_file(STATUS_FILE_PATH, &content) {
        warn!("needs_config status write failed (non-fatal): {}", e);
    }
}

/// Write tools.txt by asking McpServer for its current tool names.
fn write_tools_file(protocol: &mut dyn FlipperProtocol, server: &mcp::McpServer) {
    write_tools_file_from_names(protocol, &server.list_tool_names());
}

/// Write tools.txt from a pre-computed name list.
fn write_tools_file_from_names(protocol: &mut dyn FlipperProtocol, names: &[String]) {
    let content = names.join("\n") + "\n";
    if let Err(e) = protocol.write_file(TOOLS_FILE_PATH, &content) {
        warn!("tools.txt write failed (non-fatal): {}", e);
    }
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
