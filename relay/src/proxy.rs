/// HTTP proxy endpoints — MCP clients connect here; requests are forwarded to the device.
///
/// Supported endpoints:
///   POST /mcp             — Streamable HTTP JSON-RPC (MCP 2025-03-26)
///   GET  /mcp             — 405 Method Not Allowed
///   GET  /sse             — Legacy SSE (MCP pre-2025)
///   POST /messages        — Legacy SSE message endpoint
use std::sync::Arc;

use axum::body::Bytes;
use axum::extract::{Query, State};
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response, Sse};
use axum::response::sse::Event;
use axum::routing::{get, post};
use axum::Router;
use futures_util::{stream, StreamExt};
use serde::Deserialize;
use tracing::info;

use crate::tunnel::{send_to_device, TunnelState};

pub fn router(state: Arc<TunnelState>) -> Router {
    Router::new()
        .route("/mcp", post(mcp_post_handler))
        .route("/mcp", get(mcp_get_handler))
        .route("/sse", get(sse_handler))
        .route("/messages", post(messages_handler))
        .with_state(state)
}

/// POST /mcp — forward JSON-RPC to device, return response
async fn mcp_post_handler(
    State(state): State<Arc<TunnelState>>,
    body: Bytes,
) -> Response {
    let body_str = match std::str::from_utf8(&body) {
        Ok(s) => s,
        Err(_) => return (StatusCode::BAD_REQUEST, "Invalid UTF-8 body").into_response(),
    };

    info!(
        "POST /mcp ({} bytes) → device",
        body_str.len()
    );

    match send_to_device(&state, body_str).await {
        Ok(Some(response)) => (
            StatusCode::OK,
            [(axum::http::header::CONTENT_TYPE, "application/json")],
            response,
        )
            .into_response(),
        Ok(None) => StatusCode::ACCEPTED.into_response(),
        Err(status) => status.into_response(),
    }
}

/// GET /mcp — not used for streamable HTTP; return 405
async fn mcp_get_handler() -> impl IntoResponse {
    StatusCode::METHOD_NOT_ALLOWED
}

/// GET /sse — legacy SSE transport: send endpoint event, then stream responses
async fn sse_handler(State(state): State<Arc<TunnelState>>) -> Response {
    if !state.is_connected().await {
        return (StatusCode::SERVICE_UNAVAILABLE, "No device connected").into_response();
    }

    // Generate a session ID and send the endpoint event, then a heartbeat stream.
    // For the relay, full SSE session management would require persisting session
    // queues — this simplified version sends the messages endpoint then keeps-alive.
    let session_id = uuid::Uuid::new_v4().simple().to_string();
    let endpoint_event = format!("/messages?sessionId={}", session_id);
    info!("SSE session {} started", session_id);

    let events = stream::iter(vec![
        Ok::<Event, std::convert::Infallible>(
            Event::default().event("endpoint").data(endpoint_event),
        ),
    ])
    .chain(stream::unfold((), |_| async {
        tokio::time::sleep(std::time::Duration::from_secs(25)).await;
        Some((
            Ok::<Event, std::convert::Infallible>(Event::default().comment("heartbeat")),
            (),
        ))
    }));

    Sse::new(events)
        .keep_alive(
            axum::response::sse::KeepAlive::new()
                .interval(std::time::Duration::from_secs(25))
                .text("heartbeat"),
        )
        .into_response()
}

/// POST /messages?sessionId=xxx — legacy SSE: forward JSON-RPC to device
#[derive(Deserialize)]
struct SessionQuery {
    #[serde(rename = "sessionId")]
    session_id: Option<String>,
}

async fn messages_handler(
    State(state): State<Arc<TunnelState>>,
    Query(query): Query<SessionQuery>,
    body: Bytes,
) -> Response {
    let body_str = match std::str::from_utf8(&body) {
        Ok(s) => s,
        Err(_) => return (StatusCode::BAD_REQUEST, "Invalid UTF-8").into_response(),
    };

    info!(
        "POST /messages session={:?} ({} bytes) → device",
        query.session_id,
        body_str.len()
    );

    // For the relay, the response is delivered over the existing SSE connection.
    // We forward the request but don't need to return the response in the POST body.
    // The device would push the response to the SSE stream via the session queue.
    // This simplified implementation just forwards the request to the device.
    match send_to_device(&state, body_str).await {
        Ok(_) => StatusCode::ACCEPTED.into_response(),
        Err(status) => status.into_response(),
    }
}
