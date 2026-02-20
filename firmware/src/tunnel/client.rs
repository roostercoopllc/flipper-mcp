use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc;
use std::sync::Arc;
use std::thread;
use std::time::Duration;

use anyhow::Result;
use esp_idf_svc::ws::client::{
    EspWebSocketClient, EspWebSocketClientConfig, WebSocketEvent, WebSocketEventType,
};
use log::{error, info, warn};

use crate::mcp::McpServer;

const CONNECT_TIMEOUT: Duration = Duration::from_secs(10);
const HEARTBEAT_INTERVAL: Duration = Duration::from_secs(25);
const TUNNEL_STACK_SIZE: usize = 10240;

/// Spawn a background thread that maintains a WebSocket connection to the relay server.
/// MCP requests arriving over the tunnel are dispatched to `mcp_server` and responses
/// are sent back over the same WebSocket connection.
///
/// `relay_state` is set to `true` when the WebSocket is connected and `false` on
/// disconnect or error — allowing `main.rs` to surface this in `status.txt`.
///
/// The thread handles reconnection automatically with exponential backoff (5s → 60s max).
pub fn start_tunnel(relay_url: String, mcp_server: Arc<McpServer>, relay_state: Arc<AtomicBool>) {
    thread::Builder::new()
        .stack_size(TUNNEL_STACK_SIZE)
        .spawn(move || {
            let mut backoff_secs = 5u64;
            loop {
                info!("Tunnel: connecting to {}", relay_url);
                match run_session(&relay_url, &mcp_server, &relay_state) {
                    Ok(()) => {
                        info!("Tunnel: disconnected cleanly, reconnecting...");
                        relay_state.store(false, Ordering::Relaxed);
                        backoff_secs = 5;
                    }
                    Err(e) => {
                        warn!("Tunnel: session error ({}). Retrying in {}s", e, backoff_secs);
                        relay_state.store(false, Ordering::Relaxed);
                        thread::sleep(Duration::from_secs(backoff_secs));
                        backoff_secs = (backoff_secs * 2).min(60);
                    }
                }
            }
        })
        .expect("Failed to spawn tunnel thread");
}

/// Run one WebSocket session. Returns Ok(()) on clean disconnect, Err on failures.
fn run_session(
    relay_url: &str,
    mcp_server: &Arc<McpServer>,
    relay_state: &Arc<AtomicBool>,
) -> Result<()> {
    // Channel: WS event callback → processing loop
    let (tx, rx) = mpsc::sync_channel::<SessionEvent>(16);

    let tx_msg = tx.clone();
    let tx_disc = tx;

    let cfg = EspWebSocketClientConfig {
        reconnect_timeout_ms: 0,  // disable built-in reconnect; we do our own
        network_timeout_ms: 10_000,
        ..Default::default()
    };

    let relay_state_cb = relay_state.clone();

    let mut client = EspWebSocketClient::new(
        relay_url,
        &cfg,
        CONNECT_TIMEOUT,
        move |event: Result<WebSocketEvent<'_>, esp_idf_svc::sys::EspError>| match event {
            Ok(evt) => match &evt.event_type {
                WebSocketEventType::Connected => {
                    relay_state_cb.store(true, Ordering::Relaxed);
                    info!("Tunnel: WebSocket connected");
                }
                WebSocketEventType::Text(data) => {
                    let _ = tx_msg.try_send(SessionEvent::Message(data.to_string()));
                }
                WebSocketEventType::Binary(data) => {
                    // Some relays may send as binary; treat as UTF-8 text
                    if let Ok(s) = std::str::from_utf8(data) {
                        let _ = tx_msg.try_send(SessionEvent::Message(s.to_string()));
                    }
                }
                WebSocketEventType::Disconnected | WebSocketEventType::Closed => {
                    relay_state_cb.store(false, Ordering::Relaxed);
                    info!("Tunnel: WebSocket disconnected");
                    let _ = tx_disc.try_send(SessionEvent::Disconnected);
                }
                _ => {}
            },
            Err(e) => {
                relay_state_cb.store(false, Ordering::Relaxed);
                error!("Tunnel: WS event error: {}", e);
                let _ = tx_disc.try_send(SessionEvent::Disconnected);
            }
        },
    )
    .map_err(|e| anyhow::anyhow!("WebSocket connect failed: {}", e))?;

    loop {
        match rx.recv_timeout(HEARTBEAT_INTERVAL) {
            Ok(SessionEvent::Message(body)) => {
                if let Some(response) = mcp_server.handle_request(&body) {
                    client
                        .send(
                            esp_idf_svc::ws::FrameType::Text(false),
                            response.as_bytes(),
                        )
                        .map_err(|e| anyhow::anyhow!("WS send failed: {}", e))?;
                }
            }
            Ok(SessionEvent::Disconnected) => {
                return Ok(());
            }
            Err(mpsc::RecvTimeoutError::Timeout) => {
                // Send a heartbeat ping to keep the connection alive
                client
                    .send(esp_idf_svc::ws::FrameType::Ping, &[])
                    .map_err(|e| anyhow::anyhow!("WS ping failed: {}", e))?;
            }
            Err(mpsc::RecvTimeoutError::Disconnected) => {
                return Err(anyhow::anyhow!("Event channel closed unexpectedly"));
            }
        }
    }
}

enum SessionEvent {
    Message(String),
    Disconnected,
}
