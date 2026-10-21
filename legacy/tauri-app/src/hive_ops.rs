use crate::state::AppState;
use abigail_skills::{HiveAgentInfo, HiveOperations};
use async_trait::async_trait;
use tauri::{AppHandle, Manager};

pub struct TauriHiveOps {
    app_handle: AppHandle,
}

impl TauriHiveOps {
    pub fn new(app_handle: AppHandle) -> Self {
        Self { app_handle }
    }
}

#[async_trait]
impl HiveOperations for TauriHiveOps {
    async fn list_agents(&self) -> Result<Vec<HiveAgentInfo>, String> {
        let state = self.app_handle.state::<AppState>();
        let agents = state.identity_manager.list_agents()?;
        Ok(agents
            .into_iter()
            .map(|a| HiveAgentInfo {
                id: a.id,
                name: a.name,
            })
            .collect())
    }

    async fn load_agent(&self, agent_id: &str) -> Result<(), String> {
        let state = self.app_handle.state::<AppState>();
        crate::commands::identity::load_agent(state, agent_id.to_string()).await
    }

    async fn create_agent(&self, name: &str) -> Result<String, String> {
        let state = self.app_handle.state::<AppState>();
        crate::commands::identity::create_agent(state, name.to_string())
    }

    async fn get_active_agent_id(&self) -> Result<Option<String>, String> {
        let state = self.app_handle.state::<AppState>();
        let active = state.active_agent_id.read().map_err(|e| e.to_string())?;
        Ok(active.clone())
    }

    async fn get_config_value(&self, key: &str) -> Result<serde_json::Value, String> {
        // Security check: Never expose API keys or secrets
        if key.contains("api_key") || key.contains("secrets") {
            return Err("Access denied to sensitive configuration key".to_string());
        }

        let state = self.app_handle.state::<AppState>();
        let config = state.config.read().map_err(|e| e.to_string())?;

        let val = match key {
            "agent_name" => serde_json::to_value(&config.agent_name),
            "primary_color" => serde_json::to_value(&config.primary_color),
            "avatar_url" => serde_json::to_value(&config.avatar_url),
            "routing_mode" => serde_json::to_value(&config.routing_mode),
            "local_llm_base_url" => serde_json::to_value(&config.local_llm_base_url),
            "birth_complete" => serde_json::to_value(&config.birth_complete),
            _ => return Err(format!("Unknown or restricted config key: {}", key)),
        }
        .map_err(|e| e.to_string())?;

        Ok(val)
    }

    async fn set_config_value(&self, key: &str, value: serde_json::Value) -> Result<(), String> {
        // Security check: Never allow writing API keys or secrets via this path
        if key.contains("api_key") || key.contains("secrets") {
            return Err("Access denied to sensitive configuration key".to_string());
        }

        let state = self.app_handle.state::<AppState>();
        {
            let mut config = state.config.write().map_err(|e| e.to_string())?;

            match key {
                "agent_name" => {
                    config.agent_name =
                        Some(serde_json::from_value(value).map_err(|e| e.to_string())?)
                }
                "primary_color" => {
                    config.primary_color =
                        Some(serde_json::from_value(value).map_err(|e| e.to_string())?)
                }
                "avatar_url" => {
                    config.avatar_url =
                        Some(serde_json::from_value(value).map_err(|e| e.to_string())?)
                }
                "local_llm_base_url" => {
                    let url: Option<String> =
                        serde_json::from_value(value).map_err(|e| e.to_string())?;
                    if let Some(u) = url {
                        let normalized =
                            abigail_core::validate_local_llm_url(&u).map_err(|e| e.to_string())?;
                        config.local_llm_base_url = Some(normalized);
                    } else {
                        config.local_llm_base_url = None;
                    }
                }
                _ => return Err(format!("Unknown or restricted config key: {}", key)),
            }

            config
                .save(&config.config_path())
                .map_err(|e| e.to_string())?;
        }

        // Rebuild router if we changed the local URL
        if key == "local_llm_base_url" {
            crate::rebuild_router(&state).await?;
        }

        Ok(())
    }

    async fn set_skill_secret(&self, key: &str, value: &str) -> Result<(), String> {
        let state = self.app_handle.state::<AppState>();

        let data_dir = {
            let config = state.config.read().map_err(|e| e.to_string())?;
            config.data_dir.clone()
        };
        crate::commands::skills::validate_secret_namespace_with(&state.registry, &data_dir, key)?;

        {
            let mut vault = state.skills_secrets.lock().map_err(|e| e.to_string())?;
            vault.set_secret(key, value);
            vault.save().map_err(|e| e.to_string())?;
        }

        Ok(())
    }

    async fn get_skill_secret_names(&self) -> Result<Vec<String>, String> {
        let state = self.app_handle.state::<AppState>();
        let vault = state.skills_secrets.lock().map_err(|e| e.to_string())?;
        Ok(vault
            .list_providers()
            .into_iter()
            .map(|s| s.to_string())
            .collect())
    }
}
