use std::sync::{Arc, Mutex};

use log::{info, warn};
use serde_json::{json, Value};

use crate::uart::FlipperProtocol;

use super::jsonrpc::{
    self, error_response, success_response, JsonRpcRequest, JsonRpcResponse, INTERNAL_ERROR,
    INVALID_PARAMS, METHOD_NOT_FOUND, PARSE_ERROR,
};
use super::tools::ToolRegistry;

pub struct McpServer {
    tools: ToolRegistry,
}

impl McpServer {
    pub fn new(protocol: Arc<Mutex<dyn FlipperProtocol>>) -> Self {
        Self {
            tools: ToolRegistry::new(protocol),
        }
    }

    /// Handle a JSON-RPC request body. Returns None for notifications (202 response),
    /// Some(json) for requests that need a response.
    pub fn handle_request(&self, body: &str) -> Option<String> {
        let request: JsonRpcRequest = match serde_json::from_str(body) {
            Ok(req) => req,
            Err(e) => {
                warn!("Failed to parse JSON-RPC request: {}", e);
                let resp = error_response(Value::Null, PARSE_ERROR, format!("Parse error: {}", e));
                return Some(serde_json::to_string(&resp).unwrap_or_default());
            }
        };

        // Notifications have no id — return None to signal 202 Accepted
        let id = match request.id {
            Some(id) => id,
            None => {
                info!("Received notification: {}", request.method);
                return None;
            }
        };

        let response = self.dispatch(id, &request.method, &request.params);
        Some(serde_json::to_string(&response).unwrap_or_default())
    }

    fn dispatch(&self, id: Value, method: &str, params: &Option<Value>) -> JsonRpcResponse {
        info!("MCP request: {} (id={})", method, id);

        match method {
            "initialize" => self.handle_initialize(id, params),
            "tools/list" => self.handle_tools_list(id),
            "tools/call" => self.handle_tools_call(id, params),
            "resources/list" => self.handle_resources_list(id),
            "resources/read" => error_response(id, jsonrpc::INTERNAL_ERROR, "Resource not found"),
            _ => {
                warn!("Unknown method: {}", method);
                error_response(id, METHOD_NOT_FOUND, format!("Method not found: {}", method))
            }
        }
    }

    fn handle_initialize(&self, id: Value, _params: &Option<Value>) -> JsonRpcResponse {
        info!("MCP initialize — capability negotiation");
        success_response(
            id,
            json!({
                "protocolVersion": "2025-03-26",
                "capabilities": {
                    "tools": {},
                    "resources": {}
                },
                "serverInfo": {
                    "name": "flipper-mcp",
                    "version": env!("CARGO_PKG_VERSION")
                }
            }),
        )
    }

    fn handle_tools_list(&self, id: Value) -> JsonRpcResponse {
        success_response(id, self.tools.list_tools())
    }

    fn handle_tools_call(&self, id: Value, params: &Option<Value>) -> JsonRpcResponse {
        let params = match params {
            Some(p) => p,
            None => return error_response(id, INVALID_PARAMS, "Missing params"),
        };

        let tool_name = match params.get("name").and_then(|v| v.as_str()) {
            Some(name) => name,
            None => return error_response(id, INVALID_PARAMS, "Missing tool name"),
        };

        let arguments = params.get("arguments").cloned().unwrap_or(json!({}));

        info!("Calling tool: {}", tool_name);
        let result = self.tools.call_tool(tool_name, &arguments);

        match serde_json::to_value(&result) {
            Ok(val) => success_response(id, val),
            Err(e) => error_response(id, INTERNAL_ERROR, format!("Serialization error: {}", e)),
        }
    }

    fn handle_resources_list(&self, id: Value) -> JsonRpcResponse {
        success_response(id, json!({ "resources": [] }))
    }
}
