use std::sync::{Arc, Mutex};

use log::{info, warn};
use serde_json::{json, Value};

use crate::log_buffer::LogBuffer;
use crate::uart::FlipperProtocol;

use super::jsonrpc::{self, JsonRpcRequest, INVALID_PARAMS, METHOD_NOT_FOUND, PARSE_ERROR};
use super::tools::ToolRegistry;

pub struct McpServer {
    tools: ToolRegistry,
    /// Shared log buffer — tool call results are pushed here so the
    /// Flipper FAP "View Logs" screen can show remote tool activity.
    log_buffer: Arc<LogBuffer>,
}

impl McpServer {
    pub fn new(protocol: Arc<Mutex<dyn FlipperProtocol>>, log_buffer: Arc<LogBuffer>) -> Self {
        Self {
            tools: ToolRegistry::new(protocol),
            log_buffer,
        }
    }

    /// Handle a JSON-RPC request body, streaming the response directly to a writer.
    ///
    /// Returns `Ok(true)` if a response was written, `Ok(false)` for notifications
    /// (caller should send 202 with no body).
    ///
    /// ALL serialization goes directly to the writer — no intermediate Value tree
    /// or String allocation. This keeps heap usage bounded regardless of response
    /// size (critical for ESP32-S2's ~320KB RAM).
    pub fn handle_request_streaming(
        &self,
        body: &str,
        w: &mut impl std::io::Write,
    ) -> std::io::Result<bool> {
        let request: JsonRpcRequest = match serde_json::from_str(body) {
            Ok(req) => req,
            Err(e) => {
                warn!("Failed to parse JSON-RPC request: {}", e);
                write_rpc_error(w, &Value::Null, PARSE_ERROR, &format!("Parse error: {}", e))?;
                return Ok(true);
            }
        };

        let id = match request.id {
            Some(id) => id,
            None => {
                info!("Received notification: {}", request.method);
                return Ok(false);
            }
        };

        info!("MCP request: {} (id={})", request.method, id);
        self.dispatch_streaming(w, &id, &request.method, &request.params)?;
        Ok(true)
    }

    /// Stream a JSON-RPC response for a parsed request directly to a writer.
    ///
    /// The caller is responsible for parsing the request and handling the
    /// notification case (no id). This method writes a complete JSON-RPC
    /// response to the writer for every dispatch path.
    pub fn dispatch_streaming(
        &self,
        w: &mut impl std::io::Write,
        id: &Value,
        method: &str,
        params: &Option<Value>,
    ) -> std::io::Result<()> {
        match method {
            "initialize" => {
                info!("MCP initialize — capability negotiation");
                write_rpc_result_start(w, id)?;
                w.write_all(
                    br#"{"protocolVersion":"2025-03-26","capabilities":{"tools":{},"resources":{}},"serverInfo":{"name":"flipper-mcp","version":""#,
                )?;
                w.write_all(env!("CARGO_PKG_VERSION").as_bytes())?;
                w.write_all(b"\"}}")?;
                w.write_all(b"}")?;
            }
            "tools/list" => {
                write_rpc_result_start(w, id)?;
                w.write_all(b"{\"tools\":[")?;
                let tools = self.tools.list_tool_definitions();
                for (i, tool) in tools.iter().enumerate() {
                    if i > 0 {
                        w.write_all(b",")?;
                    }
                    serde_json::to_writer(&mut *w, tool)
                        .map_err(|e| std::io::Error::new(std::io::ErrorKind::Other, e))?;
                }
                w.write_all(b"]}")?;
                w.write_all(b"}")?;
            }
            "tools/call" => {
                let params = match params {
                    Some(p) => p,
                    None => {
                        write_rpc_error(w, id, INVALID_PARAMS, "Missing params")?;
                        return Ok(());
                    }
                };

                let tool_name = match params.get("name").and_then(|v| v.as_str()) {
                    Some(name) => name,
                    None => {
                        write_rpc_error(w, id, INVALID_PARAMS, "Missing tool name")?;
                        return Ok(());
                    }
                };

                let arguments = params.get("arguments").cloned().unwrap_or(json!({}));

                info!("Calling tool: {}", tool_name);
                let result = self.tools.call_tool(tool_name, &arguments);

                // Push to log buffer so FAP "View Logs" shows remote tool activity
                self.log_buffer.push(&format!(
                    "[tool] {} {}",
                    tool_name,
                    if result.is_error { "ERR" } else { "OK" }
                ));

                write_rpc_result_start(w, id)?;
                serde_json::to_writer(&mut *w, &result)
                    .map_err(|e| std::io::Error::new(std::io::ErrorKind::Other, e))?;
                w.write_all(b"}")?;
            }
            "resources/list" => {
                write_rpc_result_start(w, id)?;
                w.write_all(b"{\"resources\":[]}")?;
                w.write_all(b"}")?;
            }
            "resources/read" => {
                write_rpc_error(w, id, jsonrpc::INTERNAL_ERROR, "Resource not found")?;
            }
            "modules/refresh" => {
                info!("Refreshing dynamic modules (FAP discovery + config reload)");
                self.tools.refresh_dynamic();
                write_rpc_result_start(w, id)?;
                w.write_all(b"{\"status\":\"refreshed\"}")?;
                w.write_all(b"}")?;
            }
            _ => {
                warn!("Unknown method: {}", method);
                write_rpc_error(
                    w,
                    id,
                    METHOD_NOT_FOUND,
                    &format!("Method not found: {}", method),
                )?;
            }
        }
        Ok(())
    }

    /// Return full tool definitions for OpenAPI spec generation.
    pub fn list_tool_definitions(&self) -> Vec<super::types::ToolDefinition> {
        self.tools.list_tool_definitions()
    }

    /// Refresh dynamic modules and return all tool names.
    /// Called from the main loop when "refresh_modules" command arrives over UART.
    pub fn refresh_and_list_tools(&self) -> Vec<String> {
        self.tools.refresh_dynamic();
        self.tools.list_tool_names()
    }

    /// Return all current tool names without refreshing.
    pub fn list_tool_names(&self) -> Vec<String> {
        self.tools.list_tool_names()
    }
}

/// Write `{"jsonrpc":"2.0","id":<id>,"result":` — caller writes result value then closing `}`.
fn write_rpc_result_start(w: &mut impl std::io::Write, id: &Value) -> std::io::Result<()> {
    w.write_all(b"{\"jsonrpc\":\"2.0\",\"id\":")?;
    serde_json::to_writer(&mut *w, id)
        .map_err(|e| std::io::Error::new(std::io::ErrorKind::Other, e))?;
    w.write_all(b",\"result\":")
}

/// Write a complete JSON-RPC error response.
pub fn write_rpc_error(
    w: &mut impl std::io::Write,
    id: &Value,
    code: i32,
    message: &str,
) -> std::io::Result<()> {
    w.write_all(b"{\"jsonrpc\":\"2.0\",\"id\":")?;
    serde_json::to_writer(&mut *w, id)
        .map_err(|e| std::io::Error::new(std::io::ErrorKind::Other, e))?;
    w.write_all(b",\"error\":{\"code\":")?;
    write!(w, "{}", code)?;
    w.write_all(b",\"message\":")?;
    // Use serde to properly escape the message string (quotes, backslashes, etc.)
    serde_json::to_writer(&mut *w, message)
        .map_err(|e| std::io::Error::new(std::io::ErrorKind::Other, e))?;
    w.write_all(b"}}")
}
