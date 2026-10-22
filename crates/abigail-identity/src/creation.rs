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

fn validate_fingerprint(fingerprint: Option<&str>) -> Result<(), AgentCreationError> {
    if fingerprint.is_some_and(|value| {
        value.is_empty() || value.len() > 256 || value.chars().any(char::is_control)
    }) {
        return Err(AgentCreationError::InvalidRequest(
            "Creation request fingerprint is invalid.".into(),
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
        match record.state {
            AgentCreationState::Reserved => Ok(AgentCreationStatus::Reserved {
                name: record.name.clone(),
            }),
            AgentCreationState::Unavailable => Ok(AgentCreationStatus::Unavailable {
                name: record.name.clone(),
                id: record.entity_id.clone(),
            }),
            AgentCreationState::Complete => {
                let directory = self.identities_dir().join(&record.entity_id);
                let registered = config.find_agent(&record.entity_id).is_some_and(|entry| {
                    !entry.is_hive
                        && entry.directory == self.registry_directory_for(&record.entity_id)
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
        }
    }

    fn check_creation_binding(
        record: &AgentCreationRecord,
        name: &str,
        fingerprint: Option<&str>,
    ) -> Result<(), AgentCreationError> {
        if record.name != name {
            return Err(AgentCreationError::Conflict(
                "This creation key belongs to a different Entity name.".into(),
            ));
        }
        if record.request_fingerprint.as_deref() != fingerprint {
            return Err(AgentCreationError::Conflict(
                "This creation key belongs to different original creation input.".into(),
            ));
        }
        Ok(())
    }

    fn reserve_creation_locked(
        &self,
        config: &mut GlobalConfig,
        name: &str,
        key: &str,
        fingerprint: Option<&str>,
    ) -> Result<AgentCreationRecord, AgentCreationError> {
        if let Some(record) = config.creation_attempts.get(key) {
            Self::check_creation_binding(record, name, fingerprint)?;
            return Ok(record.clone());
        }
        let entity_id = Uuid::new_v4().to_string();
        if config.find_agent(&entity_id).is_some()
            || config
                .creation_attempts
                .values()
                .any(|attempt| attempt.entity_id == entity_id)
        {
            return Err(AgentCreationError::Storage(
                "New Entity ID is already in use.".into(),
            ));
        }
        let record = AgentCreationRecord {
            name: name.to_string(),
            entity_id,
            request_fingerprint: fingerprint.map(str::to_owned),
            state: AgentCreationState::Reserved,
        };
        let mut next = config.clone();
        next.creation_attempts
            .insert(key.to_string(), record.clone());
        next.save(&self.data_root)
            .map_err(|error| AgentCreationError::Storage(error.to_string()))?;
        *config = next;
        Ok(record)
    }

    pub fn reserve_agent_creation(
        &self,
        name: &str,
        key: &str,
        fingerprint: Option<&str>,
    ) -> Result<AgentCreationStatus, AgentCreationError> {
        validate_key(key)?;
        validate_name(name)?;
        validate_fingerprint(fingerprint)?;
        let mut config = self
            .global_config
            .write()
            .map_err(|error| AgentCreationError::Storage(error.to_string()))?;
        self.reserve_creation_locked(&mut config, name, key, fingerprint)?;
        let status = self.creation_status_in(&config, key)?;
        if matches!(status, AgentCreationStatus::Unavailable { .. }) {
            return Err(AgentCreationError::Unavailable(
                "The Entity for this creation key is no longer available; it will not be recreated."
                    .into(),
            ));
        }
        Ok(status)
    }

    pub fn create_agent_idempotent(
        &self,
        name: &str,
        key: &str,
    ) -> Result<(String, PathBuf), AgentCreationError> {
        self.create_agent_idempotent_with_fingerprint(name, key, None)
    }

    pub fn create_agent_idempotent_with_fingerprint(
        &self,
        name: &str,
        key: &str,
        fingerprint: Option<&str>,
    ) -> Result<(String, PathBuf), AgentCreationError> {
        validate_key(key)?;
        validate_name(name)?;
        validate_fingerprint(fingerprint)?;
        let mut config = self
            .global_config
            .write()
            .map_err(|error| AgentCreationError::Storage(error.to_string()))?;
        let record = self.reserve_creation_locked(&mut config, name, key, fingerprint)?;
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

    pub fn abandon_agent_creation(&self, key: &str) -> Result<(), AgentCreationError> {
        validate_key(key)?;
        let mut config = self
            .global_config
            .write()
            .map_err(|error| AgentCreationError::Storage(error.to_string()))?;
        let Some(record) = config.creation_attempts.get(key).cloned() else {
            return Ok(());
        };
        if record.state == AgentCreationState::Unavailable {
            return Ok(());
        }
        if record.state != AgentCreationState::Reserved {
            return Err(AgentCreationError::Storage(
                "Only an unfinished Entity creation reservation can be abandoned.".into(),
            ));
        }
        if !Uuid::parse_str(&record.entity_id)
            .is_ok_and(|uuid| uuid.to_string() == record.entity_id)
        {
            return Err(AgentCreationError::Storage(
                "Creation reservation contains an invalid entity ID.".into(),
            ));
        }
        if config.find_agent(&record.entity_id).is_some() {
            return Err(AgentCreationError::Storage(
                "A registered Entity must be deleted before retiring its creation receipt.".into(),
            ));
        }
        let directory = self.identities_dir().join(&record.entity_id);
        match std::fs::symlink_metadata(&directory) {
            Ok(metadata) if metadata.file_type().is_symlink() || !metadata.is_dir() => {
                std::fs::remove_file(&directory)
                    .map_err(|error| AgentCreationError::Storage(error.to_string()))?;
            }
            Ok(_) => {
                std::fs::remove_dir_all(&directory)
                    .map_err(|error| AgentCreationError::Storage(error.to_string()))?;
            }
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
            Err(error) => return Err(AgentCreationError::Storage(error.to_string())),
        }
        let mut next = config.clone();
        next.creation_attempts.get_mut(key).unwrap().state = AgentCreationState::Unavailable;
        next.save(&self.data_root)
            .map_err(|error| AgentCreationError::Storage(error.to_string()))?;
        *config = next;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use crate::{AgentCreationError, AgentCreationStatus, IdentityManager};
    use abigail_core::global_config::{AgentCreationRecord, AgentCreationState};
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
        assert_eq!(
            persisted.creation_attempts[&key].state,
            AgentCreationState::Unavailable
        );
    }

    #[test]
    fn fingerprinted_reservation_is_immutable_and_abandon_is_terminal() {
        let root = TestRoot::new();
        let key = Uuid::new_v4().to_string();
        let manager = root.manager();
        manager
            .reserve_agent_creation("Signal", &key, Some("seed-a"))
            .unwrap();
        manager
            .reserve_agent_creation("Signal", &key, Some("seed-a"))
            .unwrap();
        assert!(matches!(
            manager.reserve_agent_creation("Signal", &key, Some("seed-b")),
            Err(AgentCreationError::Conflict(_))
        ));
        let receipt = manager.global_config.read().unwrap().creation_attempts[&key].clone();
        assert_eq!(receipt.request_fingerprint.as_deref(), Some("seed-a"));
        let partial = manager.identities_dir().join(&receipt.entity_id);
        std::fs::create_dir_all(partial.join("docs")).unwrap();
        std::fs::write(partial.join("unfinished"), b"partial").unwrap();

        manager.abandon_agent_creation(&key).unwrap();
        assert!(!partial.exists());
        assert_eq!(
            manager.entity_creation_status(&key).unwrap(),
            AgentCreationStatus::Unavailable {
                name: "Signal".into(),
                id: receipt.entity_id.clone(),
            }
        );
        assert!(matches!(
            manager.create_agent_idempotent_with_fingerprint("Signal", &key, Some("seed-a")),
            Err(AgentCreationError::Unavailable(_))
        ));
        assert!(matches!(
            manager.reserve_agent_creation("Signal", &key, Some("seed-b")),
            Err(AgentCreationError::Conflict(_))
        ));
    }

    #[test]
    fn none_and_some_fingerprints_conflict_exactly() {
        let root = TestRoot::new();
        let key = Uuid::new_v4().to_string();
        let manager = root.manager();
        manager
            .reserve_agent_creation("Legacy", &key, None)
            .unwrap();
        assert!(matches!(
            manager.reserve_agent_creation("Legacy", &key, Some("sha256:new-protocol")),
            Err(AgentCreationError::Conflict(_))
        ));
        assert!(
            manager.global_config.read().unwrap().creation_attempts[&key]
                .request_fingerprint
                .is_none()
        );
    }

    #[test]
    fn abandon_refuses_complete_damaged_receipt_and_preserves_all_files() {
        let root = TestRoot::new();
        let key = Uuid::new_v4().to_string();
        let manager = root.manager();
        let (entity_id, directory) = manager
            .create_agent_idempotent("Possibly born", &key)
            .unwrap();
        let sentinel = directory.join("docs").join("keep-me");
        std::fs::write(&sentinel, b"irreplaceable").unwrap();
        {
            let mut config = manager.global_config.write().unwrap();
            config.remove_agent(&entity_id);
            config.save(&root.0).unwrap();
        }

        assert!(matches!(
            manager.entity_creation_status(&key).unwrap(),
            AgentCreationStatus::Unavailable { .. }
        ));
        assert!(manager.abandon_agent_creation(&key).is_err());
        assert_eq!(std::fs::read(&sentinel).unwrap(), b"irreplaceable");
        assert_eq!(
            GlobalConfig::load(&root.0).unwrap().creation_attempts[&key].state,
            AgentCreationState::Complete
        );
    }

    #[test]
    fn abandon_rejects_malformed_stored_uuid_before_path_resolution() {
        let root = TestRoot::new();
        let key = Uuid::new_v4().to_string();
        let manager = root.manager();
        std::fs::create_dir_all(manager.identities_dir()).unwrap();
        let outside = root.0.join("outside");
        std::fs::create_dir_all(&outside).unwrap();
        std::fs::write(outside.join("keep-me"), b"safe").unwrap();
        {
            let mut config = manager.global_config.write().unwrap();
            config.creation_attempts.insert(
                key.clone(),
                AgentCreationRecord {
                    name: "Malformed".into(),
                    entity_id: "../outside".into(),
                    request_fingerprint: None,
                    state: AgentCreationState::Reserved,
                },
            );
            config.save(&root.0).unwrap();
        }

        assert!(manager.abandon_agent_creation(&key).is_err());
        assert_eq!(std::fs::read(outside.join("keep-me")).unwrap(), b"safe");
        assert_eq!(
            GlobalConfig::load(&root.0).unwrap().creation_attempts[&key].state,
            AgentCreationState::Reserved
        );
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

    #[test]
    fn legacy_receipt_without_fingerprint_remains_generic_and_loadable() {
        let root = TestRoot::new();
        let key = Uuid::new_v4().to_string();
        let manager = root.manager();
        manager
            .reserve_agent_creation("Legacy", &key, None)
            .unwrap();
        let mut legacy = serde_json::to_value(GlobalConfig::load(&root.0).unwrap()).unwrap();
        legacy["creation_attempts"][&key]
            .as_object_mut()
            .unwrap()
            .remove("request_fingerprint");
        std::fs::write(
            GlobalConfig::config_path(&root.0),
            serde_json::to_vec_pretty(&legacy).unwrap(),
        )
        .unwrap();

        let loaded = GlobalConfig::load(&root.0).unwrap();
        assert!(loaded.creation_attempts[&key].request_fingerprint.is_none());
        drop(manager);
        root.manager()
            .create_agent_idempotent("Legacy", &key)
            .unwrap();
    }
}
