//! API key validation and model discovery for LLM providers.

use serde::{Deserialize, Serialize};
use std::time::Duration;

/// Information about a model available from a provider.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ModelInfo {
    /// Model identifier (e.g. "gpt-4.1", "claude-sonnet-4-6").
    pub id: String,
    /// Human-readable name, if available.
    pub display_name: Option<String>,
    /// Unix timestamp of model creation, if available.
    pub created: Option<i64>,
}

/// Validate an API key for a given provider by making a test request.
/// Returns Ok(()) if valid, Err with message if invalid.
pub async fn validate_api_key(provider: &str, key: &str) -> anyhow::Result<()> {
    match provider {
        "openai" => validate_openai(key).await,
        "anthropic" => validate_anthropic(key).await,
        "perplexity" => validate_perplexity(key).await,
        "xai" => validate_xai(key).await,
        "google" => validate_google(key).await,
        "tavily" => validate_tavily(key).await,
        _ => Ok(()), // Unknown providers: accept without validation
    }
}

async fn validate_openai(key: &str) -> anyhow::Result<()> {
    let client = reqwest::Client::builder()
        .timeout(Duration::from_secs(10))
        .build()?;

    let response = client
        .get("https://api.openai.com/v1/models")
        .header("Authorization", format!("Bearer {}", key))
        .send()
        .await?;

    match response.status().as_u16() {
        200 => Ok(()),
        401 => Err(anyhow::anyhow!("Invalid API key")),
        429 => Ok(()), // Rate limited but key is valid
        _ => Err(anyhow::anyhow!("API error: {}", response.status())),
    }
}

async fn validate_anthropic(key: &str) -> anyhow::Result<()> {
    let client = reqwest::Client::builder()
        .timeout(Duration::from_secs(10))
        .build()?;

    // Use minimal message request to test key
    let response = client
        .post("https://api.anthropic.com/v1/messages")
        .header("x-api-key", key)
        .header("anthropic-version", "2023-06-01")
        .header("content-type", "application/json")
        .body(
            r#"{"model":"claude-sonnet-4-6","max_tokens":1,"messages":[{"role":"user","content":"hi"}]}"#,
        )
        .send()
        .await?;

    match response.status().as_u16() {
        200 => Ok(()),
        401 => Err(anyhow::anyhow!("Invalid API key")),
        403 => Err(anyhow::anyhow!(
            "API key lacks permission (billing or access issue)"
        )),
        429 | 529 => Ok(()), // Rate limited / overloaded but key is valid
        400 => {
            // Parse body to distinguish auth-adjacent errors from model issues
            let body = response.text().await.unwrap_or_default();
            if body.contains("invalid x-api-key") || body.contains("invalid api key") {
                Err(anyhow::anyhow!("Invalid API key"))
            } else {
                Err(anyhow::anyhow!("Anthropic API error (400): {}", body))
            }
        }
        status => Err(anyhow::anyhow!("API error: {}", status)),
    }
}

async fn validate_perplexity(key: &str) -> anyhow::Result<()> {
    let client = reqwest::Client::builder()
        .timeout(Duration::from_secs(10))
        .build()?;

    // Perplexity has no /models endpoint; use a minimal chat completion request
    let response = client
        .post("https://api.perplexity.ai/chat/completions")
        .header("Authorization", format!("Bearer {}", key))
        .header("content-type", "application/json")
        .body(r#"{"model":"sonar","max_tokens":1,"messages":[{"role":"user","content":"hi"}]}"#)
        .send()
        .await?;

    match response.status().as_u16() {
        200 => Ok(()),
        401 => Err(anyhow::anyhow!("Invalid API key")),
        403 => Err(anyhow::anyhow!("API key lacks permission")),
        429 => Ok(()), // Rate limited but key is valid
        _ => Err(anyhow::anyhow!("API error: {}", response.status())),
    }
}

async fn validate_xai(key: &str) -> anyhow::Result<()> {
    let client = reqwest::Client::builder()
        .timeout(Duration::from_secs(10))
        .build()?;

    let response = client
        .get("https://api.x.ai/v1/models")
        .header("Authorization", format!("Bearer {}", key))
        .send()
        .await?;

    match response.status().as_u16() {
        200 => Ok(()),
        401 => Err(anyhow::anyhow!("Invalid API key")),
        _ => Err(anyhow::anyhow!("API error: {}", response.status())),
    }
}

async fn validate_google(key: &str) -> anyhow::Result<()> {
    let client = reqwest::Client::builder()
        .timeout(Duration::from_secs(10))
        .build()?;

    let response = client
        .get(format!(
            "https://generativelanguage.googleapis.com/v1/models?key={}",
            key
        ))
        .send()
        .await?;

    match response.status().as_u16() {
        200 => Ok(()),
        400 | 401 | 403 => Err(anyhow::anyhow!("Invalid API key")),
        _ => Err(anyhow::anyhow!("API error: {}", response.status())),
    }
}

async fn validate_tavily(key: &str) -> anyhow::Result<()> {
    let client = reqwest::Client::builder()
        .timeout(Duration::from_secs(10))
        .build()?;

    // Tavily doesn't have a simple auth check endpoint, so we do a minimal search
    let response = client
        .post("https://api.tavily.com/search")
        .header("content-type", "application/json")
        .body(format!(
            r#"{{"api_key":"{}","query":"test","max_results":1}}"#,
            key
        ))
        .send()
        .await?;

    match response.status().as_u16() {
        200 => Ok(()),
        401 | 403 => Err(anyhow::anyhow!("Invalid API key")),
        429 => Ok(()), // Rate limited but key is valid
        _ => Err(anyhow::anyhow!("API error: {}", response.status())),
    }
}

// ---------------------------------------------------------------------------
// Model discovery
// ---------------------------------------------------------------------------

/// Discover available models from a provider using its API key.
/// Returns a list of models on success, or an error if discovery fails.
pub async fn discover_models(provider: &str, api_key: &str) -> Result<Vec<ModelInfo>, String> {
    match provider {
        "openai" => discover_openai_models(api_key).await,
        "xai" => discover_xai_models(api_key).await,
        "google" => discover_google_models(api_key).await,
        "anthropic" => Ok(curated_anthropic_models()),
        "perplexity" => Ok(curated_perplexity_models()),
        _ => Ok(vec![]), // Unknown providers: no discovery
    }
}

/// Return true when a model ID belongs to the given provider's chat-capable set.
///
/// Unknown providers default to `true` so newer providers are not blocked by
/// stale validation logic, but incompatible known-provider overrides can be
/// stripped before they reach the wrong API.
pub fn is_model_compatible_with_provider(provider: &str, model: &str) -> bool {
    let provider = provider.trim().to_lowercase();
    let model = model.trim();
    if model.is_empty() {
        return false;
    }

    match provider.as_str() {
        "openai" => is_chat_openai_model(model),
        "google" => is_chat_google_model(model),
        "xai" => is_chat_xai_model(model),
        "anthropic" => model.starts_with("claude-"),
        "perplexity" => model.starts_with("sonar"),
        "claude-cli" | "gemini-cli" | "codex-cli" | "grok-cli" => false,
        _ => true,
    }
}

/// Whitelist of OpenAI model prefixes that are chat-completion capable.
/// Everything else (embeddings, TTS, DALL-E, Sora, Whisper, moderation,
/// realtime, audio, search-preview, instruct, legacy completions) is excluded.
fn is_chat_openai_model(id: &str) -> bool {
    const CHAT_PREFIXES: &[&str] = &[
        "gpt-4.1",
        "gpt-4.5",
        "gpt-5",
        "gpt-4o",
        "gpt-4-turbo",
        "gpt-4-",
        "gpt-4",
        "o1",
        "o3",
        "o4",
        "chatgpt-4o-latest",
    ];
    const BLOCKED_SUFFIXES: &[&str] = &["-instruct", "-search-preview", "-realtime", "-audio"];
    if BLOCKED_SUFFIXES.iter().any(|s| id.contains(s)) {
        return false;
    }
    CHAT_PREFIXES.iter().any(|prefix| id.starts_with(prefix))
}

/// Whitelist of Google model prefixes that are chat capable.
fn is_chat_google_model(id: &str) -> bool {
    const CHAT_PREFIXES: &[&str] = &["gemini-2", "gemini-1.5-pro", "gemini-1.5-flash"];
    const BLOCKED: &[&str] = &["embedding", "aqa", "imagen", "veo", "chirp"];
    if BLOCKED.iter().any(|b| id.contains(b)) {
        return false;
    }
    CHAT_PREFIXES.iter().any(|prefix| id.starts_with(prefix))
}

/// Whitelist of xAI model prefixes that are chat capable.
fn is_chat_xai_model(id: &str) -> bool {
    const CHAT_PREFIXES: &[&str] = &["grok-"];
    const BLOCKED: &[&str] = &["embedding", "image"];
    if BLOCKED.iter().any(|b| id.contains(b)) {
        return false;
    }
    CHAT_PREFIXES.iter().any(|prefix| id.starts_with(prefix))
}

/// OpenAI: GET /v1/models → parse data[].id, filtering out non-chat models
async fn discover_openai_models(key: &str) -> Result<Vec<ModelInfo>, String> {
    let client = reqwest::Client::builder()
        .timeout(Duration::from_secs(15))
        .build()
        .map_err(|e| e.to_string())?;

    let response = client
        .get("https://api.openai.com/v1/models")
        .header("Authorization", format!("Bearer {}", key))
        .send()
        .await
        .map_err(|e| format!("OpenAI model discovery request failed: {}", e))?;

    if response.status().as_u16() != 200 {
        return Err(format!(
            "OpenAI model discovery failed with status {}",
            response.status()
        ));
    }

    let body: serde_json::Value = response
        .json()
        .await
        .map_err(|e| format!("Failed to parse OpenAI models response: {}", e))?;

    let models = body["data"]
        .as_array()
        .map(|arr| {
            arr.iter()
                .filter_map(|m| {
                    let id = m["id"].as_str()?.to_string();
                    if !is_chat_openai_model(&id) {
                        return None;
                    }
                    let created = m["created"].as_i64();
                    Some(ModelInfo {
                        display_name: None,
                        id,
                        created,
                    })
                })
                .collect()
        })
        .unwrap_or_default();

    Ok(models)
}

/// xAI: GET /v1/models → OpenAI-compatible format
async fn discover_xai_models(key: &str) -> Result<Vec<ModelInfo>, String> {
    let client = reqwest::Client::builder()
        .timeout(Duration::from_secs(15))
        .build()
        .map_err(|e| e.to_string())?;

    let response = client
        .get("https://api.x.ai/v1/models")
        .header("Authorization", format!("Bearer {}", key))
        .send()
        .await
        .map_err(|e| format!("xAI model discovery request failed: {}", e))?;

    if response.status().as_u16() != 200 {
        return Err(format!(
            "xAI model discovery failed with status {}",
            response.status()
        ));
    }

    let body: serde_json::Value = response
        .json()
        .await
        .map_err(|e| format!("Failed to parse xAI models response: {}", e))?;

    let models = body["data"]
        .as_array()
        .map(|arr| {
            arr.iter()
                .filter_map(|m| {
                    let id = m["id"].as_str()?.to_string();
                    if !is_chat_xai_model(&id) {
                        return None;
                    }
                    let created = m["created"].as_i64();
                    Some(ModelInfo {
                        display_name: None,
                        id,
                        created,
                    })
                })
                .collect()
        })
        .unwrap_or_default();

    Ok(models)
}

/// Google: GET /v1/models?key=KEY → parse models[].name
async fn discover_google_models(key: &str) -> Result<Vec<ModelInfo>, String> {
    let client = reqwest::Client::builder()
        .timeout(Duration::from_secs(15))
        .build()
        .map_err(|e| e.to_string())?;

    let response = client
        .get(format!(
            "https://generativelanguage.googleapis.com/v1/models?key={}",
            key
        ))
        .send()
        .await
        .map_err(|e| format!("Google model discovery request failed: {}", e))?;

    if response.status().as_u16() != 200 {
        return Err(format!(
            "Google model discovery failed with status {}",
            response.status()
        ));
    }

    let body: serde_json::Value = response
        .json()
        .await
        .map_err(|e| format!("Failed to parse Google models response: {}", e))?;

    let models = body["models"]
        .as_array()
        .map(|arr| {
            arr.iter()
                .filter_map(|m| {
                    // Google format: "models/gemini-2.0-flash" → strip prefix
                    let raw_name = m["name"].as_str()?;
                    let id = raw_name.strip_prefix("models/").unwrap_or(raw_name);
                    if !is_chat_google_model(id) {
                        return None;
                    }
                    let display = m["displayName"].as_str().map(|s| s.to_string());
                    Some(ModelInfo {
                        id: id.to_string(),
                        display_name: display,
                        created: None,
                    })
                })
                .collect()
        })
        .unwrap_or_default();

    Ok(models)
}

/// Anthropic: no listing API — return curated models (Feb 2026).
fn curated_anthropic_models() -> Vec<ModelInfo> {
    vec![
        ModelInfo {
            id: "claude-sonnet-4-6".to_string(),
            display_name: Some("Claude Sonnet 4.6".to_string()),
            created: None,
        },
        ModelInfo {
            id: "claude-haiku-4-5".to_string(),
            display_name: Some("Claude Haiku 4.5".to_string()),
            created: None,
        },
        ModelInfo {
            id: "claude-opus-4-6".to_string(),
            display_name: Some("Claude Opus 4.6".to_string()),
            created: None,
        },
    ]
}

/// Perplexity: no listing API — return curated models.
fn curated_perplexity_models() -> Vec<ModelInfo> {
    vec![
        ModelInfo {
            id: "sonar".to_string(),
            display_name: Some("Sonar".to_string()),
            created: None,
        },
        ModelInfo {
            id: "sonar-pro".to_string(),
            display_name: Some("Sonar Pro".to_string()),
            created: None,
        },
        ModelInfo {
            id: "sonar-reasoning".to_string(),
            display_name: Some("Sonar Reasoning".to_string()),
            created: None,
        },
        ModelInfo {
            id: "sonar-reasoning-pro".to_string(),
            display_name: Some("Sonar Reasoning Pro".to_string()),
            created: None,
        },
    ]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_validate_unknown_provider() {
        // Unknown providers should always pass (no validation)
        let result = validate_api_key("unknown_provider", "any-key").await;
        assert!(result.is_ok());
    }

    #[tokio::test]
    async fn test_validate_openai_invalid() {
        // Test with obviously invalid key
        let result = validate_api_key("openai", "invalid-key").await;
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn test_validate_anthropic_invalid() {
        let result = validate_api_key("anthropic", "invalid-key").await;
        assert!(result.is_err());
    }

    #[test]
    fn test_curated_anthropic_models() {
        let models = curated_anthropic_models();
        assert!(!models.is_empty());
        assert!(models.iter().any(|m| m.id.contains("claude")));
    }

    #[test]
    fn test_curated_perplexity_models() {
        let models = curated_perplexity_models();
        assert!(!models.is_empty());
        assert!(models.iter().any(|m| m.id == "sonar"));
    }

    #[tokio::test]
    async fn test_discover_unknown_provider() {
        let result = discover_models("unknown", "any-key").await;
        assert!(result.is_ok());
        assert!(result.unwrap().is_empty());
    }

    #[tokio::test]
    async fn test_discover_anthropic_curated() {
        let result = discover_models("anthropic", "any-key").await;
        assert!(result.is_ok());
        let models = result.unwrap();
        assert!(models.len() >= 2);
    }

    #[tokio::test]
    async fn test_discover_perplexity_curated() {
        let result = discover_models("perplexity", "any-key").await;
        assert!(result.is_ok());
        let models = result.unwrap();
        assert!(models.len() >= 3);
    }

    #[test]
    fn test_openai_chat_model_whitelist() {
        assert!(is_chat_openai_model("gpt-4.1"));
        assert!(is_chat_openai_model("gpt-4.1-mini"));
        assert!(is_chat_openai_model("gpt-4.5-preview"));
        assert!(is_chat_openai_model("gpt-5"));
        assert!(is_chat_openai_model("gpt-5.2"));
        assert!(is_chat_openai_model("gpt-4o"));
        assert!(is_chat_openai_model("gpt-4o-mini"));
        assert!(is_chat_openai_model("gpt-4-turbo"));
        assert!(is_chat_openai_model("gpt-4-0613"));
        assert!(is_chat_openai_model("o1-preview"));
        assert!(is_chat_openai_model("o3-mini"));
        assert!(is_chat_openai_model("o4-mini"));
        assert!(is_chat_openai_model("chatgpt-4o-latest"));

        assert!(!is_chat_openai_model("text-embedding-3-small"));
        assert!(!is_chat_openai_model("tts-1"));
        assert!(!is_chat_openai_model("dall-e-3"));
        assert!(!is_chat_openai_model("whisper-1"));
        assert!(!is_chat_openai_model("text-moderation-latest"));
        assert!(!is_chat_openai_model("babbage-002"));
        assert!(!is_chat_openai_model("davinci-002"));
        assert!(!is_chat_openai_model("omni-moderation-latest"));
        assert!(!is_chat_openai_model("gpt-3.5-turbo-instruct"));
        assert!(!is_chat_openai_model("gpt-4o-realtime-preview"));
        assert!(!is_chat_openai_model("gpt-4o-audio-preview"));
        assert!(!is_chat_openai_model("gpt-4o-search-preview"));
        assert!(!is_chat_openai_model("sora-2025"));
        assert!(!is_chat_openai_model("canary-tts"));
    }

    #[test]
    fn test_google_chat_model_whitelist() {
        assert!(is_chat_google_model("gemini-2.0-flash"));
        assert!(is_chat_google_model("gemini-2.5-pro"));
        assert!(is_chat_google_model("gemini-2.5-flash"));
        assert!(is_chat_google_model("gemini-1.5-pro"));
        assert!(is_chat_google_model("gemini-1.5-flash"));

        assert!(!is_chat_google_model("text-embedding-004"));
        assert!(!is_chat_google_model("embedding-001"));
        assert!(!is_chat_google_model("aqa"));
        assert!(!is_chat_google_model("imagen-3.0"));
    }

    #[test]
    fn test_xai_chat_model_whitelist() {
        assert!(is_chat_xai_model("grok-3"));
        assert!(is_chat_xai_model("grok-4-1-fast-reasoning"));

        assert!(!is_chat_xai_model("some-embedding-model"));
        assert!(!is_chat_xai_model("grok-image-gen"));
    }

    #[test]
    fn test_model_provider_compatibility_guard() {
        assert!(is_model_compatible_with_provider("openai", "gpt-4.1"));
        assert!(is_model_compatible_with_provider(
            "google",
            "gemini-2.5-pro"
        ));
        assert!(is_model_compatible_with_provider(
            "xai",
            "grok-4-1-fast-reasoning"
        ));
        assert!(is_model_compatible_with_provider(
            "anthropic",
            "claude-sonnet-4-6"
        ));
        assert!(is_model_compatible_with_provider("perplexity", "sonar-pro"));

        assert!(!is_model_compatible_with_provider(
            "openai",
            "gemini-2.5-pro"
        ));
        assert!(!is_model_compatible_with_provider("google", "gpt-4.1"));
        assert!(!is_model_compatible_with_provider("xai", "gpt-4.1"));
        assert!(!is_model_compatible_with_provider(
            "claude-cli",
            "claude-sonnet-4-6"
        ));
        assert!(is_model_compatible_with_provider(
            "openrouter",
            "google/gemini-2.5-pro"
        ));
    }
}
