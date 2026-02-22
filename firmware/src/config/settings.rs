use log::{info, warn};

#[derive(Debug, Clone)]
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
