/// mDNS advertisement — requires `espressif/mdns` managed component.
/// Wrapped in cfg so the firmware compiles cleanly when the component isn't present.
#[cfg(any(esp_idf_comp_mdns_enabled, esp_idf_comp_espressif__mdns_enabled))]
pub mod mdns;

/// WebSocket tunnel client — requires `espressif/esp_websocket_client` managed component.
#[cfg(esp_idf_comp_espressif__esp_websocket_client_enabled)]
pub mod client;

use std::any::Any;
use std::sync::Arc;

use log::info;

use crate::mcp::McpServer;

/// Attempt to start mDNS advertisement. Returns an opaque handle that must stay alive
/// for the advertisement to persist; returns None if the mDNS component isn't available.
///
/// To enable: add `espressif/mdns: ">=1.3.0"` to `firmware/idf_component.yml`,
/// then `cargo clean && cargo build`.
pub fn start_mdns_if_available(hostname: &str) -> Option<Box<dyn Any + Send + 'static>> {
    #[cfg(any(esp_idf_comp_mdns_enabled, esp_idf_comp_espressif__mdns_enabled))]
    {
        match mdns::start_mdns(hostname) {
            Ok(handle) => return Some(Box::new(handle)),
            Err(e) => log::warn!("mDNS init failed ({}); local discovery unavailable", e),
        }
    }
    #[cfg(not(any(esp_idf_comp_mdns_enabled, esp_idf_comp_espressif__mdns_enabled)))]
    info!(
        "mDNS component not built — add espressif/mdns to idf_component.yml for {}.local",
        hostname
    );
    None
}

/// Start the reverse WebSocket tunnel if a relay URL is configured and the WS client
/// component is present. Logs an info message and returns if either condition is unmet.
///
/// To enable: add `espressif/esp_websocket_client: ">=1.1.0"` to `firmware/idf_component.yml`,
/// then `cargo clean && cargo build`.
pub fn start_tunnel_if_available(relay_url: &str, mcp_server: Arc<McpServer>) {
    if relay_url.is_empty() {
        return;
    }
    #[cfg(esp_idf_comp_espressif__esp_websocket_client_enabled)]
    {
        info!("Starting tunnel to {}", relay_url);
        client::start_tunnel(relay_url.to_string(), mcp_server);
        return;
    }
    // Suppress unused warning when cfg is false
    let _ = mcp_server;
    info!(
        "Tunnel component not built — add espressif/esp_websocket_client to idf_component.yml \
         to enable remote access via relay ({})",
        relay_url
    );
}
