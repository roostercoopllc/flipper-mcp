use anyhow::Result;
use esp_idf_svc::eventloop::EspSystemEventLoop;
use esp_idf_svc::hal::modem::Modem;
use esp_idf_svc::nvs::EspDefaultNvsPartition;
use esp_idf_svc::wifi::{BlockingWifi, EspWifi};
use log::info;

use crate::config::Settings;

use super::{ap, station};

/// Outcome of the WiFi setup attempt.
pub enum WifiOutcome {
    /// STA mode — connected to an existing WiFi network. MCP server should start.
    Connected(BlockingWifi<EspWifi<'static>>),
    /// AP mode — hotspot is active, captive portal is serving.
    /// Device will restart automatically after credentials are saved.
    AccessPoint(BlockingWifi<EspWifi<'static>>),
}

/// Try STA first; fall back to AP if credentials are missing or connection fails.
pub fn connect_or_ap(
    modem: Modem,
    sys_loop: EspSystemEventLoop,
    nvs_partition: EspDefaultNvsPartition,
    settings: &Settings,
) -> Result<WifiOutcome> {
    if settings.wifi_ssid.is_empty() {
        info!("No WiFi SSID configured — starting AP mode for initial setup");
        let wifi = ap::start_access_point(modem, sys_loop, nvs_partition)?;
        return Ok(WifiOutcome::AccessPoint(wifi));
    }

    // Note: modem ownership is consumed by connect_wifi, so AP fallback is not
    // possible after a failed STA attempt. AP mode is only for the no-SSID case above.
    // If STA fails (wrong password, network down) the error propagates and the device
    // can be returned to AP mode by erasing NVS (idf.py erase-flash or wifi-config.sh).
    info!("Attempting STA connection to {:?}", settings.wifi_ssid);
    let wifi = station::connect_wifi(modem, sys_loop, nvs_partition, settings)?;
    info!("STA connected successfully");
    Ok(WifiOutcome::Connected(wifi))
}
