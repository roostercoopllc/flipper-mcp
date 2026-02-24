use std::sync::Arc;

use anyhow::{Context, Result};
use esp_idf_svc::http::server::{Configuration, EspHttpServer};
use esp_idf_svc::http::Method;
use esp_idf_svc::io::Write;
use log::info;

use crate::mcp::server::McpServer;
use crate::mcp::types::ToolDefinition;

use super::sse::{register_sse_handlers, SseState};

const MAX_REQUEST_BODY: usize = 16384; // 16KB

const CORS_HEADERS: &[(&str, &str)] = &[
    ("Access-Control-Allow-Origin", "*"),
    ("Access-Control-Allow-Methods", "GET, POST, OPTIONS"),
    ("Access-Control-Allow-Headers", "Content-Type, Accept"),
    ("Access-Control-Max-Age", "86400"),
];

/// Adapter: wraps an `esp_idf_svc::io::Write` implementor as a `std::io::Write`
/// so that `serde_json::to_writer` can stream JSON directly to the HTTP response.
struct StdIoWriter<W>(W);

impl<W: Write> std::io::Write for StdIoWriter<W>
where
    W::Error: std::fmt::Display,
{
    fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
        self.0
            .write(buf)
            .map_err(|e| std::io::Error::new(std::io::ErrorKind::Other, e.to_string()))
    }
    fn flush(&mut self) -> std::io::Result<()> {
        self.0
            .flush()
            .map_err(|e| std::io::Error::new(std::io::ErrorKind::Other, e.to_string()))
    }
}

pub fn start_http_server(server: Arc<McpServer>, sse_state: SseState) -> Result<EspHttpServer<'static>> {
    let config = Configuration {
        http_port: 8080,
        stack_size: 10240,
        max_uri_handlers: 10,
        ..Default::default()
    };

    let mut http = EspHttpServer::new(&config).context("Failed to start HTTP server")?;
    info!("HTTP server starting on port 8080");

    // POST /mcp — Streamable HTTP JSON-RPC requests
    // All responses are streamed directly to the HTTP writer — no intermediate
    // Value tree or String allocation (prevents OOM on ESP32-S2).
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

        // Stream the response directly to the HTTP writer
        let resp = request.into_response(200, Some("OK"), &[
            ("Content-Type", "application/json"),
            ("Access-Control-Allow-Origin", "*"),
        ])?;
        let mut writer = StdIoWriter(resp);

        match server_post.handle_request_streaming(body_str, &mut writer) {
            Ok(_) => {} // true = response written, false = notification (empty 200 body is fine)
            Err(e) => {
                log::error!("Streaming error: {}", e);
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

    // GET /openapi.json — dynamic OpenAPI spec for tool discovery
    let server_openapi = server.clone();
    http.fn_handler::<anyhow::Error, _>("/openapi.json", Method::Get, move |request| {
        let tools = server_openapi.list_tool_definitions();
        let resp = request.into_response(
            200,
            Some("OK"),
            &[
                ("Content-Type", "application/json"),
                ("Access-Control-Allow-Origin", "*"),
            ],
        )?;
        let mut writer = StdIoWriter(resp);
        write_openapi_spec(&mut writer, &tools)?;
        Ok(())
    })
    .map_err(|e| anyhow::anyhow!("Failed to register GET /openapi.json: {e}"))?;

    // OPTIONS /mcp — CORS preflight
    http.fn_handler::<anyhow::Error, _>("/mcp", Method::Options, |request| {
        request.into_response(204, Some("No Content"), CORS_HEADERS)?;
        Ok(())
    })
    .map_err(|e| anyhow::anyhow!("Failed to register OPTIONS /mcp: {e}"))?;

    // OPTIONS /openapi.json — CORS preflight
    http.fn_handler::<anyhow::Error, _>("/openapi.json", Method::Options, |request| {
        request.into_response(204, Some("No Content"), CORS_HEADERS)?;
        Ok(())
    })
    .map_err(|e| anyhow::anyhow!("Failed to register OPTIONS /openapi.json: {e}"))?;

    // Legacy SSE handlers: GET /sse and POST /messages
    register_sse_handlers(&mut http, server, sse_state)?;

    info!("HTTP server ready — POST /mcp, GET /health, GET /openapi.json, GET /sse, POST /messages");
    Ok(http)
}

/// Write an OpenAPI 3.1 spec to a `std::io::Write` stream, serializing one tool
/// at a time to avoid allocating the entire spec in memory (~20KB for 30 tools).
fn write_openapi_spec(w: &mut impl std::io::Write, tools: &[ToolDefinition]) -> Result<()> {
    // --- Header ---
    w.write_all(concat!(
        r#"{"openapi":"3.1.0","info":{"title":"Flipper MCP Server","#,
        r#""description":"Model Context Protocol server on Flipper Zero WiFi Dev Board (ESP32-S2). "#,
        r#"Exposes Flipper Zero hardware as MCP tools.","#,
        r#""version":""#,
    ).as_bytes())?;
    w.write_all(env!("CARGO_PKG_VERSION").as_bytes())?;
    w.write_all(b"\"},")?;

    // --- Paths ---
    w.write_all(concat!(
        r#""paths":{"#,
        // /health
        r#""/health":{"get":{"operationId":"healthCheck","summary":"Health check","#,
        r#""responses":{"200":{"description":"Server is running","content":{"application/json":{"#,
        r#""schema":{"type":"object","properties":{"status":{"type":"string"},"version":{"type":"string"}}}}}}}}},"#,
        // /mcp
        r#""/mcp":{"post":{"operationId":"mcpJsonRpc","#,
        r#""summary":"MCP JSON-RPC 2.0 endpoint (Streamable HTTP transport)","#,
        r#""description":"Send JSON-RPC 2.0 requests. Methods: initialize, tools/list, tools/call, resources/list","#,
        r#""requestBody":{"required":true,"content":{"application/json":{"schema":{"type":"object","#,
        r#""required":["jsonrpc","method"],"properties":{"jsonrpc":{"type":"string","const":"2.0"},"#,
        r#""id":{},"method":{"type":"string","enum":["initialize","tools/list","tools/call","resources/list"]},"#,
        r#""params":{"type":"object"}}}}}},"#,
        r#""responses":{"200":{"description":"JSON-RPC response"},"202":{"description":"Notification accepted"}}}},"#,
        // /openapi.json
        r#""/openapi.json":{"get":{"operationId":"openApiSpec","summary":"OpenAPI specification (this document)","#,
        r#""responses":{"200":{"description":"OpenAPI 3.1 JSON"}}}}},"#,
    ).as_bytes())?;

    // --- x-mcp-tools: stream one tool definition at a time ---
    w.write_all(b"\"x-mcp-tools\":[")?;
    for (i, tool) in tools.iter().enumerate() {
        if i > 0 {
            w.write_all(b",")?;
        }
        serde_json::to_writer(&mut *w, tool)
            .map_err(|e| anyhow::anyhow!("tool serialization: {e}"))?;
    }
    w.write_all(b"]}")?;

    Ok(())
}
