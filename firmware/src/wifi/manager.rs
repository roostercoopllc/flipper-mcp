use anyhow::Result;
use esp_idf_svc::eventloop::EspSystemEventLoop;
use esp_idf_svc::hal::modem::Modem;
use esp_idf_svc::nvs::EspDefaultNvsPartition;
use esp_idf_svc::wifi::{BlockingWifi, EspWifi};

use crate::config::Settings;

use super::station;

/// Create the WiFi driver without connecting. See `start_and_connect`.
pub fn create_wifi(
    modem: Modem,
    sys_loop: EspSystemEventLoop,
    nvs_partition: EspDefaultNvsPartition,
    settings: &Settings,
) -> Result<BlockingWifi<EspWifi<'static>>> {
    station::create_wifi(modem, sys_loop, nvs_partition, settings)
}

/// Start the radio and connect. Can be retried on failure.
pub fn start_and_connect(wifi: &mut BlockingWifi<EspWifi<'static>>) -> Result<()> {
    station::start_and_connect(wifi)
}

/// Re-apply credentials after config change. Call before retrying `start_and_connect`.
pub fn reconfigure(wifi: &mut BlockingWifi<EspWifi<'static>>, settings: &Settings) -> Result<()> {
    station::reconfigure(wifi, settings)
}
