/// WebSocket tunnel endpoint — the ESP32 device connects here.
///
/// Protocol:
///   1. ESP32 connects with HTTP header `X-Device-Id: <id>`
///   2. Relay accepts WS connection and registers the device
///   3. MCP requests arrive as text frames (JSON-RPC)
///   4. Device sends responses as text frames (JSON-RPC)
///   5. Relay routes each response to the waiting HTTP handler via the pending map
use std::sync::Arc;

use axum::extract::{State, WebSocketUpgrade};
use axum::extract::ws::{Message, WebSocket};
use axum::http::{HeaderMap, StatusCode};
use axum::response::{IntoResponse, Response};
use axum::routing::get;
use axum::Router;
use dashmap::DashMap;
use futures_util::{SinkExt, StreamExt};
use serde_json::Value;
use tokio::sync::{oneshot, Mutex};
use tracing::{info, warn};

/// A request waiting for a response from the Flipper.
/// Keyed by JSON-RPC `id` (serialized as a string).
pub type PendingMap = Arc<DashMap<String, oneshot::Sender<String>>>;

/// Shared relay state — one connected device at a time (simple single-device model).
pub struct TunnelState {
    /// Sender side of the device's WebSocket connection (if connected).
    /// Protected by a Mutex so HTTP handlers can send through it.
    pub device_tx: Arc<Mutex<Option<futures_util::stream::SplitSink<WebSocket, Message>>>>,
    /// In-flight requests waiting for responses from the device.
    pub pending: PendingMap,
    /// Human-readable device ID from the X-Device-Id header.
    pub device_id: Arc<Mutex<Option<String>>>,
}

impl TunnelState {
    pub fn new() -> Self {
        Self {
            device_tx: Arc::new(Mutex::new(None)),
            pending: Arc::new(DashMap::new()),
            device_id: Arc::new(Mutex::new(None)),
        }
    }

    pub async fn is_connected(&self) -> bool {
        self.device_tx.lock().await.is_some()
    }
}

pub fn router(state: Arc<TunnelState>) -> Router {
    Router::new()
        .route("/tunnel", get(tunnel_handler))
        .route("/health", get(health_handler))
        .with_state(state)
}

async fn health_handler(State(state): State<Arc<TunnelState>>) -> impl IntoResponse {
    let device_id = state.device_id.lock().await.clone();
    let connected = state.is_connected().await;
    axum::Json(serde_json::json!({
        "status": "ok",
        "device_connected": connected,
        "device_id": device_id,
    }))
}

async fn tunnel_handler(
    State(state): State<Arc<TunnelState>>,
    headers: HeaderMap,
    ws: WebSocketUpgrade,
) -> Response {
    let device_id = headers
        .get("x-device-id")
        .and_then(|v| v.to_str().ok())
        .unwrap_or("unknown")
        .to_string();

    ws.on_upgrade(move |socket| handle_device_ws(socket, state, device_id))
}

async fn handle_device_ws(socket: WebSocket, state: Arc<TunnelState>, device_id: String) {
    info!("Device '{}' connected via tunnel", device_id);
    *state.device_id.lock().await = Some(device_id.clone());

    let (sender, mut receiver) = socket.split();
    *state.device_tx.lock().await = Some(sender);

    // Read loop — receive responses from the device and route them to waiting HTTP handlers
    while let Some(Ok(msg)) = receiver.next().await {
        match msg {
            Message::Text(text) => {
                route_response(&state.pending, text.as_str());
            }
            Message::Binary(bytes) => {
                if let Ok(text) = std::str::from_utf8(&bytes) {
                    route_response(&state.pending, text);
                }
            }
            Message::Close(_) => break,
            _ => {}
        }
    }

    info!("Device '{}' disconnected", device_id);
    *state.device_tx.lock().await = None;
    *state.device_id.lock().await = None;

    // Fail any remaining pending requests
    state.pending.retain(|_, tx| {
        let _ = tx; // drop the sender — the receiver will see an Err
        false
    });
}

/// Extract the JSON-RPC id from a response and deliver it to the waiting handler.
fn route_response(pending: &PendingMap, text: &str) {
    let id_key = extract_id_key(text);
    if let Some((_, tx)) = pending.remove(&id_key) {
        let _ = tx.send(text.to_string());
    } else {
        warn!("Received response for unknown request id: {}", id_key);
    }
}

/// Serialize the JSON-RPC `id` field to a string key for the pending map.
/// Handles null, number, and string ids.
pub fn extract_id_key(json: &str) -> String {
    if let Ok(v) = serde_json::from_str::<Value>(json) {
        match &v["id"] {
            Value::Null => "null".to_string(),
            Value::Number(n) => n.to_string(),
            Value::String(s) => s.clone(),
            other => other.to_string(),
        }
    } else {
        "null".to_string()
    }
}

/// Send a request to the connected device and wait for the response.
/// Returns Err if no device is connected or the device disconnects before responding.
pub async fn send_to_device(
    state: &TunnelState,
    request_body: &str,
) -> Result<Option<String>, StatusCode> {
    let id_key = extract_id_key(request_body);
    let is_notification = id_key == "null"
        && serde_json::from_str::<Value>(request_body)
            .ok()
            .and_then(|v| v.get("id").cloned())
            .map(|id| id.is_null())
            .unwrap_or(false);

    {
        let mut tx = state.device_tx.lock().await;
        match tx.as_mut() {
            Some(sender) => {
                sender
                    .send(Message::Text(request_body.to_string()))
                    .await
                    .map_err(|_| StatusCode::BAD_GATEWAY)?;
            }
            None => return Err(StatusCode::SERVICE_UNAVAILABLE),
        }
    }

    // Notifications don't expect a response
    if is_notification {
        return Ok(None);
    }

    let (tx, rx) = oneshot::channel();
    state.pending.insert(id_key, tx);

    match tokio::time::timeout(std::time::Duration::from_secs(30), rx).await {
        Ok(Ok(response)) => Ok(Some(response)),
        Ok(Err(_)) => Err(StatusCode::BAD_GATEWAY),     // device disconnected
        Err(_) => Err(StatusCode::GATEWAY_TIMEOUT),     // 30s timeout
    }
}
