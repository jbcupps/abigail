//! Idempotent entity creation for the single Hive owner of a data root.
//!
//! The registry lock serializes local requests. Its atomic file stores both the
//! reservation and final registration, so a process restart cannot allocate a
//! second ID for the same key. Independent Hives sharing a root are unsupported.

use crate::IdentityManager;
use abigail_core::global_config::{AgentCreationRecord, AgentCreationState};
use abigail_core::{AgentEntry, GlobalConfig};
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
        let message = match self {
            Self::InvalidRequest(message)
            | Self::Conflict(message)
            | Self::Unavailable(message)
            | Self::Storage(message) => message,
        };
        formatter.write_str(message)
    }
}

impl std::error::Error for AgentCreationError {}

fn canonical_uuid(value: &str) -> bool {
    Uuid::parse_str(value).is_ok_and(|uuid| uuid.to_string() == value)
}

fn validate_key(key: &str) -> Result<(), AgentCreationError> {
    if !canonical_uuid(key) {
        return Err(AgentCreationError::InvalidRequest(
            "creation_key must be a canonical UUID.".to_string(),
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
                .to_string(),
        ));
    }
    Ok(())
}

impl IdentityManager {
    /// Inert fixture for ordinary creation tests; never initializes a vault or Hive.
    /// This fixed synthetic key must not be used by application startup.
    #[cfg(any(test, feature = "test-fixtures"))]
    #[doc(hidden)]
    pub fn isolated_creation_test_manager(data_root: PathBuf) -> Result<Self, String> {
        let temporary_root = std::env::temp_dir();
        if !data_root.is_absolute()
            || !data_root.starts_with(&temporary_root)
            || data_root == temporary_root
        {
            return Err("Creation fixture requires an absolute temporary data root.".to_string());
        }
        if std::env::var_os("ABIGAIL_DOCUMENTS_DIR").is_some() {
            return Err("Creation fixture requires ABIGAIL_DOCUMENTS_DIR to be unset.".to_string());
        }
        std::fs::create_dir_all(&data_root).map_err(|error| error.to_string())?;
        let global_config = if GlobalConfig::config_path(&data_root).is_file() {
            GlobalConfig::load(&data_root).map_err(|error| error.to_string())?
        } else {
            GlobalConfig::new(&data_root)
        };
        Ok(Self {
            data_root,
            global_config: std::sync::RwLock::new(global_config),
            master_key: ed25519_dalek::SigningKey::from_bytes(&[9; 32]),
        })
    }

    /// Inspect an exact key without creating or repairing anything.
    pub fn entity_creation_status(
        &self,
        key: &str,
    ) -> Result<AgentCreationStatus, AgentCreationError> {
        validate_key(key)?;
        let gc = self
            .global_config
            .read()
            .map_err(|error| AgentCreationError::Storage(error.to_string()))?;
        self.creation_status_in(&gc, key)
    }

    fn creation_status_in(
        &self,
        gc: &GlobalConfig,
        key: &str,
    ) -> Result<AgentCreationStatus, AgentCreationError> {
        let Some(record) = gc.creation_attempts.get(key) else {
            return Ok(AgentCreationStatus::Absent);
        };
        if !canonical_uuid(&record.entity_id) {
            return Err(AgentCreationError::Storage(
                "Creation reservation contains an invalid entity ID.".to_string(),
            ));
        }
        if record.state == AgentCreationState::Reserved {
            return Ok(AgentCreationStatus::Reserved {
                name: record.name.clone(),
            });
        }
        let directory = self.identities_dir().join(&record.entity_id);
        let available = gc.find_agent(&record.entity_id).is_some_and(|entry| {
            !entry.is_hive && entry.directory == self.registry_directory_for(&record.entity_id)
        }) && directory.join("config.json").is_file();
        if available {
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

    /// Resume or return one entity for an immutable client key/name pair.
    /// Failures after reservation leave that same ID available for safe replay.
    pub fn create_agent_idempotent(
        &self,
        name: &str,
        key: &str,
    ) -> Result<(String, PathBuf), AgentCreationError> {
        validate_key(key)?;
        validate_name(name)?;
        let mut gc = self
            .global_config
            .write()
            .map_err(|error| AgentCreationError::Storage(error.to_string()))?;
        if let Some(record) = gc.creation_attempts.get(key) {
            if record.name != name {
                return Err(AgentCreationError::Conflict(
                    "This creation key belongs to a different entity name.".to_string(),
                ));
            }
        }
        match self.creation_status_in(&gc, key)? {
            AgentCreationStatus::Complete { id, directory, .. } => return Ok((id, directory)),
            AgentCreationStatus::Unavailable { .. } => {
                return Err(AgentCreationError::Unavailable(
                    "The entity for this creation key is no longer available; it will not be recreated."
                        .to_string(),
                ))
            }
            AgentCreationStatus::Absent | AgentCreationStatus::Reserved { .. } => {}
        }
        let record = self.reserve_creation(&mut gc, key, name)?;
        let result = self.finish_creation(&mut gc, key, &record)?;
        drop(gc);
        let _ = self.create_documents_folder(&result.0);
        Ok(result)
    }

    fn reserve_creation(
        &self,
        gc: &mut GlobalConfig,
        key: &str,
        name: &str,
    ) -> Result<AgentCreationRecord, AgentCreationError> {
        if let Some(record) = gc.creation_attempts.get(key) {
            return Ok(record.clone());
        }
        let entity_id = Uuid::new_v4().to_string();
        let directory = self.identities_dir().join(&entity_id);
        if gc.find_agent(&entity_id).is_some()
            || gc
                .creation_attempts
                .values()
                .any(|attempt| attempt.entity_id == entity_id)
            || directory
                .try_exists()
                .map_err(|error| AgentCreationError::Storage(error.to_string()))?
        {
            return Err(AgentCreationError::Storage(
                "New entity ID is already in use.".to_string(),
            ));
        }
        let record = AgentCreationRecord {
            name: name.to_string(),
            entity_id,
            state: AgentCreationState::Reserved,
        };
        let mut next = gc.clone();
        next.creation_attempts
            .insert(key.to_string(), record.clone());
        next.save(&self.data_root)
            .map_err(|error| AgentCreationError::Storage(error.to_string()))?;
        *gc = next;
        Ok(record)
    }

    fn finish_creation(
        &self,
        gc: &mut GlobalConfig,
        key: &str,
        record: &AgentCreationRecord,
    ) -> Result<(String, PathBuf), AgentCreationError> {
        // A reserved ID cannot already be registered: registration and completion
        // share one atomic write. Reject inconsistent data rather than overwrite it.
        if gc.find_agent(&record.entity_id).is_some() {
            return Err(AgentCreationError::Storage(
                "Reserved entity ID is already registered.".to_string(),
            ));
        }
        self.initialize_agent_files(&record.entity_id, &record.name, &gc.default_theme)
            .map_err(AgentCreationError::Storage)?;
        let mut next = gc.clone();
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
        *gc = next;
        Ok((
            record.entity_id.clone(),
            self.identities_dir().join(&record.entity_id),
        ))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use abigail_core::AppConfig;
    use std::path::Path;
    use std::sync::{Arc, Barrier};

    // No vault, credentials, Hive bootstrap, provider, or user Documents access.
    struct TestRoot(PathBuf);

    impl TestRoot {
        fn new() -> Self {
            let path = std::env::temp_dir().join(format!("abigail-creation-{}", Uuid::new_v4()));
            std::fs::create_dir_all(&path).unwrap();
            Self(path)
        }

        fn manager(&self) -> IdentityManager {
            IdentityManager::isolated_creation_test_manager(self.0.clone()).unwrap()
        }
    }

    impl Drop for TestRoot {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }

    fn reserve(manager: &IdentityManager, key: &str) -> AgentCreationRecord {
        let mut gc = manager.global_config.write().unwrap();
        manager.reserve_creation(&mut gc, key, "Signal").unwrap()
    }

    fn config_bytes(directory: &Path) -> Vec<u8> {
        std::fs::read(directory.join("config.json")).unwrap()
    }

    #[test]
    fn absent_lookup_does_not_write_or_allocate() {
        let root = TestRoot::new();
        let manager = root.manager();
        assert_eq!(
            manager
                .entity_creation_status(&Uuid::new_v4().to_string())
                .unwrap(),
            AgentCreationStatus::Absent
        );
        assert!(!GlobalConfig::config_path(&root.0).exists());
        assert!(!manager.identities_dir().exists());
    }

    #[test]
    fn concurrent_replay_allocates_exactly_one_entity() {
        let root = TestRoot::new();
        let manager = Arc::new(root.manager());
        let barrier = Arc::new(Barrier::new(8));
        let key = Uuid::new_v4().to_string();
        let handles: Vec<_> = (0..8)
            .map(|_| {
                let manager = Arc::clone(&manager);
                let barrier = Arc::clone(&barrier);
                let key = key.clone();
                std::thread::spawn(move || {
                    barrier.wait();
                    manager.create_agent_idempotent("Signal", &key).unwrap()
                })
            })
            .collect();
        let results: Vec<_> = handles
            .into_iter()
            .map(|thread| thread.join().unwrap())
            .collect();
        assert!(results.iter().all(|result| result == &results[0]));
        let gc = GlobalConfig::load(&root.0).unwrap();
        assert_eq!(gc.agents.len(), 1);
        assert_eq!(gc.creation_attempts.len(), 1);
        assert_eq!(
            std::fs::read_dir(manager.identities_dir()).unwrap().count(),
            1
        );
    }

    #[test]
    fn completed_replay_survives_restart_and_does_not_overwrite_config() {
        let root = TestRoot::new();
        let key = Uuid::new_v4().to_string();
        let first = root
            .manager()
            .create_agent_idempotent("Signal", &key)
            .unwrap();
        let mut config = AppConfig::load(&first.1.join("config.json")).unwrap();
        config.primary_color = Some("custom".to_string());
        config.save(&first.1.join("config.json")).unwrap();
        let before = config_bytes(&first.1);
        let restarted = root.manager();
        assert_eq!(
            restarted.create_agent_idempotent("Signal", &key).unwrap(),
            first
        );
        assert_eq!(config_bytes(&first.1), before);
        assert!(
            matches!(restarted.entity_creation_status(&key).unwrap(), AgentCreationStatus::Complete { id, .. } if id == first.0)
        );
    }

    #[test]
    fn reservation_before_files_survives_restart() {
        let root = TestRoot::new();
        let key = Uuid::new_v4().to_string();
        let record = reserve(&root.manager(), &key);
        assert!(!root.0.join("identities").exists());
        let restarted = root.manager();
        assert_eq!(
            restarted.entity_creation_status(&key).unwrap(),
            AgentCreationStatus::Reserved {
                name: "Signal".to_string()
            }
        );
        let result = restarted.create_agent_idempotent("Signal", &key).unwrap();
        assert_eq!(result.0, record.entity_id);
        assert_eq!(GlobalConfig::load(&root.0).unwrap().agents.len(), 1);
    }

    #[test]
    fn initialized_reservation_replays_without_overwriting() {
        let root = TestRoot::new();
        let key = Uuid::new_v4().to_string();
        let manager = root.manager();
        let record = reserve(&manager, &key);
        manager
            .initialize_agent_files(&record.entity_id, "Signal", "phosphor")
            .unwrap();
        let directory = manager.identities_dir().join(&record.entity_id);
        let before = config_bytes(&directory);
        let result = root
            .manager()
            .create_agent_idempotent("Signal", &key)
            .unwrap();
        assert_eq!(result.0, record.entity_id);
        assert_eq!(config_bytes(&directory), before);
    }

    #[test]
    fn conflicting_name_is_rejected_before_and_after_completion() {
        let root = TestRoot::new();
        let key = Uuid::new_v4().to_string();
        let manager = root.manager();
        reserve(&manager, &key);
        assert!(matches!(
            manager.create_agent_idempotent("Other", &key),
            Err(AgentCreationError::Conflict(_))
        ));
        manager.create_agent_idempotent("Signal", &key).unwrap();
        assert!(matches!(
            manager.create_agent_idempotent("Other", &key),
            Err(AgentCreationError::Conflict(_))
        ));
        assert_eq!(GlobalConfig::load(&root.0).unwrap().agents.len(), 1);
    }

    #[test]
    fn deleted_completion_is_terminal_after_restart() {
        let root = TestRoot::new();
        let key = Uuid::new_v4().to_string();
        let manager = root.manager();
        let (id, directory) = manager.create_agent_idempotent("Signal", &key).unwrap();
        manager.delete_agent(&id).unwrap();
        let restarted = root.manager();
        assert_eq!(
            restarted.entity_creation_status(&key).unwrap(),
            AgentCreationStatus::Unavailable {
                name: "Signal".to_string(),
                id
            }
        );
        assert!(matches!(
            restarted.create_agent_idempotent("Signal", &key),
            Err(AgentCreationError::Unavailable(_))
        ));
        assert!(!directory.exists());
        assert_eq!(GlobalConfig::load(&root.0).unwrap().agents.len(), 0);
    }

    #[test]
    fn missing_completed_config_is_not_recreated() {
        let root = TestRoot::new();
        let key = Uuid::new_v4().to_string();
        let manager = root.manager();
        let (_, directory) = manager.create_agent_idempotent("Signal", &key).unwrap();
        std::fs::remove_file(directory.join("config.json")).unwrap();
        assert!(matches!(
            manager.create_agent_idempotent("Signal", &key),
            Err(AgentCreationError::Unavailable(_))
        ));
        assert!(!directory.join("config.json").exists());
    }

    #[test]
    fn reservation_write_failure_does_not_publish_or_initialize() {
        let root = TestRoot::new();
        let manager = root.manager();
        std::fs::create_dir(GlobalConfig::config_path(&root.0)).unwrap();
        let key = Uuid::new_v4().to_string();
        assert!(matches!(
            manager.create_agent_idempotent("Signal", &key),
            Err(AgentCreationError::Storage(_))
        ));
        assert!(manager
            .global_config
            .read()
            .unwrap()
            .creation_attempts
            .is_empty());
        assert!(!manager.identities_dir().exists());
    }

    #[test]
    fn final_registry_write_failure_leaves_only_a_reservation_for_replay() {
        let root = TestRoot::new();
        let manager = root.manager();
        let key = Uuid::new_v4().to_string();
        let record = reserve(&manager, &key);
        let registry_path = GlobalConfig::config_path(&root.0);
        let saved = std::fs::read(&registry_path).unwrap();
        std::fs::remove_file(&registry_path).unwrap();
        std::fs::create_dir(&registry_path).unwrap();
        {
            let mut gc = manager.global_config.write().unwrap();
            assert!(matches!(
                manager.finish_creation(&mut gc, &key, &record),
                Err(AgentCreationError::Storage(_))
            ));
            assert!(gc.agents.is_empty());
            assert_eq!(
                gc.creation_attempts[&key].state,
                AgentCreationState::Reserved
            );
        }
        std::fs::remove_dir(&registry_path).unwrap();
        std::fs::write(&registry_path, saved).unwrap();
        let (id, _) = root
            .manager()
            .create_agent_idempotent("Signal", &key)
            .unwrap();
        assert_eq!(id, record.entity_id);
        assert_eq!(GlobalConfig::load(&root.0).unwrap().agents.len(), 1);
    }

    #[test]
    fn invalid_partial_config_stays_untouched_and_keeps_reservation() {
        let root = TestRoot::new();
        let manager = root.manager();
        let key = Uuid::new_v4().to_string();
        let record = reserve(&manager, &key);
        let directory = manager.identities_dir().join(&record.entity_id);
        std::fs::create_dir_all(&directory).unwrap();
        std::fs::write(directory.join("config.json"), b"partial").unwrap();
        assert!(matches!(
            manager.create_agent_idempotent("Signal", &key),
            Err(AgentCreationError::Storage(_))
        ));
        assert_eq!(config_bytes(&directory), b"partial");
        assert_eq!(
            GlobalConfig::load(&root.0).unwrap().creation_attempts[&key].state,
            AgentCreationState::Reserved
        );
        assert!(GlobalConfig::load(&root.0).unwrap().agents.is_empty());
    }

    #[test]
    fn mismatched_partial_config_is_never_adopted_or_overwritten() {
        let root = TestRoot::new();
        let manager = root.manager();
        let key = Uuid::new_v4().to_string();
        let record = reserve(&manager, &key);
        manager
            .initialize_agent_files(&record.entity_id, "Someone else", "modern")
            .unwrap();
        let directory = manager.identities_dir().join(&record.entity_id);
        let before = config_bytes(&directory);
        assert!(matches!(
            manager.create_agent_idempotent("Signal", &key),
            Err(AgentCreationError::Storage(_))
        ));
        assert_eq!(config_bytes(&directory), before);
        assert!(GlobalConfig::load(&root.0).unwrap().agents.is_empty());
    }

    #[test]
    fn file_initialization_failure_keeps_the_reserved_id_for_restart_retry() {
        let root = TestRoot::new();
        let manager = root.manager();
        let key = Uuid::new_v4().to_string();
        let record = reserve(&manager, &key);
        let directory = manager.identities_dir().join(&record.entity_id);
        std::fs::create_dir_all(manager.identities_dir()).unwrap();
        std::fs::write(&directory, b"temporary obstruction").unwrap();
        assert!(matches!(
            manager.create_agent_idempotent("Signal", &key),
            Err(AgentCreationError::Storage(_))
        ));
        let gc = GlobalConfig::load(&root.0).unwrap();
        assert!(gc.agents.is_empty());
        assert_eq!(
            gc.creation_attempts[&key].state,
            AgentCreationState::Reserved
        );
        std::fs::remove_file(&directory).unwrap();
        assert_eq!(
            root.manager()
                .create_agent_idempotent("Signal", &key)
                .unwrap()
                .0,
            record.entity_id
        );
    }

    #[test]
    fn invalid_key_or_name_never_reserves() {
        let root = TestRoot::new();
        let manager = root.manager();
        for key in ["bad", "", "AAAAAAAA-AAAA-4AAA-AAAA-AAAAAAAAAAAA"] {
            assert!(matches!(
                manager.create_agent_idempotent("Signal", key),
                Err(AgentCreationError::InvalidRequest(_))
            ));
            assert!(matches!(
                manager.entity_creation_status(key),
                Err(AgentCreationError::InvalidRequest(_))
            ));
        }
        for name in [
            "".to_string(),
            " Signal".to_string(),
            "Signal\n".to_string(),
            "x".repeat(81),
        ] {
            assert!(matches!(
                manager.create_agent_idempotent(&name, &Uuid::new_v4().to_string()),
                Err(AgentCreationError::InvalidRequest(_))
            ));
        }
        assert!(!GlobalConfig::config_path(&root.0).exists());
    }

    #[test]
    fn old_registry_and_unkeyed_creation_remain_compatible() {
        let root = TestRoot::new();
        let mut old = serde_json::to_value(GlobalConfig::new(&root.0)).unwrap();
        old.as_object_mut().unwrap().remove("creation_attempts");
        std::fs::write(
            GlobalConfig::config_path(&root.0),
            serde_json::to_vec(&old).unwrap(),
        )
        .unwrap();
        let manager = root.manager();
        let first = manager.create_agent("Legacy").unwrap();
        let second = manager.create_agent("Legacy").unwrap();
        assert_ne!(first.0, second.0);
        let gc = GlobalConfig::load(&root.0).unwrap();
        assert_eq!(gc.agents.len(), 2);
        assert!(gc.creation_attempts.is_empty());
    }
}
