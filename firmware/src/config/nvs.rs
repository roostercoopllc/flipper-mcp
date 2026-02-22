use anyhow::{Context, Result};
use esp_idf_svc::nvs::{EspDefaultNvsPartition, EspNvs, NvsDefault};
use log::info;

use super::Settings;

const NVS_NAMESPACE: &str = "fmcp_cfg";

pub struct NvsConfig {
    nvs: EspNvs<NvsDefault>,
}

impl NvsConfig {
    pub fn new(partition: EspDefaultNvsPartition) -> Result<Self> {
        let nvs =
            EspNvs::new(partition, NVS_NAMESPACE, true).context("Failed to open NVS namespace")?;
        Ok(Self { nvs })
    }

    fn read_str(&self, key: &str) -> Option<String> {
        let mut buf = [0u8; 256];
        match self.nvs.get_str(key, &mut buf) {
            Ok(Some(s)) => {
                let s = s.trim_end_matches('\0').to_string();
                if s.is_empty() {
                    None
                } else {
                    Some(s)
                }
            }
            _ => None,
        }
    }

    fn write_str(&mut self, key: &str, value: &str) -> Result<()> {
        self.nvs
            .set_str(key, value)
            .context(format!("NVS write failed: {}", key))?;
        Ok(())
    }

    /// Populate settings from NVS. Only overwrites fields that have stored values.
    pub fn load_settings(&self, settings: &mut Settings) {
        if let Some(ssid) = self.read_str("wifi_ssid") {
            settings.wifi_ssid = ssid;
            info!("NVS: wifi_ssid loaded");
        }
        if let Some(pass) = self.read_str("wifi_pass") {
            settings.wifi_password = pass;
            info!("NVS: wifi_password loaded");
        }
        if let Some(name) = self.read_str("device_name") {
            settings.device_name = name;
            info!("NVS: device_name loaded");
        }
        if let Some(url) = self.read_str("relay_url") {
            settings.relay_url = url;
            info!("NVS: relay_url loaded");
        }
    }

    /// Persist current settings to NVS.
    pub fn save_settings(&mut self, settings: &Settings) -> Result<()> {
        self.write_str("wifi_ssid", &settings.wifi_ssid)?;
        self.write_str("wifi_pass", &settings.wifi_password)?;
        self.write_str("device_name", &settings.device_name)?;
        self.write_str("relay_url", &settings.relay_url)?;
        info!("NVS: settings saved");
        Ok(())
    }
}
