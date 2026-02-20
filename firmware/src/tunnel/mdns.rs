use anyhow::Result;
use esp_idf_svc::mdns::EspMdns;
use log::info;

/// Advertise this device on the local network via mDNS.
/// After calling this the device is reachable at `{hostname}.local:8080`.
/// The returned `EspMdns` must be kept alive for the advertisement to persist.
pub fn start_mdns(hostname: &str) -> Result<EspMdns> {
    let mut mdns = EspMdns::take()?;
    mdns.set_hostname(hostname)?;
    mdns.set_instance_name(&format!("Flipper MCP ({})", hostname))?;
    // Advertise the MCP HTTP service so clients can discover it without knowing the IP
    mdns.add_service(None, "_mcp", "_tcp", 8080, &[])?;
    // Also advertise plain HTTP for browsers / generic discovery
    mdns.add_service(None, "_http", "_tcp", 8080, &[])?;
    info!("mDNS: advertising {}.local:8080", hostname);
    Ok(mdns)
}
