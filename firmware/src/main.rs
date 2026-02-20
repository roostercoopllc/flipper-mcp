mod config;
mod uart;
mod wifi;

use std::thread;
use std::time::Duration;

use anyhow::Result;
use esp_idf_svc::eventloop::EspSystemEventLoop;
use esp_idf_svc::hal::peripherals::Peripherals;
use esp_idf_svc::log::EspLogger;
use esp_idf_svc::nvs::EspDefaultNvsPartition;
use log::{error, info};

use config::NvsStorage;
use uart::{CliProtocol, FlipperProtocol, UartTransport};

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

    // Step 8: Main loop — keep firmware alive
    info!("Firmware ready. Entering main loop.");
    loop {
        thread::sleep(Duration::from_secs(30));
        info!("heartbeat — firmware alive");
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
