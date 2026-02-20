use anyhow::{Context, Result};
use esp_idf_svc::nvs::{EspDefaultNvsPartition, EspNvs, NvsDefault};
use log::info;

use super::Settings;

const NAMESPACE: &str = "flipper_cfg";
const KEY_WIFI_SSID: &str = "wifi_ssid";
const KEY_WIFI_PASS: &str = "wifi_pass";
const KEY_BAUD_RATE: &str = "baud_rate";
const KEY_DEVICE_NAME: &str = "dev_name";

pub struct NvsStorage {
    nvs: EspNvs<NvsDefault>,
}

impl NvsStorage {
    pub fn new(partition: EspDefaultNvsPartition) -> Result<Self> {
        let nvs = EspNvs::new(partition, NAMESPACE, true)
            .context("Failed to open NVS namespace")?;
        Ok(Self { nvs })
    }

    pub fn load_settings(&self) -> Settings {
        let mut settings = Settings::default();

        if let Some(ssid) = self.get_string(KEY_WIFI_SSID) {
            settings.wifi_ssid = ssid;
        }
        if let Some(pass) = self.get_string(KEY_WIFI_PASS) {
            settings.wifi_password = pass;
        }
        if let Ok(Some(baud)) = self.nvs.get_u32(KEY_BAUD_RATE) {
            settings.uart_baud_rate = baud;
        }
        if let Some(name) = self.get_string(KEY_DEVICE_NAME) {
            settings.device_name = name;
        }

        info!("Loaded settings from NVS (SSID: {:?})", settings.wifi_ssid);
        settings
    }

    pub fn save_settings(&mut self, settings: &Settings) -> Result<()> {
        self.nvs
            .set_str(KEY_WIFI_SSID, &settings.wifi_ssid)
            .context("Failed to save WiFi SSID")?;
        self.nvs
            .set_str(KEY_WIFI_PASS, &settings.wifi_password)
            .context("Failed to save WiFi password")?;
        self.nvs
            .set_u32(KEY_BAUD_RATE, settings.uart_baud_rate)
            .context("Failed to save baud rate")?;
        self.nvs
            .set_str(KEY_DEVICE_NAME, &settings.device_name)
            .context("Failed to save device name")?;

        info!("Settings saved to NVS");
        Ok(())
    }

    fn get_string(&self, key: &str) -> Option<String> {
        let len = match self.nvs.str_len(key) {
            Ok(Some(len)) if len > 0 => len,
            _ => return None,
        };
        let mut buf = vec![0u8; len];
        match self.nvs.get_str(key, &mut buf) {
            Ok(Some(s)) if !s.is_empty() => Some(s.to_string()),
            _ => None,
        }
    }
}
