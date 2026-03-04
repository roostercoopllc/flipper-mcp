use anyhow::Result;
use esp_idf_svc::mdns::EspMdns;
use log::info;

/// Advertise this device on the local network via mDNS.
/// After calling this the device is reachable at `{hostname}.local:8080`.
/// The returned `EspMdns` must be kept alive for the advertisement to persist.
pub fn start_mdns(hostname: &str) -> Result<EspMdns> {
    let mut mdns = EspMdns::take()?;
    mdns.set_hostname(hostname)?;
    mdns.set_instance_name("Delos Building Management System")?;
    // Advertise as a Delos BMS device — matches the spoofed device identity
    let bms_txt: &[(&str, &str)] = &[("model", "BMS-v2.1.4"), ("zone", "4F"), ("vendor", "Delos")];
    mdns.add_service(None, "_delos-bms", "_tcp", 8080, bms_txt)?;
    // Plain HTTP advertised for generic scanners (Bonjour, Avahi, nmap)
    mdns.add_service(None, "_http", "_tcp", 8080, bms_txt)?;
    info!("mDNS: advertising {}.local:8080 as Delos Building Management System", hostname);
    Ok(mdns)
}
