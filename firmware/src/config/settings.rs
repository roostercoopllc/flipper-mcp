use anyhow::Result;
use log::{info, warn};
use serde::{Deserialize, Serialize};

/// Path to the config file on the Flipper Zero's SD card.
/// Read via UART `storage read` after transport is initialized.
pub const SD_CONFIG_PATH: &str = "/ext/apps_data/flipper_mcp/config.txt";

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Settings {
    pub wifi_ssid: String,
    pub wifi_password: String,
    pub uart_baud_rate: u32,
    pub device_name: String,
    /// Optional WebSocket relay URL, e.g. `ws://relay.example.com:9090/tunnel`.
    /// Empty string disables the tunnel.
    pub relay_url: String,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            wifi_ssid: String::new(),
            wifi_password: String::new(),
            uart_baud_rate: 115_200,
            device_name: "flipper-mcp".to_string(),
            relay_url: String::new(),
        }
    }
}

impl Settings {
    /// Merge values from a key=value text file read from the Flipper SD card.
    /// Only overwrites fields that are present in the file.
    /// Lines starting with # are comments. Blank lines are ignored.
    ///
    /// Example file contents:
    /// ```text
    /// # Flipper MCP Configuration
    /// wifi_ssid=MyNetwork
    /// wifi_password=MyPassword
    /// device_name=flipper-mcp
    /// ```
    pub fn merge_from_text(&mut self, text: &str) {
        for line in text.lines() {
            let line = line.trim();
            if line.is_empty() || line.starts_with('#') {
                continue;
            }
            if let Some((key, value)) = line.split_once('=') {
                let key = key.trim();
                let value = value.trim();
                match key {
                    "wifi_ssid" => {
                        self.wifi_ssid = value.to_string();
                        info!("SD config: wifi_ssid set");
                    }
                    "wifi_password" => {
                        self.wifi_password = value.to_string();
                        info!("SD config: wifi_password set");
                    }
                    "uart_baud_rate" => {
                        if let Ok(baud) = value.parse::<u32>() {
                            self.uart_baud_rate = baud;
                            info!("SD config: uart_baud_rate = {}", baud);
                        } else {
                            warn!("SD config: invalid uart_baud_rate: {}", value);
                        }
                    }
                    "device_name" => {
                        self.device_name = value.to_string();
                        info!("SD config: device_name = {}", value);
                    }
                    "relay_url" => {
                        self.relay_url = value.to_string();
                        info!("SD config: relay_url set");
                    }
                    _ => {
                        warn!("SD config: unknown key: {}", key);
                    }
                }
            }
        }
    }

    /// Parse pipe-delimited key=value pairs from a FAP protocol CONFIG message.
    /// Example: `"ssid=MyNetwork|password=secret|device=flipper-mcp|relay="`
    pub fn merge_from_pipe_pairs(&mut self, payload: &str) {
        for pair in payload.split('|') {
            let pair = pair.trim();
            if pair.is_empty() {
                continue;
            }
            if let Some((key, value)) = pair.split_once('=') {
                match key.trim() {
                    "ssid" => {
                        self.wifi_ssid = value.to_string();
                        info!("FAP config: wifi_ssid set");
                    }
                    "password" => {
                        self.wifi_password = value.to_string();
                        info!("FAP config: wifi_password set");
                    }
                    "device" | "device_name" => {
                        self.device_name = value.to_string();
                        info!("FAP config: device_name = {}", value);
                    }
                    "relay" | "relay_url" => {
                        self.relay_url = value.to_string();
                        info!("FAP config: relay_url set");
                    }
                    _ => {
                        warn!("FAP config: unknown key: {}", key);
                    }
                }
            }
        }
    }
}
