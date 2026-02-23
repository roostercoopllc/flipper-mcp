use anyhow::{bail, ensure, Context, Result};
use esp_idf_svc::eventloop::EspSystemEventLoop;
use esp_idf_svc::hal::modem::Modem;
use esp_idf_svc::nvs::EspDefaultNvsPartition;
use esp_idf_svc::wifi::{AuthMethod, BlockingWifi, ClientConfiguration, Configuration, EspWifi};
use log::{info, warn};

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

    let auth = parse_auth_method(&settings.wifi_auth, settings.wifi_password.is_empty());
    info!("WiFi auth: {:?} (config='{}')", auth, settings.wifi_auth);
    let config = Configuration::Client(ClientConfiguration {
        ssid: settings.wifi_ssid.as_str().try_into().unwrap(),
        password: settings.wifi_password.as_str().try_into().unwrap(),
        auth_method: auth,
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

/// Scan for visible APs. Returns a summary string for FAP logging.
pub fn scan_aps(wifi: &mut BlockingWifi<EspWifi<'static>>) -> String {
    match wifi.scan() {
        Ok(aps) => {
            info!("Scan found {} APs:", aps.len());
            let mut summary = format!("Scan: {} APs.", aps.len());
            for ap in aps.iter().take(5) {
                info!(
                    "  '{}' ch={} rssi={} auth={:?}",
                    ap.ssid, ap.channel, ap.signal_strength, ap.auth_method
                );
                // Add first few to summary for FAP display
                if summary.len() < 120 {
                    summary.push_str(&format!(" '{}' {}dBm", ap.ssid, ap.signal_strength));
                }
            }
            summary
        }
        Err(e) => {
            warn!("AP scan failed: {}", e);
            format!("Scan failed: {}", e)
        }
    }
}

/// Map the config string to an ESP-IDF AuthMethod.
///
/// Valid values: "wpa2", "wpa3", "wpa2wpa3", "open", or "" (auto).
/// Auto = WPA2Personal if password is set, None if open.
fn parse_auth_method(config_value: &str, no_password: bool) -> AuthMethod {
    match config_value.trim().to_lowercase().as_str() {
        "open" | "none" => AuthMethod::None,
        "wpa2" => AuthMethod::WPA2Personal,
        "wpa3" => AuthMethod::WPA3Personal,
        "wpa2wpa3" => AuthMethod::WPA2WPA3Personal,
        "wep" => AuthMethod::WEP,
        _ => {
            // Auto-detect: WPA2 if password set, open otherwise
            if no_password {
                AuthMethod::None
            } else {
                AuthMethod::WPA2Personal
            }
        }
    }
}

/// Start the WiFi radio and connect to the configured network.
/// Returns Ok(()) on success; Err on failure (caller can retry).
/// Safe to call repeatedly — resets WiFi state before each attempt.
pub fn start_and_connect(wifi: &mut BlockingWifi<EspWifi<'static>>) -> Result<()> {
    // Clean up any prior state so retries don't fail on "already started"
    let _ = wifi.disconnect();
    let _ = wifi.stop();

    wifi.start().context("WiFi start failed")?;
    info!("WiFi started");

    wifi.connect().context("WiFi connect failed")?;
    info!("WiFi connected");

    wifi.wait_netif_up().context("Network interface failed to come up")?;

    let ip_info = wifi.wifi().sta_netif().get_ip_info()?;
    info!("WiFi connected — IP: {}", ip_info.ip);
    Ok(())
}
