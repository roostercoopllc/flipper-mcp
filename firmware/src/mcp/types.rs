use serde::Serialize;
use serde_json::Value;

#[derive(Debug, Clone, Serialize)]
pub struct ToolDefinition {
    pub name: String,
    pub description: String,
    #[serde(rename = "inputSchema")]
    pub input_schema: Value,
}

#[derive(Debug, Serialize)]
pub struct TextContent {
    pub r#type: &'static str,
    pub text: String,
}

impl TextContent {
    pub fn new(text: impl Into<String>) -> Self {
        Self {
            r#type: "text",
            text: text.into(),
        }
    }
}

#[derive(Debug, Serialize)]
pub struct ToolResult {
    pub content: Vec<TextContent>,
    #[serde(rename = "isError")]
    pub is_error: bool,
}

impl ToolResult {
    pub fn success(text: impl Into<String>) -> Self {
        Self {
            content: vec![TextContent::new(text)],
            is_error: false,
        }
    }

    pub fn error(text: impl Into<String>) -> Self {
        Self {
            content: vec![TextContent::new(text)],
            is_error: true,
        }
    }
}
