use crate::cognitive::provider::{
    CompletionRequest, CompletionResponse, LlmProvider, ToolCall,
};
use async_openai::config::OpenAIConfig;
use async_openai::types::{
    ChatCompletionMessageToolCall, ChatCompletionRequestAssistantMessage,
    ChatCompletionRequestMessage, ChatCompletionRequestSystemMessage,
    ChatCompletionRequestToolMessage, ChatCompletionRequestUserMessage,
    ChatCompletionTool, ChatCompletionToolType, CreateChatCompletionRequest, FunctionCall,
    FunctionObject,
};
use async_trait::async_trait;

pub struct OpenAiProvider {
    client: async_openai::Client<OpenAIConfig>,
}

impl OpenAiProvider {
    pub fn new(api_key: Option<String>) -> Self {
        let key = api_key
            .or_else(|| std::env::var("OPENAI_API_KEY").ok())
            .unwrap_or_default();
        let config = OpenAIConfig::new().with_api_key(key);
        let client = async_openai::Client::with_config(config);
        Self { client }
    }
}

/// Map our Message role string to the correct async_openai variant.
fn map_message(m: &crate::cognitive::provider::Message) -> ChatCompletionRequestMessage {
    match m.role.as_str() {
        "system" => ChatCompletionRequestMessage::System(ChatCompletionRequestSystemMessage {
            content: m.content.clone(),
            ..Default::default()
        }),
        "assistant" => {
            // If the assistant message carried tool_calls, map them too.
            let tool_calls = m.tool_calls.as_ref().map(|tcs| {
                tcs.iter()
                    .map(|tc| ChatCompletionMessageToolCall {
                        id: tc.id.clone(),
                        r#type: ChatCompletionToolType::Function,
                        function: FunctionCall {
                            name: tc.name.clone(),
                            arguments: tc.arguments.clone(),
                        },
                    })
                    .collect()
            });
            ChatCompletionRequestMessage::Assistant(ChatCompletionRequestAssistantMessage {
                content: Some(m.content.clone()),
                tool_calls,
                ..Default::default()
            })
        }
        "tool" => ChatCompletionRequestMessage::Tool(ChatCompletionRequestToolMessage {
            content: m.content.clone(),
            tool_call_id: m.tool_call_id.clone().unwrap_or_default(),
            ..Default::default()
        }),
        _ => ChatCompletionRequestMessage::User(ChatCompletionRequestUserMessage {
            content: m.content.clone().into(),
            ..Default::default()
        }),
    }
}

#[async_trait]
impl LlmProvider for OpenAiProvider {
    async fn complete(&self, request: &CompletionRequest) -> anyhow::Result<CompletionResponse> {
        let messages: Vec<ChatCompletionRequestMessage> =
            request.messages.iter().map(map_message).collect();

        // Map tool definitions if provided.
        let tools: Option<Vec<ChatCompletionTool>> = request.tools.as_ref().map(|defs| {
            defs.iter()
                .map(|td| ChatCompletionTool {
                    r#type: ChatCompletionToolType::Function,
                    function: FunctionObject {
                        name: td.name.clone(),
                        description: Some(td.description.clone()),
                        parameters: Some(td.parameters.clone()),
                    },
                })
                .collect()
        });

        let req = CreateChatCompletionRequest {
            model: "gpt-4o-mini".to_string(),
            messages,
            tools,
            ..Default::default()
        };

        let response = self.client.chat().create(req).await?;
        let choice = response.choices.first();

        let content = choice
            .and_then(|c| c.message.content.clone())
            .unwrap_or_default();

        // Extract tool calls from the response.
        let tool_calls = choice
            .and_then(|c| c.message.tool_calls.as_ref())
            .map(|tcs| {
                tcs.iter()
                    .map(|tc| ToolCall {
                        id: tc.id.clone(),
                        name: tc.function.name.clone(),
                        arguments: tc.function.arguments.clone(),
                    })
                    .collect::<Vec<_>>()
            })
            .filter(|v| !v.is_empty());

        Ok(CompletionResponse {
            content,
            tool_calls,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::cognitive::provider::{CompletionRequest, Message};

    #[tokio::test]
    async fn test_openai_provider() {
        if std::env::var("OPENAI_API_KEY").is_err() {
            return;
        }
        let key = std::env::var("OPENAI_API_KEY").unwrap();
        let provider = OpenAiProvider::new(Some(key));
        let request = CompletionRequest::simple(vec![Message::new("user", "Say hello in one word.")]);
        let response = provider.complete(&request).await.unwrap();
        assert!(!response.content.is_empty());
    }
}
