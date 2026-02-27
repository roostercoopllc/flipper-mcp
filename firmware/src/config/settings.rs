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
    /// WiFi auth method: "wpa2", "wpa3", "wpa2wpa3", "open".
    /// Empty string = auto-detect (WPA2 if password set, open otherwise).
    pub wifi_auth: String,
    /// Optional MAC address spoofing. Format: "AA:BB:CC:DD:EE:FF"
    /// Empty string = use hardware default.
    /// Useful for impersonating server hardware during penetration testing.
    pub wifi_mac: String,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            wifi_ssid: String::new(),
            wifi_password: String::new(),
            uart_baud_rate: 115_200,
            device_name: "flipper-mcp".to_string(),
            relay_url: String::new(),
            wifi_auth: String::new(),
            wifi_mac: String::new(),
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
                        self.wifi_ssid = value.trim().to_string();
                        info!("FAP config: wifi_ssid set (len={})", self.wifi_ssid.len());
                    }
                    "password" => {
                        self.wifi_password = value.trim().to_string();
                        info!("FAP config: wifi_password set (len={})", self.wifi_password.len());
                    }
                    "device" | "device_name" => {
                        self.device_name = value.trim().to_string();
                        info!("FAP config: device_name = {}", self.device_name);
                    }
                    "relay" | "relay_url" => {
                        self.relay_url = value.trim().to_string();
                        info!("FAP config: relay_url set");
                    }
                    "wifi_auth" | "auth" => {
                        self.wifi_auth = value.trim().to_lowercase();
                        info!("FAP config: wifi_auth = {}", self.wifi_auth);
                    }
                    "wifi_mac" | "mac" => {
                        self.wifi_mac = value.trim().to_uppercase();
                        info!("FAP config: wifi_mac = {}", self.wifi_mac);
                    }
                    _ => {
                        warn!("FAP config: unknown key: {}", key);
                    }
                }
            }
        }
    }
}
