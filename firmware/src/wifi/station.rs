use anyhow::{bail, Context, Result};
use esp_idf_svc::eventloop::EspSystemEventLoop;
use esp_idf_svc::hal::modem::Modem;
use esp_idf_svc::nvs::EspDefaultNvsPartition;
use esp_idf_svc::wifi::{BlockingWifi, ClientConfiguration, Configuration, EspWifi};
use log::info;

use crate::config::Settings;

pub fn connect_wifi(
    modem: Modem,
    sys_loop: EspSystemEventLoop,
    nvs: EspDefaultNvsPartition,
    settings: &Settings,
) -> Result<BlockingWifi<EspWifi<'static>>> {
    if settings.wifi_ssid.is_empty() {
        bail!("WiFi SSID is empty — configure via NVS or wifi-config.sh");
    }

    info!("Connecting to WiFi SSID: {:?}", settings.wifi_ssid);

    let mut wifi = BlockingWifi::wrap(
        EspWifi::new(modem, sys_loop.clone(), Some(nvs))?,
        sys_loop,
    )?;

    let config = Configuration::Client(ClientConfiguration {
        ssid: settings
            .wifi_ssid
            .as_str()
            .try_into()
            .context("SSID too long (max 32 bytes)")?,
        password: settings
            .wifi_password
            .as_str()
            .try_into()
            .context("Password too long (max 64 bytes)")?,
        ..Default::default()
    });

    wifi.set_configuration(&config)?;
    wifi.start().context("WiFi start failed")?;
    info!("WiFi started");

    wifi.connect().context("WiFi connect failed")?;
    info!("WiFi connected");

    wifi.wait_netif_up().context("Network interface failed to come up")?;

    let ip_info = wifi.wifi().sta_netif().get_ip_info()?;
    info!("WiFi connected — IP: {}", ip_info.ip);

    Ok(wifi)
}
