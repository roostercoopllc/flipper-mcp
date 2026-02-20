use std::sync::{Arc, Mutex};

use anyhow::{Context, Result};
use esp_idf_svc::hal::modem::Modem;
use esp_idf_svc::http::server::{Configuration as HttpConfig, EspHttpServer};
use esp_idf_svc::http::Method;
use esp_idf_svc::io::Write;
use esp_idf_svc::nvs::EspDefaultNvsPartition;
use esp_idf_svc::eventloop::EspSystemEventLoop;
use esp_idf_svc::wifi::{AccessPointConfiguration, BlockingWifi, Configuration, EspWifi};
use log::info;

use crate::config::{NvsStorage, Settings};

pub const AP_IP: &str = "192.168.4.1";
const AP_SSID_PREFIX: &str = "FlipperMCP";

/// Start the ESP32 in WiFi AP mode.
/// The SSID is `FlipperMCP-XXXX` where XXXX comes from the last 2 bytes of the MAC address.
pub fn start_access_point(
    modem: Modem,
    sys_loop: EspSystemEventLoop,
    nvs_partition: EspDefaultNvsPartition,
) -> Result<BlockingWifi<EspWifi<'static>>> {
    let mac_suffix = read_mac_suffix();
    let ssid_str = format!("{}-{:04X}", AP_SSID_PREFIX, mac_suffix);

    info!("Starting WiFi AP: SSID={}", ssid_str);

    let mut wifi = BlockingWifi::wrap(
        EspWifi::new(modem, sys_loop.clone(), Some(nvs_partition))
            .context("Failed to create EspWifi")?,
        sys_loop,
    )?;

    let config = Configuration::AccessPoint(AccessPointConfiguration {
        ssid: ssid_str.as_str().try_into().unwrap_or_default(),
        password: "".try_into().unwrap(), // open network
        channel: 6,
        max_connections: 4,
        ..Default::default()
    });

    wifi.set_configuration(&config)?;
    wifi.start().context("AP start failed")?;
    wifi.wait_netif_up().context("AP netif failed to come up")?;

    info!("AP ready — connect to '{}' then open http://{}", ssid_str, AP_IP);
    Ok(wifi)
}

/// Start the captive portal HTTP server on port 80.
/// Serves a WiFi config form; on submit saves credentials to NVS and reboots.
pub fn start_portal_server(nvs_partition: EspDefaultNvsPartition) -> Result<EspHttpServer<'static>> {
    let config = HttpConfig {
        http_port: 80,
        stack_size: 8192,
        max_uri_handlers: 4,
        ..Default::default()
    };
    let mut http = EspHttpServer::new(&config).context("Failed to start portal HTTP server")?;

    // GET / — serve the config form
    http.fn_handler::<anyhow::Error, _>("/", Method::Get, |request| {
        request
            .into_response(200, Some("OK"), &[("Content-Type", "text/html; charset=utf-8")])?
            .write_all(PORTAL_HTML.as_bytes())?;
        Ok(())
    })
    .context("Failed to register GET /")?;

    // POST /configure — parse form body, save credentials, reboot
    let nvs = Arc::new(Mutex::new(
        NvsStorage::new(nvs_partition).context("Failed to open NVS for portal")?,
    ));
    http.fn_handler::<anyhow::Error, _>("/configure", Method::Post, move |mut request| {
        let mut buf = [0u8; 256];
        let mut body = Vec::new();
        loop {
            let n = request.read(&mut buf).map_err(|e| anyhow::anyhow!("{e}"))?;
            if n == 0 {
                break;
            }
            body.extend_from_slice(&buf[..n]);
            if body.len() > 256 {
                break;
            }
        }

        let body_str = std::str::from_utf8(&body).unwrap_or("");
        let (ssid, pass) = parse_form_body(body_str);

        if !ssid.is_empty() {
            let mut settings = Settings::default();
            settings.wifi_ssid = ssid;
            settings.wifi_password = pass;

            let mut storage = nvs.lock().unwrap();
            if let Err(e) = storage.save_settings(&settings) {
                log::error!("Failed to save WiFi settings: {}", e);
            } else {
                info!("WiFi credentials saved, rebooting...");
            }
        }

        request
            .into_response(200, Some("OK"), &[("Content-Type", "text/html; charset=utf-8")])?
            .write_all(SAVED_HTML.as_bytes())?;

        // Spawn a thread to reboot after the response is flushed
        std::thread::spawn(|| {
            std::thread::sleep(std::time::Duration::from_millis(500));
            unsafe { esp_idf_svc::sys::esp_restart() };
        });

        Ok(())
    })
    .context("Failed to register POST /configure")?;

    info!("Captive portal HTTP server started on port 80");
    Ok(http)
}

/// Read the last 2 bytes of the STA MAC address for the AP SSID suffix.
fn read_mac_suffix() -> u16 {
    let mut mac = [0u8; 6];
    unsafe {
        esp_idf_svc::sys::esp_read_mac(mac.as_mut_ptr(), esp_idf_svc::sys::esp_mac_type_t_ESP_MAC_WIFI_STA);
    }
    u16::from_be_bytes([mac[4], mac[5]])
}

/// Parse `application/x-www-form-urlencoded` body: `ssid=MyNet&pass=secret`
fn parse_form_body(body: &str) -> (String, String) {
    let mut ssid = String::new();
    let mut pass = String::new();

    for part in body.split('&') {
        if let Some((key, value)) = part.split_once('=') {
            let decoded = url_decode(value);
            match key {
                "ssid" => ssid = decoded,
                "pass" => pass = decoded,
                _ => {}
            }
        }
    }
    (ssid, pass)
}

/// Minimal URL percent-decoding for form values.
fn url_decode(s: &str) -> String {
    let s = s.replace('+', " ");
    let mut result = String::with_capacity(s.len());
    let mut chars = s.chars().peekable();
    while let Some(c) = chars.next() {
        if c == '%' {
            let h1 = chars.next().and_then(|c| c.to_digit(16));
            let h2 = chars.next().and_then(|c| c.to_digit(16));
            if let (Some(h1), Some(h2)) = (h1, h2) {
                result.push(char::from((h1 * 16 + h2) as u8));
            }
        } else {
            result.push(c);
        }
    }
    result
}

const PORTAL_HTML: &str = r#"<!DOCTYPE html>
<html>
<head>
  <meta charset="utf-8">
  <meta name="viewport" content="width=device-width,initial-scale=1">
  <title>FlipperMCP Setup</title>
  <style>
    body{font-family:sans-serif;max-width:400px;margin:40px auto;padding:20px;color:#333}
    h2{color:#f60}
    label{display:block;margin:12px 0 4px;font-weight:bold}
    input{width:100%;padding:8px;margin:0;box-sizing:border-box;border:1px solid #ccc;border-radius:4px}
    button{width:100%;padding:12px;margin-top:16px;background:#f60;color:#fff;border:none;border-radius:4px;font-size:16px;cursor:pointer}
    button:hover{background:#d50}
  </style>
</head>
<body>
  <h2>FlipperMCP WiFi Setup</h2>
  <p>Connect this Flipper WiFi Dev Board to your local network.</p>
  <form method="POST" action="/configure">
    <label for="ssid">WiFi Network (SSID)</label>
    <input id="ssid" name="ssid" type="text" required maxlength="32" autocomplete="off">
    <label for="pass">Password</label>
    <input id="pass" name="pass" type="password" maxlength="64" autocomplete="off">
    <button type="submit">Save &amp; Connect</button>
  </form>
</body>
</html>"#;

const SAVED_HTML: &str = r#"<!DOCTYPE html>
<html>
<head>
  <meta charset="utf-8">
  <title>FlipperMCP Setup</title>
  <style>body{font-family:sans-serif;max-width:400px;margin:40px auto;padding:20px;color:#333}h2{color:#f60}</style>
</head>
<body>
  <h2>Saved!</h2>
  <p>FlipperMCP is rebooting and connecting to your WiFi network.</p>
  <p>You can close this page. Once connected, the device will be accessible at its local IP on port 8080.</p>
</body>
</html>"#;
