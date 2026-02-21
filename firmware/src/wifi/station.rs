use anyhow::{bail, ensure, Context, Result};
use esp_idf_svc::eventloop::EspSystemEventLoop;
use esp_idf_svc::hal::modem::Modem;
use esp_idf_svc::nvs::EspDefaultNvsPartition;
use esp_idf_svc::wifi::{BlockingWifi, ClientConfiguration, Configuration, EspWifi};
use log::info;

use crate::config::Settings;

/// Create the WiFi driver (consumes the modem peripheral) and apply initial config.
/// Does NOT start or connect — call `start_and_connect` for that.
pub fn create_wifi(
    modem: Modem,
    sys_loop: EspSystemEventLoop,
    nvs: EspDefaultNvsPartition,
    settings: &Settings,
) -> Result<BlockingWifi<EspWifi<'static>>> {
    if settings.wifi_ssid.is_empty() {
        bail!("WiFi SSID is empty — create config.txt on Flipper SD card");
    }

    let mut wifi = BlockingWifi::wrap(
        EspWifi::new(modem, sys_loop.clone(), Some(nvs))?,
        sys_loop,
    )?;

    apply_config(&mut wifi, settings)?;
    Ok(wifi)
}

/// Apply SSID/password configuration to the WiFi driver.
fn apply_config(wifi: &mut BlockingWifi<EspWifi<'static>>, settings: &Settings) -> Result<()> {
    ensure!(settings.wifi_ssid.len() <= 32, "SSID too long (max 32 bytes)");
    ensure!(settings.wifi_password.len() <= 64, "Password too long (max 64 bytes)");

    let config = Configuration::Client(ClientConfiguration {
        ssid: settings.wifi_ssid.as_str().try_into().unwrap(),
        password: settings.wifi_password.as_str().try_into().unwrap(),
        ..Default::default()
    });
    wifi.set_configuration(&config)?;
    Ok(())
}

/// Re-apply config after the user may have changed credentials.
pub fn reconfigure(wifi: &mut BlockingWifi<EspWifi<'static>>, settings: &Settings) -> Result<()> {
    // Stop before reconfiguring so the driver accepts new settings
    let _ = wifi.disconnect();
    let _ = wifi.stop();
    apply_config(wifi, settings)
}

/// Start the WiFi radio and connect to the configured network.
/// Returns Ok(()) on success; Err on failure (caller can retry).
pub fn start_and_connect(wifi: &mut BlockingWifi<EspWifi<'static>>) -> Result<()> {
    wifi.start().context("WiFi start failed")?;
    info!("WiFi started");

    wifi.connect().context("WiFi connect failed")?;
    info!("WiFi connected");

    wifi.wait_netif_up().context("Network interface failed to come up")?;

    let ip_info = wifi.wifi().sta_netif().get_ip_info()?;
    info!("WiFi connected — IP: {}", ip_info.ip);
    Ok(())
}
