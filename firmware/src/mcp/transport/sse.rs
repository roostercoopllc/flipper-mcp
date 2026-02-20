use std::collections::{HashMap, VecDeque};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::Duration;

use anyhow::Result;
use esp_idf_svc::http::server::EspHttpServer;
use esp_idf_svc::http::Method;
use esp_idf_svc::io::Write;

use crate::mcp::server::McpServer;

/// Per-session queue: maps sessionId → pending SSE messages
pub type SseState = Arc<Mutex<HashMap<String, VecDeque<String>>>>;

/// Create a new, empty SSE session registry.
pub fn new_sse_state() -> SseState {
    Arc::new(Mutex::new(HashMap::new()))
}

/// Register `GET /sse` and `POST /messages` handlers on an existing HTTP server.
pub fn register_sse_handlers(
    http: &mut EspHttpServer<'static>,
    mcp_server: Arc<McpServer>,
    sessions: SseState,
) -> Result<()> {
    // GET /sse ──────────────────────────────────────────────────────────────
    // Opens an SSE stream: sends the endpoint event, then delivers JSON-RPC
    // responses as `event: message` events. Heartbeat comment every 25s.
    let sessions_get = sessions.clone();
    http.fn_handler::<anyhow::Error, _>("/sse", Method::Get, move |request| {
        let session_id = random_session_id();
        let endpoint = format!("/messages?sessionId={}", session_id);
        sessions_get
            .lock()
            .unwrap()
            .insert(session_id.clone(), VecDeque::new());

        log::info!("SSE session opened: {}", session_id);

        let mut resp = request.into_response(
            200,
            Some("OK"),
            &[
                ("Content-Type", "text/event-stream"),
                ("Cache-Control", "no-cache"),
                ("X-Accel-Buffering", "no"),
            ],
        )?;

        // Send the endpoint event so the client knows where to POST requests
        let endpoint_event = format!("event: endpoint\ndata: {}\n\n", endpoint);
        resp.write_all(endpoint_event.as_bytes())?;

        // Deliver responses and send heartbeats until the connection drops
        loop {
            thread::sleep(Duration::from_secs(25));

            let pending: Vec<String> = {
                let mut s = sessions_get.lock().unwrap();
                match s.get_mut(&session_id) {
                    Some(q) => q.drain(..).collect(),
                    None => break, // session removed (e.g., server stopped)
                }
            };

            for msg in pending {
                let event = format!("event: message\ndata: {}\n\n", msg);
                if resp.write_all(event.as_bytes()).is_err() {
                    sessions_get.lock().unwrap().remove(&session_id);
                    log::info!("SSE session {} closed (client disconnected on message)", session_id);
                    return Ok(());
                }
            }

            // Heartbeat comment — keeps connection alive through proxies/load balancers
            if resp.write_all(b": heartbeat\n\n").is_err() {
                break;
            }
        }

        sessions_get.lock().unwrap().remove(&session_id);
        log::info!("SSE session {} closed", session_id);
        Ok(())
    })
    .map_err(|e| anyhow::anyhow!("Failed to register GET /sse: {e}"))?;

    // POST /messages ────────────────────────────────────────────────────────
    // Receives JSON-RPC requests from the MCP client. The response is enqueued
    // to the client's SSE session queue; this handler returns 202 Accepted.
    let sessions_post = sessions;
    http.fn_handler::<anyhow::Error, _>("/messages", Method::Post, move |mut request| {
        let session_id = parse_session_id(request.uri());

        // Read request body
        let mut buf = [0u8; 4096];
        let mut body = Vec::new();
        loop {
            let n = request.read(&mut buf).map_err(|e| anyhow::anyhow!("{e}"))?;
            if n == 0 {
                break;
            }
            body.extend_from_slice(&buf[..n]);
            if body.len() > 16384 {
                break;
            }
        }

        let body_str = std::str::from_utf8(&body).unwrap_or("");

        // Process the JSON-RPC request and enqueue the response
        if let Some(response_json) = mcp_server.handle_request(body_str) {
            if let Some(sid) = session_id {
                let mut s = sessions_post.lock().unwrap();
                if let Some(queue) = s.get_mut(&sid) {
                    queue.push_back(response_json);
                } else {
                    log::warn!("POST /messages: unknown sessionId {}", sid);
                }
            }
        }

        request.into_response(202, Some("Accepted"), &[])?;
        Ok(())
    })
    .map_err(|e| anyhow::anyhow!("Failed to register POST /messages: {e}"))?;

    log::info!("SSE handlers registered: GET /sse, POST /messages");
    Ok(())
}

/// Generate a random 8-hex-char session ID using the ESP32 hardware RNG.
fn random_session_id() -> String {
    let r = unsafe { esp_idf_svc::sys::esp_random() };
    format!("{:08x}", r)
}

/// Extract `sessionId` from a URI like `/messages?sessionId=abc123&other=x`.
fn parse_session_id(uri: &str) -> Option<String> {
    uri.split('?')
        .nth(1)?
        .split('&')
        .find_map(|kv| kv.strip_prefix("sessionId="))
        .map(|s| s.to_string())
}
