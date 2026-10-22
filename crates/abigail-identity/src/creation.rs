//! Durable idempotent Entity creation receipts.

use crate::IdentityManager;
use abigail_core::{AgentCreationRecord, AgentCreationState, AgentEntry, GlobalConfig};
use std::path::PathBuf;
use uuid::Uuid;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AgentCreationStatus {
    Absent,
    Reserved {
        name: String,
    },
    Complete {
        name: String,
        id: String,
        directory: PathBuf,
    },
    Unavailable {
        name: String,
        id: String,
    },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AgentCreationError {
    InvalidRequest(String),
    Conflict(String),
    Unavailable(String),
    Storage(String),
}

impl std::fmt::Display for AgentCreationError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::InvalidRequest(message)
            | Self::Conflict(message)
            | Self::Unavailable(message)
            | Self::Storage(message) => formatter.write_str(message),
        }
    }
}

impl std::error::Error for AgentCreationError {}

fn validate_key(key: &str) -> Result<(), AgentCreationError> {
    if !Uuid::parse_str(key).is_ok_and(|uuid| uuid.to_string() == key) {
        return Err(AgentCreationError::InvalidRequest(
            "creation_key must be a canonical UUID.".into(),
        ));
    }
    Ok(())
}

fn validate_name(name: &str) -> Result<(), AgentCreationError> {
    if name.is_empty()
        || name.trim() != name
        || name.chars().count() > 80
        || name.chars().any(char::is_control)
    {
        return Err(AgentCreationError::InvalidRequest(
            "Entity name must be trimmed, contain 1 to 80 characters, and have no control characters."
                .into(),
        ));
    }
    Ok(())
}

impl IdentityManager {
    pub fn entity_creation_status(
        &self,
        key: &str,
    ) -> Result<AgentCreationStatus, AgentCreationError> {
        validate_key(key)?;
        let config = self
            .global_config
            .read()
            .map_err(|error| AgentCreationError::Storage(error.to_string()))?;
        self.creation_status_in(&config, key)
    }

    fn creation_status_in(
        &self,
        config: &GlobalConfig,
        key: &str,
    ) -> Result<AgentCreationStatus, AgentCreationError> {
        let Some(record) = config.creation_attempts.get(key) else {
            return Ok(AgentCreationStatus::Absent);
        };
        if !Uuid::parse_str(&record.entity_id)
            .is_ok_and(|uuid| uuid.to_string() == record.entity_id)
        {
            return Err(AgentCreationError::Storage(
                "Creation reservation contains an invalid entity ID.".into(),
            ));
        }
        if record.state == AgentCreationState::Reserved {
            return Ok(AgentCreationStatus::Reserved {
                name: record.name.clone(),
            });
        }
        let directory = self.identities_dir().join(&record.entity_id);
        let registered = config.find_agent(&record.entity_id).is_some_and(|entry| {
            !entry.is_hive && entry.directory == self.registry_directory_for(&record.entity_id)
        });
        if registered && directory.join("config.json").is_file() {
            Ok(AgentCreationStatus::Complete {
                name: record.name.clone(),
                id: record.entity_id.clone(),
                directory,
            })
        } else {
            Ok(AgentCreationStatus::Unavailable {
                name: record.name.clone(),
                id: record.entity_id.clone(),
            })
        }
    }

    pub fn create_agent_idempotent(
        &self,
        name: &str,
        key: &str,
    ) -> Result<(String, PathBuf), AgentCreationError> {
        validate_key(key)?;
        validate_name(name)?;
        let mut config = self
            .global_config
            .write()
            .map_err(|error| AgentCreationError::Storage(error.to_string()))?;
        if config
            .creation_attempts
            .get(key)
            .is_some_and(|record| record.name != name)
        {
            return Err(AgentCreationError::Conflict(
                "This creation key belongs to a different Entity name.".into(),
            ));
        }
        match self.creation_status_in(&config, key)? {
            AgentCreationStatus::Complete { id, directory, .. } => return Ok((id, directory)),
            AgentCreationStatus::Unavailable { .. } => {
                return Err(AgentCreationError::Unavailable(
                    "The Entity for this creation key is no longer available; it will not be recreated."
                        .into(),
                ));
            }
            AgentCreationStatus::Absent | AgentCreationStatus::Reserved { .. } => {}
        }

        let record = if let Some(record) = config.creation_attempts.get(key) {
            record.clone()
        } else {
            let record = AgentCreationRecord {
                name: name.to_string(),
                entity_id: Uuid::new_v4().to_string(),
                state: AgentCreationState::Reserved,
            };
            let mut next = config.clone();
            next.creation_attempts
                .insert(key.to_string(), record.clone());
            next.save(&self.data_root)
                .map_err(|error| AgentCreationError::Storage(error.to_string()))?;
            *config = next;
            record
        };

        if config.find_agent(&record.entity_id).is_some() {
            return Err(AgentCreationError::Storage(
                "Reserved Entity ID is already registered.".into(),
            ));
        }
        let directory = self
            .initialize_agent_files(&record.entity_id, &record.name, &config.default_theme)
            .map_err(AgentCreationError::Storage)?;
        let mut next = config.clone();
        next.register_agent(AgentEntry {
            id: record.entity_id.clone(),
            name: record.name.clone(),
            is_hive: false,
            directory: self.registry_directory_for(&record.entity_id),
        })
        .map_err(|error| AgentCreationError::Storage(error.to_string()))?;
        next.creation_attempts.insert(
            key.to_string(),
            AgentCreationRecord {
                state: AgentCreationState::Complete,
                ..record.clone()
            },
        );
        next.save(&self.data_root)
            .map_err(|error| AgentCreationError::Storage(error.to_string()))?;
        *config = next;
        drop(config);
        let _ = self.create_documents_folder(&record.entity_id);
        Ok((record.entity_id, directory))
    }
}

#[cfg(test)]
mod tests {
    use crate::{AgentCreationError, AgentCreationStatus, IdentityManager};
    use abigail_core::global_config::AgentCreationState;
    use abigail_core::GlobalConfig;
    use ed25519_dalek::SigningKey;
    use std::collections::HashSet;
    use std::path::PathBuf;
    use std::sync::{Arc, Barrier, RwLock};
    use uuid::Uuid;

    struct TestRoot(PathBuf);

    impl TestRoot {
        fn new() -> Self {
            let root = std::env::temp_dir().join(format!("abigail-creation-{}", Uuid::new_v4()));
            std::fs::create_dir_all(&root).unwrap();
            Self(root)
        }

        fn manager(&self) -> IdentityManager {
            let config = if GlobalConfig::config_path(&self.0).exists() {
                GlobalConfig::load(&self.0).unwrap()
            } else {
                GlobalConfig::new(&self.0)
            };
            IdentityManager {
                data_root: self.0.clone(),
                global_config: RwLock::new(config),
                master_key: SigningKey::from_bytes(&[9; 32]),
            }
        }
    }

    impl Drop for TestRoot {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }

    #[test]
    fn concurrent_replay_persists_one_complete_receipt_and_survives_restart() {
        let root = TestRoot::new();
        let manager = Arc::new(root.manager());
        let key = Uuid::new_v4().to_string();
        let barrier = Arc::new(Barrier::new(9));
        let workers: Vec<_> = (0..8)
            .map(|_| {
                let manager = manager.clone();
                let key = key.clone();
                let barrier = barrier.clone();
                std::thread::spawn(move || {
                    barrier.wait();
                    manager.create_agent_idempotent("Signal", &key).unwrap()
                })
            })
            .collect();
        barrier.wait();
        let created: Vec<_> = workers
            .into_iter()
            .map(|worker| worker.join().unwrap())
            .collect();
        let ids: HashSet<_> = created.iter().map(|(id, _)| id).collect();
        assert_eq!(ids.len(), 1);

        let config = GlobalConfig::load(&root.0).unwrap();
        assert_eq!(config.agents.len(), 1);
        assert_eq!(config.creation_attempts.len(), 1);
        let receipt = &config.creation_attempts[&key];
        assert_eq!(receipt.entity_id, created[0].0);
        assert_eq!(receipt.state, AgentCreationState::Complete);

        drop(manager);
        assert_eq!(
            root.manager()
                .create_agent_idempotent("Signal", &key)
                .unwrap(),
            created[0]
        );
    }

    #[test]
    fn receipt_name_is_immutable_and_deleted_completion_is_terminal() {
        let root = TestRoot::new();
        let key = Uuid::new_v4().to_string();
        let manager = root.manager();
        let (id, _) = manager.create_agent_idempotent("Signal", &key).unwrap();
        assert!(matches!(
            manager.create_agent_idempotent("Different", &key),
            Err(AgentCreationError::Conflict(_))
        ));
        manager.delete_agent(&id).unwrap();
        drop(manager);

        let reopened = root.manager();
        assert_eq!(
            reopened.entity_creation_status(&key).unwrap(),
            AgentCreationStatus::Unavailable {
                name: "Signal".into(),
                id: id.clone(),
            }
        );
        assert!(matches!(
            reopened.create_agent_idempotent("Signal", &key),
            Err(AgentCreationError::Unavailable(_))
        ));
        let persisted = GlobalConfig::load(&root.0).unwrap();
        assert!(persisted.agents.is_empty());
        assert_eq!(persisted.creation_attempts[&key].entity_id, id);
    }

    #[test]
    fn reservation_is_durable_before_files_and_restart_reuses_its_uuid() {
        let root = TestRoot::new();
        let key = Uuid::new_v4().to_string();
        std::fs::write(root.0.join("identities"), b"temporary obstruction").unwrap();
        assert!(matches!(
            root.manager().create_agent_idempotent("Signal", &key),
            Err(AgentCreationError::Storage(_))
        ));
        let receipt = GlobalConfig::load(&root.0).unwrap().creation_attempts[&key].clone();
        assert_eq!(receipt.state, AgentCreationState::Reserved);
        assert!(GlobalConfig::load(&root.0).unwrap().agents.is_empty());

        std::fs::remove_file(root.0.join("identities")).unwrap();
        let (id, _) = root
            .manager()
            .create_agent_idempotent("Signal", &key)
            .unwrap();
        assert_eq!(id, receipt.entity_id);
    }

    #[test]
    fn global_config_without_creation_attempts_loads_an_empty_ledger() {
        let root = TestRoot::new();
        let mut legacy = serde_json::to_value(GlobalConfig::new(&root.0)).unwrap();
        legacy.as_object_mut().unwrap().remove("creation_attempts");
        std::fs::write(
            GlobalConfig::config_path(&root.0),
            serde_json::to_vec_pretty(&legacy).unwrap(),
        )
        .unwrap();

        let loaded = GlobalConfig::load(&root.0).unwrap();
        assert!(loaded.creation_attempts.is_empty());
    }
}
