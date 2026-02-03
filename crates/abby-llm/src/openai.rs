use crate::provider::{CompletionRequest, CompletionResponse, LlmProvider};
use async_openai::config::OpenAIConfig;
use async_openai::types::{
    ChatCompletionRequestMessage, ChatCompletionRequestUserMessage, CreateChatCompletionRequest,
};
use async_trait::async_trait;

pub struct OpenAiProvider {
    client: async_openai::Client<OpenAIConfig>,
}

impl OpenAiProvider {
    pub fn new(api_key: Option<String>) -> Self {
        let key = api_key
            .or_else(|| std::env::var("OPENAI_API_KEY").ok())
            .unwrap_or_else(|| "".to_string());
        let config = OpenAIConfig::new().with_api_key(key);
        let client = async_openai::Client::with_config(config);
        Self { client }
    }
}

#[async_trait]
impl LlmProvider for OpenAiProvider {
    async fn complete(&self, request: &CompletionRequest) -> anyhow::Result<CompletionResponse> {
        let messages: Vec<ChatCompletionRequestMessage> = request
            .messages
            .iter()
            .map(|m| {
                ChatCompletionRequestMessage::User(ChatCompletionRequestUserMessage {
                    content: m.content.clone().into(),
                    ..Default::default()
                })
            })
            .collect();

        let req = CreateChatCompletionRequest {
            model: "gpt-4o-mini".to_string(),
            messages,
            ..Default::default()
        };

        let response = self.client.chat().create(req).await?;
        let content = response
            .choices
            .first()
            .and_then(|c| c.message.content.clone())
            .unwrap_or_default();

        Ok(CompletionResponse { content })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::provider::{CompletionRequest, Message};

    #[tokio::test]
    async fn test_openai_provider() {
        if std::env::var("OPENAI_API_KEY").is_err() {
            return;
        }
        let key = std::env::var("OPENAI_API_KEY").unwrap();
        let provider = OpenAiProvider::new(Some(key));
        let request = CompletionRequest {
            messages: vec![Message {
                role: "user".into(),
                content: "Say hello in one word.".into(),
            }],
        };
        let response = provider.complete(&request).await.unwrap();
        assert!(!response.content.is_empty());
    }
}
