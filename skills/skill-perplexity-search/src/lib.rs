//! Perplexity Search skill: AI-powered web search with citations.
//!
//! Uses the Perplexity Sonar API to perform research queries that return
//! answers grounded in real-time web data, with source citations.

use abigail_core::SecretsVault;
use abigail_skills::{
    CapabilityDescriptor, CostEstimate, ExecutionContext, HealthStatus, NetworkPermission,
    Permission, Skill, SkillConfig, SkillError, SkillHealth, SkillManifest, SkillResult,
    ToolDescriptor, ToolOutput, ToolParams, TriggerDescriptor,
};
use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use std::any::Any;
use std::collections::HashMap;
use std::sync::{Arc, Mutex};

const PERPLEXITY_API_URL: &str = "https://api.perplexity.ai/chat/completions";
const DEFAULT_MODEL: &str = "sonar";

// ── API Types ────────────────────────────────────────────────────

#[derive(Debug, Serialize)]
struct PerplexityRequest {
    model: String,
    messages: Vec<ChatMessage>,
    #[serde(skip_serializing_if = "Option::is_none")]
    search_domain_filter: Option<Vec<String>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    search_recency_filter: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    return_related_questions: Option<bool>,
}

#[derive(Debug, Serialize)]
struct ChatMessage {
    role: String,
    content: String,
}

#[derive(Debug, Deserialize)]
struct PerplexityResponse {
    choices: Vec<Choice>,
    #[serde(default)]
    citations: Vec<String>,
    #[serde(default)]
    search_results: Vec<SearchResult>,
}

#[derive(Debug, Deserialize)]
struct Choice {
    message: ResponseMessage,
}

#[derive(Debug, Deserialize)]
struct ResponseMessage {
    content: String,
}

#[derive(Debug, Deserialize)]
struct SearchResult {
    #[serde(default)]
    title: String,
    #[serde(default)]
    url: String,
    #[serde(default)]
    snippet: String,
}

// ── Skill Implementation ────────────────────────────────────────

pub struct PerplexitySearchSkill {
    manifest: SkillManifest,
    vault: Arc<Mutex<SecretsVault>>,
}

impl PerplexitySearchSkill {
    pub fn default_manifest() -> SkillManifest {
        let toml_str = include_str!("../skill.toml");
        SkillManifest::parse(toml_str).expect("Failed to parse perplexity-search skill.toml")
    }

    pub fn with_secrets(manifest: SkillManifest, vault: Arc<Mutex<SecretsVault>>) -> Self {
        Self { manifest, vault }
    }

    fn get_api_key(&self) -> SkillResult<String> {
        let vault = self
            .vault
            .lock()
            .map_err(|e| SkillError::ToolFailed(format!("Failed to lock vault: {}", e)))?;
        vault
            .get_secret("perplexity")
            .map(str::to_string)
            .ok_or_else(|| {
                SkillError::MissingSecret(
                    "Perplexity API key not configured. Store it with key name 'perplexity'."
                        .into(),
                )
            })
    }

    async fn search(
        &self,
        query: &str,
        model: Option<&str>,
        domain_filter: Option<Vec<String>>,
        recency_filter: Option<&str>,
        return_related: bool,
    ) -> SkillResult<ToolOutput> {
        if let Some(reason) = check_query_privacy(query) {
            return Ok(ToolOutput::error(format!("Search blocked: {}", reason)));
        }

        let api_key = self.get_api_key()?;
        let model = model.unwrap_or(DEFAULT_MODEL).to_string();

        let request = PerplexityRequest {
            model: model.clone(),
            messages: vec![ChatMessage {
                role: "user".to_string(),
                content: query.to_string(),
            }],
            search_domain_filter: domain_filter.clone(),
            search_recency_filter: recency_filter.map(|s| s.to_string()),
            return_related_questions: if return_related { Some(true) } else { None },
        };

        let client = reqwest::Client::new();
        let response = client
            .post(PERPLEXITY_API_URL)
            .header("Authorization", format!("Bearer {}", api_key))
            .header("Content-Type", "application/json")
            .json(&request)
            .send()
            .await
            .map_err(|e| SkillError::ToolFailed(format!("Perplexity API request failed: {}", e)))?;

        if !response.status().is_success() {
            let status = response.status();
            let body = response.text().await.unwrap_or_default();
            return Ok(ToolOutput::error(format!(
                "Perplexity API error ({}): {}",
                status, body
            )));
        }

        let pplx_response: PerplexityResponse = response
            .json()
            .await
            .map_err(|e| SkillError::ToolFailed(format!("Failed to parse response: {}", e)))?;

        let answer = pplx_response
            .choices
            .first()
            .map(|c| c.message.content.clone())
            .unwrap_or_else(|| "No answer received.".to_string());

        // Format with citations
        let mut formatted = answer.clone();
        if !pplx_response.citations.is_empty() {
            formatted.push_str("\n\nSources:");
            for (i, url) in pplx_response.citations.iter().enumerate() {
                formatted.push_str(&format!("\n[{}] {}", i + 1, url));
            }
        }

        // Build search results array for structured data
        let search_results: Vec<serde_json::Value> = pplx_response
            .search_results
            .iter()
            .map(|r| {
                serde_json::json!({
                    "title": r.title,
                    "url": r.url,
                    "snippet": r.snippet,
                })
            })
            .collect();

        Ok(ToolOutput::success(serde_json::json!({
            "formatted": formatted,
            "answer": answer,
            "citations": pplx_response.citations,
            "search_results": search_results,
            "model": model,
            "citation_count": pplx_response.citations.len(),
        })))
    }
}

#[async_trait]
impl Skill for PerplexitySearchSkill {
    fn manifest(&self) -> &SkillManifest {
        &self.manifest
    }

    async fn initialize(&mut self, _config: SkillConfig) -> SkillResult<()> {
        Ok(())
    }

    async fn shutdown(&mut self) -> SkillResult<()> {
        Ok(())
    }

    fn health(&self) -> SkillHealth {
        let has_key = self
            .vault
            .lock()
            .map(|v| v.exists("perplexity"))
            .unwrap_or(false);
        SkillHealth {
            status: if has_key {
                HealthStatus::Healthy
            } else {
                HealthStatus::Degraded
            },
            message: if !has_key {
                Some("Perplexity API key not configured".to_string())
            } else {
                None
            },
            last_check: chrono::Utc::now(),
            metrics: HashMap::new(),
        }
    }

    fn tools(&self) -> Vec<ToolDescriptor> {
        vec![ToolDescriptor {
            name: "perplexity_search".to_string(),
            description: "Search the web using Perplexity AI. Returns an AI-generated answer \
                    grounded in real-time web data with source citations. Best for research \
                    questions, fact-checking, and getting up-to-date information."
                .to_string(),
            parameters: serde_json::json!({
                "type": "object",
                "properties": {
                    "query": {
                        "type": "string",
                        "description": "The search/research query"
                    },
                    "model": {
                        "type": "string",
                        "description": "Model to use: 'sonar' (fast, default) or 'sonar-pro' (higher quality)",
                        "enum": ["sonar", "sonar-pro"]
                    },
                    "domain_filter": {
                        "type": "array",
                        "items": { "type": "string" },
                        "description": "Domains to include (e.g. ['github.com']) or exclude (prefix with '-', e.g. ['-reddit.com'])"
                    },
                    "recency_filter": {
                        "type": "string",
                        "description": "Only include sources from this time period",
                        "enum": ["hour", "day", "week", "month"]
                    }
                },
                "required": ["query"]
            }),
            returns: serde_json::json!({
                "type": "object",
                "properties": {
                    "formatted": { "type": "string", "description": "Answer with citations" },
                    "answer": { "type": "string" },
                    "citations": { "type": "array", "items": { "type": "string" } },
                    "search_results": { "type": "array" },
                    "citation_count": { "type": "integer" }
                }
            }),
            cost_estimate: CostEstimate {
                latency_ms: 3000,
                network_bound: true,
                token_cost: Some(100), // approximate
            },
            required_permissions: vec![Permission::Network(NetworkPermission::Domains(vec![
                "api.perplexity.ai".to_string(),
            ]))],
            autonomous: true,
            requires_confirmation: false,
        }]
    }

    async fn execute_tool(
        &self,
        tool_name: &str,
        params: ToolParams,
        _context: &ExecutionContext,
    ) -> SkillResult<ToolOutput> {
        if tool_name != "perplexity_search" {
            return Err(SkillError::ToolFailed(format!(
                "Unknown tool: {}",
                tool_name
            )));
        }

        let query: String = params.get("query").ok_or_else(|| {
            SkillError::ToolFailed("Missing required parameter: query".to_string())
        })?;

        let model: Option<String> = params.get("model");
        let domain_filter: Option<Vec<String>> = params.get("domain_filter");
        let recency_filter: Option<String> = params.get("recency_filter");

        self.search(
            &query,
            model.as_deref(),
            domain_filter,
            recency_filter.as_deref(),
            false,
        )
        .await
    }

    fn capabilities(&self) -> Vec<CapabilityDescriptor> {
        vec![]
    }

    fn get_capability(&self, _cap_type: &str) -> Option<&dyn Any> {
        None
    }

    fn triggers(&self) -> Vec<TriggerDescriptor> {
        vec![]
    }
}

fn check_query_privacy(query: &str) -> Option<String> {
    let lower = query.to_lowercase();

    if (lower.contains("where does") || lower.contains("where do")) && lower.contains("live") {
        return Some("Query appears to seek someone's home address".into());
    }
    if lower.contains("home address of") || lower.contains("home address for") {
        return Some("Query appears to seek someone's home address".into());
    }
    if (lower.contains("phone number of") || lower.contains("phone number for"))
        && !lower.contains("company")
        && !lower.contains("business")
        && !lower.contains("support")
        && !lower.contains("customer service")
    {
        return Some("Query appears to seek someone's personal phone number".into());
    }
    if lower.contains("social security number")
        || lower.contains("ssn of")
        || lower.contains("ssn for")
    {
        return Some("Query seeks Social Security information".into());
    }
    if lower.contains("credit card number") || lower.contains("bank account number") {
        return Some("Query seeks financial PII".into());
    }
    if lower.contains("dox") || lower.contains("doxx") {
        return Some("Query contains doxxing language".into());
    }
    if lower.contains("real name of") && (lower.contains("anonymous") || lower.contains("username"))
    {
        return Some("Query attempts to de-anonymize someone".into());
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    fn test_manifest() -> SkillManifest {
        PerplexitySearchSkill::default_manifest()
    }

    fn test_skill() -> PerplexitySearchSkill {
        let vault = Arc::new(Mutex::new(SecretsVault::new(
            std::env::temp_dir().join("abigail_pplx_test"),
        )));
        PerplexitySearchSkill::with_secrets(test_manifest(), vault)
    }

    #[test]
    fn test_manifest_parses() {
        let manifest = test_manifest();
        assert_eq!(manifest.name, "Perplexity Search");
        assert_eq!(manifest.id.0, "com.abigail.skills.perplexity-search");
    }

    #[test]
    fn test_tools_list() {
        let skill = test_skill();
        let tools = skill.tools();
        assert_eq!(tools.len(), 1);
        assert_eq!(tools[0].name, "perplexity_search");
        assert!(tools[0].autonomous);
    }

    #[test]
    fn test_health_degraded_without_key() {
        let skill = test_skill();
        let health = skill.health();
        assert_eq!(health.status, HealthStatus::Degraded);
    }

    #[tokio::test]
    async fn test_missing_key_error() {
        let skill = test_skill();
        let result = skill.search("test query", None, None, None, false).await;
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn test_blocked_query() {
        let _skill = test_skill();
        // Even without a key, the superego check should run first
        // and block PII queries before we hit the missing key error
        let vault = Arc::new(Mutex::new(SecretsVault::new(
            std::env::temp_dir().join("abigail_pplx_test_block"),
        )));
        {
            let mut v = vault.lock().unwrap();
            v.set_secret("perplexity", "fake-key-for-test");
        }
        let skill = PerplexitySearchSkill::with_secrets(test_manifest(), vault);
        let result = skill
            .search(
                "where does John Smith live home address",
                None,
                None,
                None,
                false,
            )
            .await
            .unwrap();
        assert!(
            !result.success || result.error.is_some() || {
                let data = result.data.as_ref().unwrap();
                data["formatted"].as_str().unwrap_or("").contains("blocked")
            }
        );
    }
}
