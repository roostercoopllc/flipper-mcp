use anyhow::Result;
use esp_idf_svc::eventloop::EspSystemEventLoop;
use esp_idf_svc::hal::modem::Modem;
use esp_idf_svc::nvs::EspDefaultNvsPartition;
use esp_idf_svc::wifi::{BlockingWifi, EspWifi};

use crate::config::Settings;

use super::station;

/// Connect to an existing WiFi network in STA mode.
/// Returns the live wifi handle (must be kept alive to maintain the connection).
/// If `wifi_ssid` is empty in `settings`, returns an error — callers should
/// wait for the user to create config.txt via the Flipper FAP before calling this.
pub fn connect_wifi(
    modem: Modem,
    sys_loop: EspSystemEventLoop,
    nvs_partition: EspDefaultNvsPartition,
    settings: &Settings,
) -> Result<BlockingWifi<EspWifi<'static>>> {
    station::connect_wifi(modem, sys_loop, nvs_partition, settings)
}
