//! Persisted model choices. Presence and credentials are never routing consent.
use serde::{Deserialize, Serialize};

pub const MAX_APPROVED_MODELS: usize = 3;
pub const MAX_SAVED_MODELS: usize = 16;

/// Private Hive-to-runtime handoff. Debug output never includes credential values.
#[derive(Clone, Serialize, Deserialize)]
pub struct ResolvedModelConnection {
    pub connection: ModelConnection,
    pub api_key: Option<String>,
    pub ready: bool,
}

impl std::fmt::Debug for ResolvedModelConnection {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ResolvedModelConnection")
            .field("connection", &self.connection)
            .field("ready", &self.ready)
            .finish_non_exhaustive()
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ModelConnection {
    pub provider: String,
    pub model: Option<String>,
    pub local_url: Option<String>,
}

impl ModelConnection {
    pub fn validate(&self) -> Result<(), String> {
        if !matches!(
            self.provider.as_str(),
            "local"
                | "openai"
                | "anthropic"
                | "google"
                | "xai"
                | "perplexity"
                | "claude-cli"
                | "codex-cli"
                | "grok-cli"
        ) {
            return Err("This model connection is not supported.".into());
        }
        if self.model.as_ref().is_some_and(|m| {
            m.trim().is_empty() || m.len() > 200 || m.chars().any(char::is_control)
        }) {
            return Err("Choose a valid model.".into());
        }
        if self.provider == "local" {
            crate::validate_local_llm_url(
                self.local_url.as_deref().ok_or("Choose a local server.")?,
            )
            .map_err(|e| e.to_string())?;
        } else if self.local_url.is_some() {
            return Err("Only a local connection can specify a local server.".into());
        }
        Ok(())
    }
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ModelRoutingPolicy {
    pub connections: Vec<ModelConnection>,
    pub allowed: Vec<ModelConnection>,
}

impl ModelRoutingPolicy {
    pub fn validate(&self) -> Result<(), String> {
        if self.connections.len() > MAX_SAVED_MODELS || self.allowed.len() > MAX_APPROVED_MODELS {
            return Err("Choose at most three models for automatic routing.".into());
        }
        for (index, route) in self.connections.iter().enumerate() {
            route.validate()?;
            if self.connections[..index].contains(route) {
                return Err("Duplicate saved model connection.".into());
            }
        }
        for (index, route) in self.allowed.iter().enumerate() {
            if !self.connections.contains(route) || self.allowed[..index].contains(route) {
                return Err("Choose only saved, checked model connections, once each.".into());
            }
        }
        Ok(())
    }

    /// Only called after an explicit, successful connection check.
    pub fn approve_checked(&mut self, route: ModelConnection) -> Result<(), String> {
        let mut next = self.clone();
        if !next.connections.contains(&route) {
            next.connections.push(route.clone());
        }
        next.allowed.retain(|item| item != &route);
        next.allowed.insert(0, route);
        next.validate()?;
        *self = next;
        Ok(())
    }

    pub fn choose(&mut self, allowed: Vec<ModelConnection>) -> Result<(), String> {
        let next = Self {
            connections: self.connections.clone(),
            allowed,
        };
        next.validate()?;
        *self = next;
        Ok(())
    }
}

/// A legacy explicit primary is the only compatible inherited approval.
pub fn effective_policy(config: &crate::AppConfig) -> Result<ModelRoutingPolicy, String> {
    if let Some(policy) = &config.model_routing {
        policy.validate()?;
        return Ok(policy.clone());
    }
    let mut policy = ModelRoutingPolicy::default();
    if let Some(provider) = &config.active_provider_preference {
        let route = ModelConnection {
            provider: provider.clone(),
            model: config.ego_model.clone(),
            local_url: (provider == "local")
                .then(|| config.local_llm_base_url.clone())
                .flatten(),
        };
        // Invalid old choices require setup, never a guessed replacement.
        if route.validate().is_ok() {
            policy.approve_checked(route)?;
        }
    }
    Ok(policy)
}

#[cfg(test)]
mod tests {
    use super::*;
    fn route(name: &str) -> ModelConnection {
        ModelConnection {
            provider: name.into(),
            model: Some("fixture".into()),
            local_url: None,
        }
    }
    #[test]
    fn only_explicit_checked_connections_can_enter_policy() {
        let mut policy = ModelRoutingPolicy::default();
        assert!(policy.choose(vec![route("openai")]).is_err());
        policy.approve_checked(route("openai")).unwrap();
        policy.approve_checked(route("anthropic")).unwrap();
        assert_eq!(policy.allowed[0].provider, "anthropic");
        assert!(policy.choose(vec![route("google")]).is_err());
        policy.choose(vec![]).unwrap();
        assert!(policy.allowed.is_empty());
        assert_eq!(
            serde_json::from_str::<ModelRoutingPolicy>(&serde_json::to_string(&policy).unwrap())
                .unwrap(),
            policy
        );
    }
    #[test]
    fn legacy_approval_never_expands_to_other_credentials_or_local_fallback() {
        let mut config = crate::AppConfig::default_paths();
        config.local_llm_base_url = Some("http://127.0.0.1:11434".into());
        assert!(effective_policy(&config).unwrap().allowed.is_empty());
        config.active_provider_preference = Some("openai".into());
        assert_eq!(
            effective_policy(&config).unwrap().allowed,
            vec![ModelConnection {
                model: None,
                ..route("openai")
            }]
        );
        config.model_routing = Some(ModelRoutingPolicy::default());
        assert!(effective_policy(&config).unwrap().allowed.is_empty());
    }
    #[test]
    fn limits_and_duplicate_choices_are_atomic() {
        let mut policy = ModelRoutingPolicy::default();
        for name in ["openai", "anthropic", "google"] {
            policy.approve_checked(route(name)).unwrap();
        }
        let prior = policy.clone();
        assert!(policy.approve_checked(route("xai")).is_err());
        assert_eq!(policy, prior);
        assert!(policy
            .choose(vec![route("openai"), route("openai")])
            .is_err());
        assert_eq!(policy, prior);
    }
}
