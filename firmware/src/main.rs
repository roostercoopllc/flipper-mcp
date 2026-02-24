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

    // Step 4b: Wait for PING from FAP before sending any UART data.
    // The Flipper's expansion module is active at boot and will crash (BusFault)
    // if it receives our protocol messages. The FAP sends PING after it calls
    // expansion_disable() and sets up its UART, so we block here until then.
    info!("Waiting for PING from FAP (expansion_disable handshake)...");
    loop {
        match transport.read_line(1000) {
            Some(line) if line.starts_with("PING") => {
                info!("PING received — FAP is ready, starting protocol");
                break;
            }
            Some(line) => {
                info!("Pre-handshake UART: {} (ignoring)", line);
            }
            None => {}
        }
    }

    let fap = Arc::new(Mutex::new(FapProtocol::new(transport)));

    // Reply to the PING so FAP knows we're alive
    fap.lock().unwrap().push_pong();

    // Step 5: Load settings from NVS
    let mut settings = Settings::default();
    nvs_config.load_settings(&mut settings);

    // Step 6: If no SSID configured, wait for CONFIG message from FAP
    if settings.wifi_ssid.is_empty() {
        info!("No WiFi SSID in NVS — waiting for CONFIG from FAP");
        fap.lock().unwrap().push_status("status=needs_config");
    }
    while settings.wifi_ssid.is_empty() {
        for msg in fap.lock().unwrap().poll_messages() {
            match msg {
                FapMessage::Config(payload) => {
                    settings.merge_from_pipe_pairs(&payload);
                    // Always send ACK to acknowledge receipt, even if SSID is invalid
                    let mut ack_result = "err:no_ssid";
                    if !settings.wifi_ssid.is_empty() {
                        info!("Received WiFi config from FAP with valid SSID");
                        if let Err(e) = nvs_config.save_settings(&settings) {
                            error!("Failed to save config to NVS: {}", e);
                            ack_result = "err:nv_save";
                        } else {
                            ack_result = "ok";
                        }
                    } else {
                        warn!("Received CONFIG from FAP but SSID is empty");
                    }
                    fap.lock().unwrap().push_ack("config", ack_result);
                }
                FapMessage::Ping => {
                    fap.lock().unwrap().push_status("status=needs_config");
                }
                FapMessage::Cmd(cmd) => {
                    if cmd == "reboot" {
                        fap.lock().unwrap().push_ack("reboot", "ok");
                        thread::sleep(Duration::from_millis(100));
                        unsafe { esp_idf_svc::sys::esp_restart() }
                    }
                    fap.lock().unwrap().push_ack(&cmd, "err:no_wifi");
                }
            }
        }
        if settings.wifi_ssid.is_empty() {
            info!("Still waiting for WiFi config from FAP...");
            thread::sleep(Duration::from_secs(5));
            fap.lock().unwrap().push_status("status=needs_config");
        }
    }

    // Step 7: Connect WiFi — STA mode only, with retry loop
    // Enable verbose WiFi driver logging for handshake diagnostics
    unsafe {
        use std::ffi::CString;
        let tags = ["wifi", "wifi_init", "phy_init", "phy", "esp_netif_lwip"];
        for tag in &tags {
            let c_tag = CString::new(*tag).unwrap();
            esp_idf_svc::sys::esp_log_level_set(
                c_tag.as_ptr(),
                esp_idf_svc::sys::esp_log_level_t_ESP_LOG_DEBUG,
            );
        }
    }
    fap.lock().unwrap().push_status("status=connecting_wifi");
    fap.lock().unwrap().push_log(&format!(
        "WiFi: ssid='{}' pass_len={}",
        settings.wifi_ssid,
        settings.wifi_password.len()
    ));
    let mut wifi = wifi::create_wifi(peripherals.modem, sys_loop, nvs_partition, &settings)?;
    let mut wifi_attempt: u32 = 0;
    loop {
        wifi_attempt += 1;
        fap.lock().unwrap().push_log(&format!("WiFi attempt {}...", wifi_attempt));
        match wifi::start_and_connect(&mut wifi) {
            Ok(()) => break,
            Err(e) => {
                let err_full = format!("{:#}", e);
                error!("WiFi attempt {} failed: {}", wifi_attempt, err_full);

                // Push concise error to FAP — keep only the innermost error
                let err_short = if let Some(pos) = err_full.rfind(": ") {
                    &err_full[pos + 2..]
                } else {
                    &err_full
                };
                let err_display = if err_short.len() > 60 {
                    &err_short[..60]
                } else {
                    err_short
                };
                {
                    let f = fap.lock().unwrap();
                    f.push_log(&format!("#{} FAIL: {}", wifi_attempt, err_display));
                    f.push_status(&format!("status=wifi_error|error={}", err_display));
                }

                // Poll for FAP messages while waiting to retry
                for _ in 0..10 {
                    thread::sleep(Duration::from_secs(1));
                    for msg in fap.lock().unwrap().poll_messages() {
                        match msg {
                            FapMessage::Config(payload) => {
                                settings.merge_from_pipe_pairs(&payload);
                                let mut ack_result = "err:config_update";

                                if settings.wifi_ssid.is_empty() {
                                    warn!("CONFIG received but SSID is empty");
                                    ack_result = "err:no_ssid";
                                } else if let Err(e2) = nvs_config.save_settings(&settings) {
                                    warn!("NVS save: {}", e2);
                                    ack_result = "err:nv_save";
                                } else if let Err(e2) = wifi::reconfigure(&mut wifi, &settings) {
                                    warn!("WiFi reconfigure failed: {}", e2);
                                    ack_result = "err:wifi_reconfig";
                                } else {
                                    info!("CONFIG updated during WiFi retry, reconfigured successfully");
                                    ack_result = "ok";
                                }
                                fap.lock().unwrap().push_ack("config", ack_result);
                            }
                            FapMessage::Cmd(cmd) => {
                                info!("FAP command during WiFi retry: {}", cmd);
                                let f = fap.lock().unwrap();
                                if cmd == "reboot" {
                                    f.push_ack("reboot", "ok");
                                    drop(f);
                                    thread::sleep(Duration::from_millis(100));
                                    unsafe { esp_idf_svc::sys::esp_restart() }
                                } else if cmd == "status" {
                                    f.push_status(&format!(
                                        "status=wifi_error|error={}",
                                        err_short
                                    ));
                                    f.push_ack("status", "ok");
                                } else {
                                    f.push_ack(&cmd, "err:wifi_not_connected");
                                }
                            }
                            FapMessage::Ping => {
                                fap.lock().unwrap().push_pong();
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

    // Step 10: Create MCP server with shared FapProtocol, start HTTP.
    // The MCP server uses FapProtocol for CLI relay (execute_command sends
    // CLI| over UART, FAP executes via native SDK, returns CLI_OK/CLI_ERR).
    let protocol: Arc<Mutex<dyn FlipperProtocol>> = fap.clone();
    let mcp_server = Arc::new(mcp::McpServer::new(protocol, log_buf.clone()));

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
    {
        let f = fap.lock().unwrap();
        f.push_tools(&mcp_server.list_tool_names());
        for line in log_buf.snapshot() {
            f.push_log(&line);
        }
    }

    // Step 14: Main loop — poll UART for FAP messages
    info!("Firmware ready. MCP server listening on :8080");
    let mut poll_count: u32 = 0;
    loop {
        thread::sleep(POLL_INTERVAL);
        poll_count = poll_count.wrapping_add(1);

        let messages = fap.lock().unwrap().poll_messages();

        for msg in &messages {
            match msg {
                FapMessage::Cmd(cmd) => {
                    info!("FAP command: {}", cmd);
                    log_buf.push(&format!("FAP cmd: {}", cmd));

                    let ack_result =
                        handle_command(cmd, &mut manager, &mcp_server, &log_buf, &fap);

                    if cmd == "reboot" {
                        thread::sleep(Duration::from_millis(100));
                        unsafe { esp_idf_svc::sys::esp_restart() }
                    }

                    fap.lock().unwrap().push_ack(cmd, &ack_result);
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
                    fap.lock().unwrap().push_ack("config", &save_result);
                }
                FapMessage::Ping => {
                    fap.lock().unwrap().push_pong();
                }
            }
        }

        // After handling messages, or periodically, push status + logs
        let should_push = !messages.is_empty() || poll_count % STATUS_PUSH_EVERY == 0;
        if should_push {
            log_buf.push(&format!("tick #{} msgs={}", poll_count, messages.len()));
            push_full_status(
                &fap,
                &device_ip,
                &settings,
                &manager,
                relay_connected.load(Ordering::Relaxed),
            );
            let f = fap.lock().unwrap();
            for line in log_buf.snapshot() {
                f.push_log(&line);
            }
            drop(f);
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
    fap: &Arc<Mutex<FapProtocol>>,
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
            let f = fap.lock().unwrap();
            f.push_ack("reboot", "ok");
            for line in log_buf.snapshot() {
                f.push_log(&line);
            }
            // Caller will call esp_restart() after this returns
            "ok".to_string()
        }
        "refresh_modules" => {
            let names = mcp_server.refresh_and_list_tools();
            log_buf.push(&format!("Modules refreshed: {} tools", names.len()));
            fap.lock().unwrap().push_tools(&names);
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
    fap: &Arc<Mutex<FapProtocol>>,
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

    fap.lock().unwrap().push_status(&format!(
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
