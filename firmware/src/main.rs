mod config;
mod log_buffer;
mod mcp;
mod modules;
mod tunnel;
mod uart;
mod wifi;

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::Duration;

use anyhow::Result;
use esp_idf_svc::eventloop::EspSystemEventLoop;
use esp_idf_svc::hal::peripherals::Peripherals;
use esp_idf_svc::log::EspLogger;
use esp_idf_svc::nvs::EspDefaultNvsPartition;
use log::{error, info, warn};

use config::{NvsConfig, Settings};
use log_buffer::LogBuffer;
use mcp::transport::HttpServerManager;
use uart::{FapMessage, FapProtocol, FlipperProtocol, UartTransport};

const POLL_INTERVAL: Duration = Duration::from_secs(5);
/// Push STATUS + LOG every N poll cycles (N × POLL_INTERVAL = 30 s).
const STATUS_PUSH_EVERY: u32 = 6;

/// Stub FlipperProtocol for Phase 1 — MCP tools that need Flipper CLI
/// will get clear error messages until Phase 2 adds CLI relay via FAP.
struct StubProtocol;

impl FlipperProtocol for StubProtocol {
    fn execute_command(&mut self, _command: &str) -> Result<String> {
        anyhow::bail!("Flipper CLI not available — FAP bridge mode (Phase 2)")
    }

    fn write_file(&mut self, _path: &str, _content: &str) -> Result<()> {
        anyhow::bail!("Flipper CLI not available — FAP bridge mode (Phase 2)")
    }
}

fn main() -> Result<()> {
    // Step 1: ESP-IDF patches and logging
    esp_idf_svc::sys::link_patches();
    EspLogger::initialize_default();

    info!("=== Flipper MCP Firmware v{} ===", env!("CARGO_PKG_VERSION"));

    // Step 2: Take hardware peripherals and system services
    let peripherals = Peripherals::take()?;
    let sys_loop = EspSystemEventLoop::take()?;
    // NVS partition — clone before passing to WiFi driver (both need a handle).
    let nvs_partition = EspDefaultNvsPartition::take()?;

    // Step 3: Init NVS config store (uses a clone of the NVS partition)
    let mut nvs_config = NvsConfig::new(nvs_partition.clone())?;

    // Step 4: Init UART transport + FapProtocol
    let settings_default = Settings::default();
    info!("Initializing UART at {} baud", settings_default.uart_baud_rate);
    let transport = UartTransport::new(
        peripherals.uart0,
        peripherals.pins.gpio43,
        peripherals.pins.gpio44,
        settings_default.uart_baud_rate,
    )?;
    let fap = FapProtocol::new(transport);

    // Step 5: Load settings from NVS
    let mut settings = Settings::default();
    nvs_config.load_settings(&mut settings);

    // Step 6: If no SSID configured, wait for CONFIG message from FAP
    if settings.wifi_ssid.is_empty() {
        info!("No WiFi SSID in NVS — waiting for CONFIG from FAP");
        fap.push_status("status=needs_config");
    }
    while settings.wifi_ssid.is_empty() {
        for msg in fap.poll_messages() {
            match msg {
                FapMessage::Config(payload) => {
                    settings.merge_from_pipe_pairs(&payload);
                    if !settings.wifi_ssid.is_empty() {
                        info!("Received WiFi config from FAP");
                        if let Err(e) = nvs_config.save_settings(&settings) {
                            error!("Failed to save config to NVS: {}", e);
                        }
                        fap.push_ack("config", "ok");
                    }
                }
                FapMessage::Ping => {
                    fap.push_status("status=needs_config");
                }
                FapMessage::Cmd(cmd) => {
                    if cmd == "reboot" {
                        fap.push_ack("reboot", "ok");
                        thread::sleep(Duration::from_millis(100));
                        unsafe { esp_idf_svc::sys::esp_restart() }
                    }
                    fap.push_ack(&cmd, "err:no_wifi");
                }
            }
        }
        if settings.wifi_ssid.is_empty() {
            thread::sleep(Duration::from_secs(5));
            fap.push_status("status=needs_config");
        }
    }

    // Step 7: Connect WiFi — STA mode only, with retry loop
    fap.push_status("status=connecting_wifi");
    let mut wifi = wifi::create_wifi(peripherals.modem, sys_loop, nvs_partition, &settings)?;
    loop {
        match wifi::start_and_connect(&mut wifi) {
            Ok(()) => break,
            Err(e) => {
                error!("WiFi connect failed: {}. Retrying in 10s.", e);
                let err_msg = e.to_string();
                let err_short = if err_msg.len() > 80 {
                    &err_msg[..80]
                } else {
                    &err_msg
                };
                fap.push_status(&format!("status=wifi_error|error={}", err_short));

                // Poll for FAP messages while waiting to retry
                for _ in 0..10 {
                    thread::sleep(Duration::from_secs(1));
                    for msg in fap.poll_messages() {
                        match msg {
                            FapMessage::Config(payload) => {
                                settings.merge_from_pipe_pairs(&payload);
                                if let Err(e2) = nvs_config.save_settings(&settings) {
                                    warn!("NVS save: {}", e2);
                                }
                                wifi::reconfigure(&mut wifi, &settings)?;
                                fap.push_ack("config", "ok");
                            }
                            FapMessage::Cmd(cmd) => {
                                info!("FAP command during WiFi retry: {}", cmd);
                                if cmd == "reboot" {
                                    fap.push_ack("reboot", "ok");
                                    thread::sleep(Duration::from_millis(100));
                                    unsafe { esp_idf_svc::sys::esp_restart() }
                                } else if cmd == "status" {
                                    fap.push_status(&format!(
                                        "status=wifi_error|error={}",
                                        err_short
                                    ));
                                    fap.push_ack("status", "ok");
                                } else {
                                    fap.push_ack(&cmd, "err:wifi_not_connected");
                                }
                            }
                            FapMessage::Ping => {
                                fap.push_pong();
                            }
                        }
                    }
                }
            }
        }
    }

    // Step 8: Capture IP address
    let device_ip = wifi
        .wifi()
        .sta_netif()
        .get_ip_info()
        .map(|i| i.ip.to_string())
        .unwrap_or_else(|_| "unknown".to_string());
    info!("Device IP: {}", device_ip);

    // Step 9: Init log buffer
    let log_buf = Arc::new(LogBuffer::new());

    // Step 10: Create MCP server with stub protocol, start HTTP.
    // FapProtocol is used directly by the main loop for push/poll;
    // MCP server gets a stub since CLI relay is Phase 2.
    let stub: Arc<Mutex<dyn FlipperProtocol>> = Arc::new(Mutex::new(StubProtocol));
    let mcp_server = Arc::new(mcp::McpServer::new(stub, log_buf.clone()));

    let mut manager = HttpServerManager::new(mcp_server.clone());
    manager.start()?;

    // Step 11: mDNS advertisement
    let _mdns = tunnel::start_mdns_if_available(&settings.device_name);

    // Step 12: Reverse WebSocket tunnel (if relay_url configured)
    let relay_connected = Arc::new(AtomicBool::new(false));
    tunnel::start_tunnel_if_available(
        &settings.relay_url,
        mcp_server.clone(),
        relay_connected.clone(),
    );

    // Step 13: Push initial status + tools + log over UART
    log_buf.push(&format!(
        "Firmware v{} started. IP: {}",
        env!("CARGO_PKG_VERSION"),
        device_ip
    ));
    log_buf.push("MCP server listening on :8080");
    push_full_status(&fap, &device_ip, &settings, &manager, false);
    fap.push_tools(&mcp_server.list_tool_names());
    for line in log_buf.snapshot() {
        fap.push_log(&line);
    }

    // Step 14: Main loop — poll UART for FAP messages
    info!("Firmware ready. MCP server listening on :8080");
    let mut poll_count: u32 = 0;
    loop {
        thread::sleep(POLL_INTERVAL);
        poll_count = poll_count.wrapping_add(1);

        let messages = fap.poll_messages();

        for msg in &messages {
            match msg {
                FapMessage::Cmd(cmd) => {
                    info!("FAP command: {}", cmd);
                    log_buf.push(&format!("FAP cmd: {}", cmd));

                    let ack_result =
                        handle_command(cmd, &mut manager, &mcp_server, &log_buf, &fap);

                    if cmd == "reboot" {
                        // handle_command already sent ACK and flushed logs;
                        // small delay ensures UART TX buffer is fully transmitted
                        // before the chip resets.
                        thread::sleep(Duration::from_millis(100));
                        unsafe { esp_idf_svc::sys::esp_restart() }
                    }

                    fap.push_ack(cmd, &ack_result);
                }
                FapMessage::Config(payload) => {
                    info!("FAP config update");
                    settings.merge_from_pipe_pairs(payload);
                    let save_result = match nvs_config.save_settings(&settings) {
                        Ok(()) => "ok".to_string(),
                        Err(e) => {
                            error!("NVS save failed: {}", e);
                            format!("err:{}", e)
                        }
                    };
                    log_buf.push(&format!("Config updated: {}", save_result));
                    fap.push_ack("config", &save_result);
                }
                FapMessage::Ping => {
                    fap.push_pong();
                }
            }
        }

        // After handling messages, or periodically, push status + logs
        let should_push = !messages.is_empty() || poll_count % STATUS_PUSH_EVERY == 0;
        if should_push {
            push_full_status(
                &fap,
                &device_ip,
                &settings,
                &manager,
                relay_connected.load(Ordering::Relaxed),
            );
            for line in log_buf.snapshot() {
                fap.push_log(&line);
            }
            poll_count = 0;
        }
    }
}

/// Handle a server command from the FAP. Returns the ACK result string.
fn handle_command(
    cmd: &str,
    manager: &mut HttpServerManager,
    mcp_server: &Arc<mcp::McpServer>,
    log_buf: &Arc<LogBuffer>,
    fap: &FapProtocol,
) -> String {
    match cmd {
        "stop" => {
            manager.stop();
            "ok".to_string()
        }
        "start" => match manager.start() {
            Ok(()) => "ok".to_string(),
            Err(e) => {
                error!("Failed to start HTTP server: {}", e);
                log_buf.push(&format!("ERROR start: {}", e));
                format!("err:{}", e)
            }
        },
        "restart" => match manager.restart() {
            Ok(()) => "ok".to_string(),
            Err(e) => {
                error!("Failed to restart HTTP server: {}", e);
                log_buf.push(&format!("ERROR restart: {}", e));
                format!("err:{}", e)
            }
        },
        "reboot" => {
            info!("Reboot command received — restarting device");
            log_buf.push("Rebooting (FAP command)");
            fap.push_ack("reboot", "ok");
            for line in log_buf.snapshot() {
                fap.push_log(&line);
            }
            // Caller will call esp_restart() after this returns
            "ok".to_string()
        }
        "refresh_modules" => {
            let names = mcp_server.refresh_and_list_tools();
            log_buf.push(&format!("Modules refreshed: {} tools", names.len()));
            fap.push_tools(&names);
            "ok".to_string()
        }
        "status" => "ok".to_string(),
        _ => {
            warn!("Unknown FAP command: {}", cmd);
            format!("err:unknown:{}", cmd)
        }
    }
}

/// Push a full STATUS message with all fields.
fn push_full_status(
    fap: &FapProtocol,
    ip: &str,
    settings: &Settings,
    manager: &HttpServerManager,
    relay_connected: bool,
) {
    let server_state = if manager.is_running() {
        "running"
    } else {
        "stopped"
    };
    let relay_state = if relay_connected {
        "connected"
    } else if !settings.relay_url.is_empty() {
        "configured"
    } else {
        "disabled"
    };
    // SAFETY: esp_get_free_heap_size is a trivial C wrapper with no preconditions
    let heap_kb = unsafe { esp_idf_svc::sys::esp_get_free_heap_size() } / 1024;

    fap.push_status(&format!(
        "ip={}|ssid={}|server={}|device={}|ver={}|relay={}|heap_free={}KB",
        ip,
        settings.wifi_ssid,
        server_state,
        settings.device_name,
        env!("CARGO_PKG_VERSION"),
        relay_state,
        heap_kb,
    ));
}
