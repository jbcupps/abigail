use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

/// An entry in the global config representing a registered agent.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AgentEntry {
    /// Unique identifier for this agent (UUID v4).
    pub id: String,
    /// Human-readable name for this agent.
    pub name: String,
    /// Path to this agent's data directory (relative to identities/).
    pub directory: PathBuf,
}

/// Global settings for the Hive (multi-agent host).
/// Stored at `{data_root}/global_settings.json`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GlobalConfig {
    /// Path to the master.key file (Ed25519 signing key for the Hive).
    pub master_key_path: PathBuf,
    /// List of registered agents.
    #[serde(default)]
    pub agents: Vec<AgentEntry>,
}

impl GlobalConfig {
    /// Create a new GlobalConfig with default paths relative to data_root.
    pub fn new(data_root: &Path) -> Self {
        Self {
            master_key_path: data_root.join("master.key"),
            agents: Vec::new(),
        }
    }

    /// Path to the global_settings.json file.
    pub fn config_path(data_root: &Path) -> PathBuf {
        data_root.join("global_settings.json")
    }

    /// Load GlobalConfig from disk.
    pub fn load(data_root: &Path) -> anyhow::Result<Self> {
        let path = Self::config_path(data_root);
        let content = std::fs::read_to_string(&path)?;
        let config: Self = serde_json::from_str(&content)?;
        Ok(config)
    }

    /// Save GlobalConfig to disk.
    pub fn save(&self, data_root: &Path) -> anyhow::Result<()> {
        let path = Self::config_path(data_root);
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        let content = serde_json::to_string_pretty(self)?;
        std::fs::write(&path, content)?;
        Ok(())
    }

    /// Register a new agent. Returns error if UUID already exists.
    pub fn register_agent(&mut self, entry: AgentEntry) -> anyhow::Result<()> {
        if self.agents.iter().any(|a| a.id == entry.id) {
            anyhow::bail!("Agent with id {} already registered", entry.id);
        }
        self.agents.push(entry);
        Ok(())
    }

    /// Find an agent by UUID.
    pub fn find_agent(&self, id: &str) -> Option<&AgentEntry> {
        self.agents.iter().find(|a| a.id == id)
    }

    /// Remove an agent by UUID. Returns true if found and removed.
    pub fn remove_agent(&mut self, id: &str) -> bool {
        let len_before = self.agents.len();
        self.agents.retain(|a| a.id != id);
        self.agents.len() < len_before
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    #[test]
    fn test_global_config_roundtrip() {
        let tmp = std::env::temp_dir().join("abigail_global_config_test");
        let _ = fs::remove_dir_all(&tmp);
        fs::create_dir_all(&tmp).unwrap();

        let mut config = GlobalConfig::new(&tmp);
        config
            .register_agent(AgentEntry {
                id: "test-uuid-1".to_string(),
                name: "Agent Alpha".to_string(),
                directory: tmp.join("identities/test-uuid-1"),
            })
            .unwrap();

        config.save(&tmp).unwrap();

        let loaded = GlobalConfig::load(&tmp).unwrap();
        assert_eq!(loaded.agents.len(), 1);
        assert_eq!(loaded.agents[0].name, "Agent Alpha");

        let _ = fs::remove_dir_all(&tmp);
    }

    #[test]
    fn test_register_duplicate_fails() {
        let tmp = std::env::temp_dir().join("abigail_global_config_dup");
        let _ = fs::remove_dir_all(&tmp);

        let mut config = GlobalConfig::new(&tmp);
        config
            .register_agent(AgentEntry {
                id: "dup-uuid".to_string(),
                name: "Agent".to_string(),
                directory: PathBuf::from("identities/dup-uuid"),
            })
            .unwrap();

        let result = config.register_agent(AgentEntry {
            id: "dup-uuid".to_string(),
            name: "Agent 2".to_string(),
            directory: PathBuf::from("identities/dup-uuid"),
        });
        assert!(result.is_err());
    }

    #[test]
    fn test_find_and_remove_agent() {
        let tmp = std::env::temp_dir().join("abigail_global_config_find_remove");
        let _ = fs::remove_dir_all(&tmp);
        fs::create_dir_all(&tmp).unwrap();
        let mut config = GlobalConfig::new(&tmp);
        config
            .register_agent(AgentEntry {
                id: "a1".to_string(),
                name: "Alpha".to_string(),
                directory: PathBuf::from("identities/a1"),
            })
            .unwrap();
        config
            .register_agent(AgentEntry {
                id: "a2".to_string(),
                name: "Beta".to_string(),
                directory: PathBuf::from("identities/a2"),
            })
            .unwrap();

        assert!(config.find_agent("a1").is_some());
        assert!(config.find_agent("a3").is_none());

        assert!(config.remove_agent("a1"));
        assert!(config.find_agent("a1").is_none());
        assert_eq!(config.agents.len(), 1);

        let _ = fs::remove_dir_all(&tmp);
    }
}
