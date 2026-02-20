use std::sync::Arc;

use anyhow::{Context, Result};
use esp_idf_svc::http::server::{Configuration, EspHttpServer};
use esp_idf_svc::http::Method;
use esp_idf_svc::io::Write;
use log::info;

use crate::mcp::server::McpServer;

use super::sse::{register_sse_handlers, SseState};

const MAX_REQUEST_BODY: usize = 16384; // 16KB

pub fn start_http_server(server: Arc<McpServer>, sse_state: SseState) -> Result<EspHttpServer<'static>> {
    let config = Configuration {
        http_port: 8080,
        stack_size: 10240,
        max_uri_handlers: 8,
        ..Default::default()
    };

    let mut http = EspHttpServer::new(&config).context("Failed to start HTTP server")?;
    info!("HTTP server starting on port 8080");

    // POST /mcp — Streamable HTTP JSON-RPC requests
    let server_post = server.clone();
    http.fn_handler::<anyhow::Error, _>("/mcp", Method::Post, move |mut request| {
        let mut buf = [0u8; 4096];
        let mut body = Vec::new();
        loop {
            let n = request.read(&mut buf).map_err(|e| anyhow::anyhow!("{e}"))?;
            if n == 0 {
                break;
            }
            body.extend_from_slice(&buf[..n]);
            if body.len() > MAX_REQUEST_BODY {
                let resp = r#"{"jsonrpc":"2.0","id":null,"error":{"code":-32600,"message":"Request body too large"}}"#;
                request
                    .into_response(200, Some("OK"), &[("Content-Type", "application/json")])?
                    .write_all(resp.as_bytes())?;
                return Ok(());
            }
        }

        let body_str = std::str::from_utf8(&body).unwrap_or("");

        match server_post.handle_request(body_str) {
            Some(response_json) => {
                request
                    .into_response(200, Some("OK"), &[("Content-Type", "application/json")])?
                    .write_all(response_json.as_bytes())?;
            }
            None => {
                request.into_response(202, Some("Accepted"), &[])?;
            }
        }

        Ok(())
    })
    .map_err(|e| anyhow::anyhow!("Failed to register POST /mcp: {e}"))?;

    // GET /mcp — 405 (Streamable HTTP doesn't use GET /mcp for SSE)
    http.fn_handler::<anyhow::Error, _>("/mcp", Method::Get, |request| {
        request.into_response(405, Some("Method Not Allowed"), &[])?;
        Ok(())
    })
    .map_err(|e| anyhow::anyhow!("Failed to register GET /mcp: {e}"))?;

    // GET /health — health check
    http.fn_handler::<anyhow::Error, _>("/health", Method::Get, |request| {
        let body = format!(
            r#"{{"status":"ok","version":"{}"}}"#,
            env!("CARGO_PKG_VERSION")
        );
        request
            .into_response(200, Some("OK"), &[("Content-Type", "application/json")])?
            .write_all(body.as_bytes())?;
        Ok(())
    })
    .map_err(|e| anyhow::anyhow!("Failed to register GET /health: {e}"))?;

    // Legacy SSE handlers: GET /sse and POST /messages
    register_sse_handlers(&mut http, server, sse_state)?;

    info!("HTTP server ready — POST /mcp, GET /health, GET /sse, POST /messages");
    Ok(http)
}
