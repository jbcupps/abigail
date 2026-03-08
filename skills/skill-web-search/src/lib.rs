//! Web Search skill — searches the web via Tavily API with privacy guardrails.

use std::any::Any;
use std::collections::HashMap;
use std::sync::{Arc, Mutex};

use abigail_capabilities::sensory::web_search;
use abigail_core::secrets::SecretsVault;
use abigail_skills::channel::TriggerDescriptor;
use abigail_skills::manifest::{
    CapabilityDescriptor, NetworkPermission, Permission, SkillManifest,
};
use abigail_skills::skill::{
    CostEstimate, ExecutionContext, HealthStatus, Skill, SkillConfig, SkillError, SkillHealth,
    SkillResult, ToolDescriptor, ToolOutput, ToolParams,
};
use async_trait::async_trait;

pub struct WebSearchSkill {
    manifest: SkillManifest,
    vault: Arc<Mutex<SecretsVault>>,
}

impl WebSearchSkill {
    /// Create a new WebSearchSkill with access to the shared secrets vault.
    pub fn with_secrets(manifest: SkillManifest, vault: Arc<Mutex<SecretsVault>>) -> Self {
        Self { manifest, vault }
    }

    /// Parse the embedded skill.toml into a SkillManifest.
    pub fn default_manifest() -> SkillManifest {
        let toml_str = include_str!("../skill.toml");
        SkillManifest::parse(toml_str).expect("embedded skill.toml must be valid")
    }
}

#[async_trait]
impl Skill for WebSearchSkill {
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
            .map(|v| v.exists("tavily"))
            .unwrap_or(false);

        SkillHealth {
            status: if has_key {
                HealthStatus::Healthy
            } else {
                HealthStatus::Degraded
            },
            message: if has_key {
                None
            } else {
                Some("Tavily API key not configured".to_string())
            },
            last_check: chrono::Utc::now(),
            metrics: HashMap::new(),
        }
    }

    fn tools(&self) -> Vec<ToolDescriptor> {
        vec![ToolDescriptor {
            name: "web_search".to_string(),
            description:
                "Search the web for current information. Returns an answer and numbered sources."
                    .to_string(),
            parameters: serde_json::json!({
                "type": "object",
                "properties": {
                    "query": {
                        "type": "string",
                        "description": "The search query"
                    },
                    "max_results": {
                        "type": "integer",
                        "description": "Maximum number of results to return (default: 5)",
                        "default": 5
                    }
                },
                "required": ["query"]
            }),
            returns: serde_json::json!({
                "type": "object",
                "properties": {
                    "formatted": { "type": "string" }
                }
            }),
            cost_estimate: CostEstimate {
                latency_ms: 2000,
                network_bound: true,
                token_cost: None,
            },
            required_permissions: vec![Permission::Network(NetworkPermission::Domains(vec![
                "api.tavily.com".to_string(),
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
        if tool_name != "web_search" {
            return Err(SkillError::ToolFailed(format!(
                "Unknown tool: {}",
                tool_name
            )));
        }

        // Extract query
        let query: String = params.get("query").ok_or_else(|| {
            SkillError::ToolFailed("Missing required parameter: query".to_string())
        })?;

        let max_results: u32 = params.get("max_results").unwrap_or(5);

        if let Some(reason) = check_query_privacy(&query) {
            tracing::warn!("Blocked search query: {}", reason);
            return Ok(ToolOutput::error(format!("Search blocked: {}", reason)));
        }

        // Get Tavily API key from vault
        let api_key = {
            let vault = self
                .vault
                .lock()
                .map_err(|e| SkillError::ToolFailed(format!("Vault lock error: {}", e)))?;
            match vault.get_secret("tavily") {
                Some(secret) => secret.to_string(),
                None => {
                    return Err(SkillError::MissingSecret(
                        "Tavily API key not configured. Add it in The Forge > API Keys."
                            .to_string(),
                    ));
                }
            }
        };

        // Execute search
        let response = web_search::tavily_search(&api_key, &query, max_results)
            .await
            .map_err(SkillError::ToolFailed)?;

        let formatted = web_search::format_search_results(&response);

        Ok(ToolOutput::success(serde_json::json!({
            "formatted": formatted,
            "answer": response.answer,
            "result_count": response.results.len(),
        })))
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

    #[test]
    fn test_default_manifest_parses() {
        let manifest = WebSearchSkill::default_manifest();
        assert_eq!(manifest.id.0, "com.abigail.skills.web-search");
        assert_eq!(manifest.name, "Web Search");
        assert_eq!(manifest.secrets.len(), 1);
        assert_eq!(manifest.secrets[0].name, "tavily");
    }

    #[test]
    fn test_tools_returns_web_search() {
        let tmp = std::env::temp_dir().join("abigail_ws_skill_test");
        let _ = std::fs::remove_dir_all(&tmp);
        std::fs::create_dir_all(&tmp).unwrap();

        let vault = Arc::new(Mutex::new(SecretsVault::new(tmp.clone())));
        let skill = WebSearchSkill::with_secrets(WebSearchSkill::default_manifest(), vault);
        let tools = skill.tools();
        assert_eq!(tools.len(), 1);
        assert_eq!(tools[0].name, "web_search");

        let _ = std::fs::remove_dir_all(&tmp);
    }

    #[tokio::test]
    async fn test_execute_tool_blocked_query() {
        let tmp = std::env::temp_dir().join("abigail_ws_skill_blocked");
        let _ = std::fs::remove_dir_all(&tmp);
        std::fs::create_dir_all(&tmp).unwrap();

        let vault = Arc::new(Mutex::new(SecretsVault::new(tmp.clone())));
        {
            let mut v = vault.lock().unwrap();
            v.set_secret("tavily", "tvly-test-key");
        }

        let skill = WebSearchSkill::with_secrets(WebSearchSkill::default_manifest(), vault);
        let params = ToolParams::new().with("query", "where does Elon Musk live");
        let ctx = ExecutionContext {
            request_id: "test".to_string(),
            user_id: None,
        };

        let result = skill
            .execute_tool("web_search", params, &ctx)
            .await
            .unwrap();
        assert!(!result.success);
        assert!(result.error.unwrap().contains("Search blocked"));

        let _ = std::fs::remove_dir_all(&tmp);
    }

    #[tokio::test]
    async fn test_execute_tool_missing_key() {
        let tmp = std::env::temp_dir().join("abigail_ws_skill_nokey");
        let _ = std::fs::remove_dir_all(&tmp);
        std::fs::create_dir_all(&tmp).unwrap();

        let vault = Arc::new(Mutex::new(SecretsVault::new(tmp.clone())));
        let skill = WebSearchSkill::with_secrets(WebSearchSkill::default_manifest(), vault);
        let params = ToolParams::new().with("query", "test query");
        let ctx = ExecutionContext {
            request_id: "test".to_string(),
            user_id: None,
        };

        let result = skill.execute_tool("web_search", params, &ctx).await;
        assert!(result.is_err());

        let _ = std::fs::remove_dir_all(&tmp);
    }
}
