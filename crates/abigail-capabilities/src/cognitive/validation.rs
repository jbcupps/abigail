//! API key validation for LLM providers.

use std::time::Duration;

/// Validate an API key for a given provider by making a test request.
/// Returns Ok(()) if valid, Err with message if invalid.
pub async fn validate_api_key(provider: &str, key: &str) -> anyhow::Result<()> {
    match provider {
        "openai" => validate_openai(key).await,
        "anthropic" => validate_anthropic(key).await,
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
        .body(r#"{"model":"claude-3-5-haiku-latest","max_tokens":1,"messages":[{"role":"user","content":"hi"}]}"#)
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
}
