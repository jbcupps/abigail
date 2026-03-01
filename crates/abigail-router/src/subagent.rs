//! Subagent management: definitions, registry, and delegation.
//!
//! Foundation for the supervisor pattern — the main agent (Ego) can delegate
//! tasks to specialized subagents, each with their own capabilities and constraints.

use abigail_capabilities::cognitive::{
    CompletionRequest, CompletionResponse, Message, ToolDefinition,
};
use serde::Serialize;
use std::sync::Arc;

use crate::router::IdEgoRouter;

/// Which LLM provider backs a subagent.
#[derive(Debug, Clone, Serialize)]
pub enum SubagentProvider {
    /// Use the main Ego provider.
    SameAsEgo,
    /// Use the local Id provider.
    SameAsId,
    /// Custom provider identified by name and API key.
    Custom(String, String),
}

/// Declares a subagent's identity, capabilities, and constraints.
#[derive(Debug, Clone, Serialize)]
pub struct SubagentDefinition {
    /// Unique identifier for this subagent.
    pub id: String,
    /// Human-readable name.
    pub name: String,
    /// What this subagent does.
    pub description: String,
    /// Capability tags (e.g., "web_search", "file_read").
    pub capabilities: Vec<String>,
    /// Which LLM backs this subagent.
    pub provider: SubagentProvider,
}

/// Registry and lifecycle manager for subagents.
///
/// Follows the supervisor pattern: the main agent can delegate tasks
/// to registered subagents. Each subagent has declared capabilities
/// and uses a provider resolved through the main router.
#[derive(Clone)]
pub struct SubagentManager {
    definitions: Vec<SubagentDefinition>,
    router: Arc<IdEgoRouter>,
}

impl SubagentManager {
    /// Create a new SubagentManager backed by the given router.
    pub fn new(router: Arc<IdEgoRouter>) -> Self {
        Self {
            definitions: Vec::new(),
            router,
        }
    }

    /// Register a subagent definition.
    pub fn register(&mut self, def: SubagentDefinition) {
        // Replace existing definition with the same id
        self.definitions.retain(|d| d.id != def.id);
        self.definitions.push(def);
    }

    /// List all registered subagent definitions.
    pub fn list(&self) -> &[SubagentDefinition] {
        &self.definitions
    }

    /// Update the router reference (e.g. after router rebuild).
    pub fn update_router(&mut self, router: Arc<IdEgoRouter>) {
        self.router = router;
    }

    /// Delegate a task to a specific subagent by id.
    ///
    /// Resolves the subagent's provider, builds a completion request with the
    /// given messages and tools, and returns the response.
    pub async fn delegate(
        &self,
        subagent_id: &str,
        messages: Vec<Message>,
        tools: Vec<ToolDefinition>,
    ) -> anyhow::Result<CompletionResponse> {
        let def = self
            .definitions
            .iter()
            .find(|d| d.id == subagent_id)
            .ok_or_else(|| anyhow::anyhow!("Subagent '{}' not found", subagent_id))?;

        Self::validate_delegation_policy(def, &messages)?;

        tracing::info!(
            "Delegating to subagent '{}' ({}), {} messages, {} tools",
            def.name,
            def.id,
            messages.len(),
            tools.len()
        );

        match &def.provider {
            SubagentProvider::SameAsEgo => {
                let request = CompletionRequest {
                    messages,
                    tools: if tools.is_empty() { None } else { Some(tools) },
                    model_override: None,
                };
                self.router
                    .route_with_tools(request.messages, request.tools.unwrap_or_default())
                    .await
            }
            SubagentProvider::SameAsId => self.router.route(messages).await,
            SubagentProvider::Custom(_provider_name, _api_key) => {
                // Custom provider delegation — future phase.
                // For now, fall back to the main router's Ego.
                tracing::warn!("Custom subagent provider not yet implemented, falling back to Ego");
                let request = CompletionRequest {
                    messages,
                    tools: if tools.is_empty() { None } else { Some(tools) },
                    model_override: None,
                };
                self.router
                    .route_with_tools(request.messages, request.tools.unwrap_or_default())
                    .await
            }
        }
    }

    fn validate_delegation_policy(
        def: &SubagentDefinition,
        messages: &[Message],
    ) -> anyhow::Result<()> {
        if def.capabilities.is_empty() {
            anyhow::bail!(
                "Delegation policy denied for '{}': no declared capabilities.",
                def.id
            );
        }

        let user_prompt = messages
            .iter()
            .rev()
            .find(|m| m.role == "user")
            .map(|m| m.content.to_lowercase())
            .unwrap_or_default();

        if user_prompt.trim().is_empty() {
            anyhow::bail!(
                "Delegation policy denied for '{}': empty user message.",
                def.id
            );
        }

        // Guard destructive intents unless the subagent explicitly declares the capability.
        let destructive_intent = ["delete", "wipe", "destroy", "drop table", "format disk"]
            .iter()
            .any(|needle| user_prompt.contains(needle));
        let allows_destructive = def
            .capabilities
            .iter()
            .any(|cap| cap.eq_ignore_ascii_case("destructive_ops"));

        if destructive_intent && !allows_destructive {
            anyhow::bail!(
                "Delegation policy denied for '{}': destructive intent requires 'destructive_ops' capability.",
                def.id
            );
        }

        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_router() -> Arc<IdEgoRouter> {
        Arc::new(IdEgoRouter::new(
            None,
            None,
            None,
            None,
            abigail_core::RoutingMode::default(),
        ))
    }

    #[test]
    fn test_register_and_list() {
        let router = make_router();
        let mut mgr = SubagentManager::new(router);

        assert!(mgr.list().is_empty());

        mgr.register(SubagentDefinition {
            id: "test-1".into(),
            name: "Test Agent".into(),
            description: "A test subagent".into(),
            capabilities: vec!["web_search".into()],
            provider: SubagentProvider::SameAsEgo,
        });

        assert_eq!(mgr.list().len(), 1);
        assert_eq!(mgr.list()[0].id, "test-1");
    }

    #[test]
    fn test_register_replaces_existing() {
        let router = make_router();
        let mut mgr = SubagentManager::new(router);

        mgr.register(SubagentDefinition {
            id: "test-1".into(),
            name: "V1".into(),
            description: "first".into(),
            capabilities: vec![],
            provider: SubagentProvider::SameAsId,
        });

        mgr.register(SubagentDefinition {
            id: "test-1".into(),
            name: "V2".into(),
            description: "replaced".into(),
            capabilities: vec!["file_read".into()],
            provider: SubagentProvider::SameAsEgo,
        });

        assert_eq!(mgr.list().len(), 1);
        assert_eq!(mgr.list()[0].name, "V2");
    }

    #[tokio::test]
    async fn test_delegate_unknown_subagent() {
        let router = make_router();
        let mgr = SubagentManager::new(router);

        let result = mgr
            .delegate("nonexistent", vec![Message::new("user", "hello")], vec![])
            .await;

        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("not found"));
    }

    #[tokio::test]
    async fn test_delegate_policy_denies_empty_capabilities() {
        let router = make_router();
        let mut mgr = SubagentManager::new(router);
        mgr.register(SubagentDefinition {
            id: "empty".into(),
            name: "Empty".into(),
            description: "No capabilities".into(),
            capabilities: vec![],
            provider: SubagentProvider::SameAsEgo,
        });

        let result = mgr
            .delegate("empty", vec![Message::new("user", "hello")], vec![])
            .await;
        assert!(result.is_err());
        assert!(result
            .unwrap_err()
            .to_string()
            .contains("no declared capabilities"));
    }
}
