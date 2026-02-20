use serde_json::Value;

use crate::mcp::types::{ToolDefinition, ToolResult};
use crate::uart::FlipperProtocol;

#[allow(dead_code)]
pub trait FlipperModule: Send + Sync {
    fn name(&self) -> &str;
    fn description(&self) -> &str;
    fn tools(&self) -> Vec<ToolDefinition>;
    fn execute(&self, tool: &str, args: &Value, protocol: &mut dyn FlipperProtocol) -> ToolResult;
}
