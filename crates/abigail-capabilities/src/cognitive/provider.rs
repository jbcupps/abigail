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
    /// Optional model override. When set, providers use this model ID instead of
    /// their configured default. Used by tier-based routing to select Fast/Standard/Pro
    /// models on a per-request basis without rebuilding the provider.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub model_override: Option<String>,
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

/// Events emitted during streaming.
#[derive(Debug, Clone, Serialize)]
pub enum StreamEvent {
    /// A chunk of text content (delta).
    Token(String),
    /// Stream is complete with the final assembled response.
    Done(CompletionResponse),
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
            model_override: None,
        }
    }

    /// Create a request with a specific model override.
    pub fn with_model(mut self, model: impl Into<String>) -> Self {
        self.model_override = Some(model.into());
        self
    }
}

#[async_trait]
pub trait LlmProvider: Send + Sync {
    /// Non-streaming completion. Returns the full response at once.
    async fn complete(&self, request: &CompletionRequest) -> anyhow::Result<CompletionResponse>;

    /// Streaming completion. Sends token events through the channel as they arrive.
    /// Returns the final assembled CompletionResponse.
    ///
    /// Default implementation falls back to non-streaming `complete()` and sends
    /// the full response as a single token event.
    async fn stream(
        &self,
        request: &CompletionRequest,
        tx: tokio::sync::mpsc::Sender<StreamEvent>,
    ) -> anyhow::Result<CompletionResponse> {
        let response = self.complete(request).await?;
        let _ = tx.send(StreamEvent::Token(response.content.clone())).await;
        let _ = tx.send(StreamEvent::Done(response.clone())).await;
        Ok(response)
    }
}

// ---------------------------------------------------------------------------
// Tool-name sanitization (shared across providers)
// ---------------------------------------------------------------------------

/// Sanitize a qualified tool name for provider APIs that require
/// `^[a-zA-Z0-9_-]+$` (OpenAI, Anthropic, etc.).
/// `::` → `__`, `.` → `_`, other invalid chars → `_`.
pub fn sanitize_tool_name(name: &str) -> String {
    name.replace("::", "__")
        .replace('.', "_")
        .chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || c == '_' || c == '-' {
                c
            } else {
                '_'
            }
        })
        .collect()
}

/// Build a reverse map from sanitized tool names back to their original
/// qualified names. Used to restore `skill_id::tool_name` after the API
/// returns the sanitized variant.
pub fn build_tool_name_map(tools: &[ToolDefinition]) -> std::collections::HashMap<String, String> {
    tools
        .iter()
        .map(|td| (sanitize_tool_name(&td.name), td.name.clone()))
        .collect()
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
        assert!(!json.contains("model_override"));
    }

    #[test]
    fn test_completion_request_model_override() {
        let req =
            CompletionRequest::simple(vec![Message::new("user", "hi")]).with_model("gpt-4.1-mini");
        assert_eq!(req.model_override.as_deref(), Some("gpt-4.1-mini"));
        let json = serde_json::to_string(&req).unwrap();
        assert!(json.contains("model_override"));
        assert!(json.contains("gpt-4.1-mini"));
    }

    #[test]
    fn test_completion_request_model_override_none_by_default() {
        let req = CompletionRequest::simple(vec![Message::new("user", "hi")]);
        assert!(req.model_override.is_none());
    }

    #[test]
    fn test_sanitize_tool_name_qualified() {
        assert_eq!(
            sanitize_tool_name("builtin.hive_management::store_secret"),
            "builtin_hive_management__store_secret"
        );
    }

    #[test]
    fn test_sanitize_tool_name_already_clean() {
        assert_eq!(sanitize_tool_name("my_tool-v2"), "my_tool-v2");
    }

    #[test]
    fn test_build_tool_name_map_roundtrip() {
        let tools = vec![ToolDefinition {
            name: "com.example::do_thing".into(),
            description: "test".into(),
            parameters: serde_json::json!({}),
        }];
        let map = build_tool_name_map(&tools);
        assert_eq!(
            map.get("com_example__do_thing"),
            Some(&"com.example::do_thing".to_string())
        );
    }
}
