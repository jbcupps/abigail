use async_trait::async_trait;
use serde::{Deserialize, Serialize};

/// A tool the LLM can call (function-calling / tool-use).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolDefinition {
    pub name: String,
    pub description: String,
    pub parameters: serde_json::Value,
}

/// A tool call returned by the LLM.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolCall {
    pub id: String,
    pub name: String,
    /// JSON-encoded arguments string.
    pub arguments: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CompletionRequest {
    pub messages: Vec<Message>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tools: Option<Vec<ToolDefinition>>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Message {
    pub role: String,
    pub content: String,
    /// For tool result messages: the tool_call_id this is responding to.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tool_call_id: Option<String>,
    /// For assistant messages that invoked tools.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tool_calls: Option<Vec<ToolCall>>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CompletionResponse {
    pub content: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tool_calls: Option<Vec<ToolCall>>,
}

impl Message {
    /// Create a simple text message (no tool metadata).
    pub fn new(role: impl Into<String>, content: impl Into<String>) -> Self {
        Self {
            role: role.into(),
            content: content.into(),
            tool_call_id: None,
            tool_calls: None,
        }
    }

    /// Create a tool-result message.
    pub fn tool_result(tool_call_id: impl Into<String>, content: impl Into<String>) -> Self {
        Self {
            role: "tool".into(),
            content: content.into(),
            tool_call_id: Some(tool_call_id.into()),
            tool_calls: None,
        }
    }
}

impl CompletionRequest {
    /// Create a request with just messages (no tools).
    pub fn simple(messages: Vec<Message>) -> Self {
        Self {
            messages,
            tools: None,
        }
    }
}

#[async_trait]
pub trait LlmProvider: Send + Sync {
    async fn complete(&self, request: &CompletionRequest) -> anyhow::Result<CompletionResponse>;
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_tool_definition_serialization() {
        let td = ToolDefinition {
            name: "store_key".into(),
            description: "Store an API key".into(),
            parameters: serde_json::json!({"type": "object"}),
        };
        let json = serde_json::to_string(&td).unwrap();
        assert!(json.contains("store_key"));
        let round: ToolDefinition = serde_json::from_str(&json).unwrap();
        assert_eq!(round.name, "store_key");
    }

    #[test]
    fn test_tool_call_serialization() {
        let tc = ToolCall {
            id: "call_123".into(),
            name: "store_key".into(),
            arguments: r#"{"provider":"openai","key":"sk-test"}"#.into(),
        };
        let json = serde_json::to_string(&tc).unwrap();
        let round: ToolCall = serde_json::from_str(&json).unwrap();
        assert_eq!(round.id, "call_123");
        assert_eq!(round.name, "store_key");
    }

    #[test]
    fn test_message_new_omits_optional_fields() {
        let msg = Message::new("user", "hello");
        let json = serde_json::to_string(&msg).unwrap();
        assert!(!json.contains("tool_call_id"));
        assert!(!json.contains("tool_calls"));
    }

    #[test]
    fn test_message_tool_result() {
        let msg = Message::tool_result("call_456", "success");
        assert_eq!(msg.role, "tool");
        assert_eq!(msg.tool_call_id.as_deref(), Some("call_456"));
    }

    #[test]
    fn test_completion_request_simple_omits_tools() {
        let req = CompletionRequest::simple(vec![Message::new("user", "hi")]);
        let json = serde_json::to_string(&req).unwrap();
        assert!(!json.contains("tools"));
    }
}
