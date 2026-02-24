//! API key validation and model discovery for LLM providers.

use serde::{Deserialize, Serialize};
use std::time::Duration;

/// Information about a model available from a provider.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ModelInfo {
    /// Model identifier (e.g. "gpt-4o", "claude-sonnet-4-20250514").
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
            r#"{"model":"claude-sonnet-4-20250514","max_tokens":1,"messages":[{"role":"user","content":"hi"}]}"#,
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

/// OpenAI: GET /v1/models → parse data[].id
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

/// Anthropic: no listing API — return curated models.
fn curated_anthropic_models() -> Vec<ModelInfo> {
    vec![
        ModelInfo {
            id: "claude-sonnet-4-20250514".to_string(),
            display_name: Some("Claude Sonnet 4".to_string()),
            created: None,
        },
        ModelInfo {
            id: "claude-haiku-3-5-20241022".to_string(),
            display_name: Some("Claude 3.5 Haiku".to_string()),
            created: None,
        },
        ModelInfo {
            id: "claude-opus-4-20250514".to_string(),
            display_name: Some("Claude Opus 4".to_string()),
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
}
