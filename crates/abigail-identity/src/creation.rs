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
            profile: None,
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

    fn seed_ongoing_files(directory: &Path) {
        let mut config = AppConfig::load(&directory.join("config.json")).unwrap();
        config.primary_color = Some("#c7a86c".to_string());
        config.save(&directory.join("config.json")).unwrap();
        std::fs::create_dir_all(directory.join("docs/notes")).unwrap();
        std::fs::write(
            directory.join("docs/notes/continuity.md"),
            b"Retain this synthetic ongoing Entity note across updates.\n",
        )
        .unwrap();
        std::fs::write(
            directory.join("presence.json"),
            serde_json::to_vec(&serde_json::json!({
                "name": "Ongoing",
                "purpose": "Retain this synthetic presentation brief.",
                "temperament": "curious",
                "boundaries": "Ask before changing saved work.",
                "identity": "lumen"
            }))
            .unwrap(),
        )
        .unwrap();
    }

    fn snapshot_files(directory: &Path) -> std::collections::BTreeMap<PathBuf, Vec<u8>> {
        fn visit(
            root: &Path,
            directory: &Path,
            files: &mut std::collections::BTreeMap<PathBuf, Vec<u8>>,
        ) {
            for entry in std::fs::read_dir(directory).unwrap() {
                let entry = entry.unwrap();
                let path = entry.path();
                if entry.file_type().unwrap().is_dir() {
                    visit(root, &path, files);
                } else {
                    files.insert(
                        path.strip_prefix(root).unwrap().to_path_buf(),
                        std::fs::read(path).unwrap(),
                    );
                }
            }
        }
        let mut files = std::collections::BTreeMap::new();
        visit(directory, directory, &mut files);
        files
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
    fn reserved_future_config_refusal_preserves_files_and_creation_receipt() {
        let root = TestRoot::new();
        let manager = root.manager();
        let key = Uuid::new_v4().to_string();
        let record = reserve(&manager, &key);
        manager
            .initialize_agent_files(&record.entity_id, "Signal", "phosphor")
            .unwrap();
        let directory = manager.identities_dir().join(&record.entity_id);
        let mut config: serde_json::Value =
            serde_json::from_slice(&config_bytes(&directory)).unwrap();
        config["schema_version"] = serde_json::json!(abigail_core::CONFIG_SCHEMA_VERSION + 1);
        config["future_fixture_field"] = serde_json::json!({"must_remain": "unchanged"});
        std::fs::write(
            directory.join("config.json"),
            serde_json::to_vec_pretty(&config).unwrap(),
        )
        .unwrap();
        let before_files = snapshot_files(&root.0);
        let before_registry =
            serde_json::to_value(&*manager.global_config.read().unwrap()).unwrap();

        for manager in [&manager, &root.manager()] {
            assert!(matches!(
                manager.create_agent_idempotent("Signal", &key),
                Err(AgentCreationError::Storage(_))
            ));
            assert_eq!(
                manager.entity_creation_status(&key).unwrap(),
                AgentCreationStatus::Reserved {
                    name: "Signal".to_string()
                }
            );
            assert_eq!(
                serde_json::to_value(&*manager.global_config.read().unwrap()).unwrap(),
                before_registry
            );
            assert_eq!(snapshot_files(&root.0), before_files);
        }
    }

    #[test]
    fn reserved_legacy_config_replay_validates_without_migrating_its_bytes() {
        let root = TestRoot::new();
        let manager = root.manager();
        let key = Uuid::new_v4().to_string();
        let record = reserve(&manager, &key);
        manager
            .initialize_agent_files(&record.entity_id, "Signal", "phosphor")
            .unwrap();
        let directory = manager.identities_dir().join(&record.entity_id);
        let mut config: serde_json::Value =
            serde_json::from_slice(&config_bytes(&directory)).unwrap();
        config["schema_version"] = serde_json::json!(0);
        let legacy_bytes = serde_json::to_vec_pretty(&config).unwrap();
        std::fs::write(directory.join("config.json"), &legacy_bytes).unwrap();

        let (id, _) = manager.create_agent_idempotent("Signal", &key).unwrap();
        assert_eq!(id, record.entity_id);
        assert_eq!(config_bytes(&directory), legacy_bytes);
        assert_eq!(
            GlobalConfig::load(&root.0).unwrap().creation_attempts[&key].state,
            AgentCreationState::Complete
        );
    }

    #[test]
    fn restore_backup_config_refusal_preserves_source_directory_and_registry() {
        for invalid in ["future", "invalid version", "malformed json"] {
            let root = TestRoot::new();
            let manager = root.manager();
            let ongoing_key = Uuid::new_v4().to_string();
            manager
                .create_agent_idempotent("Ongoing fixture", &ongoing_key)
                .unwrap();
            let archived_id = Uuid::new_v4().to_string();
            manager
                .initialize_agent_files(&archived_id, "Archived fixture", "phosphor")
                .unwrap();
            let backup = root.0.join("backups/unsupported-fixture");
            std::fs::create_dir_all(backup.parent().unwrap()).unwrap();
            std::fs::rename(manager.identities_dir().join(&archived_id), &backup).unwrap();
            let mut config: serde_json::Value =
                serde_json::from_slice(&config_bytes(&backup)).unwrap();
            config["schema_version"] = match invalid {
                "future" => serde_json::json!(abigail_core::CONFIG_SCHEMA_VERSION + 1),
                _ => serde_json::json!("invalid"),
            };
            let invalid_bytes = if invalid == "malformed json" {
                b"{\"schema_version\":".to_vec()
            } else {
                serde_json::to_vec_pretty(&config).unwrap()
            };
            std::fs::write(backup.join("config.json"), invalid_bytes).unwrap();
            std::fs::write(
                backup.join("backup_metadata.json"),
                b"{\"fixture\":\"retain original backup\"}",
            )
            .unwrap();
            std::fs::write(
                backup.join("docs/synthetic-note.md"),
                b"Keep this fixture note.",
            )
            .unwrap();
            let before_files = snapshot_files(&root.0);
            let before_registry =
                serde_json::to_value(&*manager.global_config.read().unwrap()).unwrap();
            let before_identity_dirs: std::collections::BTreeSet<_> =
                std::fs::read_dir(manager.identities_dir())
                    .unwrap()
                    .map(|entry| entry.unwrap().path())
                    .collect();

            assert!(
                manager.restore_backup("unsupported-fixture").is_err(),
                "restore must refuse {invalid}"
            );
            assert!(backup.is_dir(), "backup source must remain for {invalid}");
            assert_eq!(snapshot_files(&root.0), before_files, "{invalid}");
            assert_eq!(
                serde_json::to_value(&*manager.global_config.read().unwrap()).unwrap(),
                before_registry,
                "{invalid}"
            );
            let after_identity_dirs: std::collections::BTreeSet<_> =
                std::fs::read_dir(manager.identities_dir())
                    .unwrap()
                    .map(|entry| entry.unwrap().path())
                    .collect();
            assert_eq!(after_identity_dirs, before_identity_dirs, "{invalid}");
        }
    }

    #[test]
    fn restore_supported_unsigned_backup_rebases_paths_and_preserves_saved_data() {
        for schema_version in [0, abigail_core::CONFIG_SCHEMA_VERSION] {
            let root = TestRoot::new();
            let manager = root.manager();
            let ongoing_key = Uuid::new_v4().to_string();
            let (ongoing_id, ongoing_dir) = manager
                .create_agent_idempotent("Ongoing fixture", &ongoing_key)
                .unwrap();
            let ongoing_files = snapshot_files(&ongoing_dir);
            let before_registry = GlobalConfig::load(&root.0).unwrap();
            let archived_id = Uuid::new_v4().to_string();
            manager
                .initialize_agent_files(&archived_id, "Archived fixture", "phosphor")
                .unwrap();
            let backup = root.0.join("backups/supported-fixture");
            std::fs::create_dir_all(backup.parent().unwrap()).unwrap();
            std::fs::rename(manager.identities_dir().join(&archived_id), &backup).unwrap();
            assert!(!backup.join("external_pubkey.bin").exists());
            let mut config: serde_json::Value =
                serde_json::from_slice(&config_bytes(&backup)).unwrap();
            config["schema_version"] = serde_json::json!(schema_version);
            std::fs::write(
                backup.join("config.json"),
                serde_json::to_vec_pretty(&config).unwrap(),
            )
            .unwrap();
            std::fs::write(
                backup.join("backup_metadata.json"),
                b"{\"fixture\":\"supported unsigned backup\"}",
            )
            .unwrap();
            let note = b"Synthetic archived note survives restore.";
            std::fs::write(backup.join("docs/synthetic-note.md"), note).unwrap();

            let restored_id = manager.restore_backup("supported-fixture").unwrap();
            assert_ne!(restored_id, archived_id);
            assert_ne!(restored_id, ongoing_id);
            assert!(!backup.exists());
            let restored_dir = manager.identities_dir().join(&restored_id);
            let restored = AppConfig::from_json_compatible(
                std::str::from_utf8(&config_bytes(&restored_dir)).unwrap(),
            )
            .unwrap();
            assert_eq!(restored.schema_version, abigail_core::CONFIG_SCHEMA_VERSION);
            assert_eq!(restored.agent_name.as_deref(), Some("Archived fixture"));
            assert_eq!(restored.data_dir, restored_dir);
            assert_eq!(restored.models_dir, restored_dir.join("models"));
            assert_eq!(restored.docs_dir, restored_dir.join("docs"));
            assert_eq!(restored.db_path, manager.shared_memory_db_path());
            assert_eq!(
                std::fs::read(restored_dir.join("docs/synthetic-note.md")).unwrap(),
                note
            );
            assert!(!restored_dir.join("backup_metadata.json").exists());
            assert!(!restored_dir.join("signature.sig").exists());
            assert_eq!(snapshot_files(&ongoing_dir), ongoing_files);
            let after_registry = GlobalConfig::load(&root.0).unwrap();
            assert_eq!(after_registry.agents.len(), 2);
            assert_eq!(
                serde_json::to_value(&after_registry.creation_attempts).unwrap(),
                serde_json::to_value(&before_registry.creation_attempts).unwrap()
            );
            let restored_entry = after_registry.find_agent(&restored_id).unwrap();
            assert_eq!(restored_entry.name, "Archived fixture");
            assert!(!restored_entry.is_hive);
            assert_eq!(
                restored_entry.directory,
                manager.registry_directory_for(&restored_id)
            );
        }
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

    #[test]
    fn populated_legacy_registry_preserves_entities_and_settings_when_adding_creation_ledger() {
        let root = TestRoot::new();
        let manager = root.manager();
        let (ongoing_id, ongoing_dir) = manager.create_agent("Ongoing").unwrap();
        let (other_id, _) = manager.create_agent("Other existing Entity").unwrap();
        seed_ongoing_files(&ongoing_dir);
        let ongoing_files = snapshot_files(&ongoing_dir);
        let mut old = serde_json::to_value(GlobalConfig::load(&root.0).unwrap()).unwrap();
        old.as_object_mut().unwrap().remove("creation_attempts");
        old["default_theme"] = serde_json::json!("phosphor");
        let existing_entries = old["agents"].clone();
        let legacy_bytes = serde_json::to_vec_pretty(&old).unwrap();
        std::fs::write(GlobalConfig::config_path(&root.0), &legacy_bytes).unwrap();
        drop(manager);

        let upgraded = root.manager();
        {
            let gc = upgraded.global_config.read().unwrap();
            assert!(gc.creation_attempts.is_empty());
            assert_eq!(serde_json::to_value(&gc.agents).unwrap(), existing_entries);
            assert_eq!(gc.default_theme, "phosphor");
        }
        // Opening a populated legacy registry must not rewrite it.
        assert_eq!(
            std::fs::read(GlobalConfig::config_path(&root.0)).unwrap(),
            legacy_bytes
        );
        let key = Uuid::new_v4().to_string();
        let (disposable_id, _) = upgraded
            .create_agent_idempotent("Disposable", &key)
            .unwrap();
        assert_ne!(disposable_id, ongoing_id);
        assert_ne!(disposable_id, other_id);
        drop(upgraded);

        let reopened = root.manager();
        let gc = reopened.global_config.read().unwrap();
        let mut after = serde_json::to_value(&*gc).unwrap();
        assert_eq!(gc.agents.len(), 3);
        assert_eq!(
            after["agents"].as_array().unwrap()[..2],
            existing_entries.as_array().unwrap()[..]
        );
        // The first ordinary mutation adds only its Entity and creation ledger.
        after["agents"] = existing_entries;
        after.as_object_mut().unwrap().remove("creation_attempts");
        assert_eq!(after, old);
        assert_eq!(snapshot_files(&ongoing_dir), ongoing_files);
    }

    #[test]
    fn disposable_same_name_lifecycle_preserves_ongoing_uuid_and_saved_files() {
        let root = TestRoot::new();
        let ongoing_key = Uuid::new_v4().to_string();
        let disposable_key = Uuid::new_v4().to_string();
        let manager = root.manager();
        let ongoing = manager
            .create_agent_idempotent("Ongoing", &ongoing_key)
            .unwrap();
        seed_ongoing_files(&ongoing.1);
        let ongoing_files = snapshot_files(&ongoing.1);
        let disposable = manager
            .create_agent_idempotent("Ongoing", &disposable_key)
            .unwrap();
        assert_ne!(ongoing.0, disposable.0);
        assert_ne!(ongoing.1, disposable.1);
        drop(manager);

        let reopened = root.manager();
        assert_eq!(
            reopened
                .create_agent_idempotent("Ongoing", &disposable_key)
                .unwrap(),
            disposable
        );
        reopened.delete_agent(&disposable.0).unwrap();
        assert!(!disposable.1.exists());
        assert_eq!(snapshot_files(&ongoing.1), ongoing_files);
        drop(reopened);

        let reopened = root.manager();
        assert_eq!(
            reopened.entity_creation_status(&disposable_key).unwrap(),
            AgentCreationStatus::Unavailable {
                name: "Ongoing".to_string(),
                id: disposable.0.clone(),
            }
        );
        assert!(matches!(
            reopened.create_agent_idempotent("Ongoing", &disposable_key),
            Err(AgentCreationError::Unavailable(_))
        ));
        assert_eq!(
            reopened
                .create_agent_idempotent("Ongoing", &ongoing_key)
                .unwrap(),
            ongoing
        );
        let next = reopened
            .create_agent_idempotent("Ongoing", &Uuid::new_v4().to_string())
            .unwrap();
        assert_ne!(next.0, ongoing.0);
        assert_ne!(next.0, disposable.0);
        reopened.delete_agent(&next.0).unwrap();
        drop(reopened);

        let final_manager = root.manager();
        let agents = final_manager.list_agents().unwrap();
        assert_eq!(agents.len(), 1);
        assert_eq!(agents[0].id, ongoing.0);
        assert_eq!(agents[0].name, "Ongoing");
        assert_eq!(snapshot_files(&ongoing.1), ongoing_files);
    }

    #[test]
    fn failed_disposable_creation_resumes_reserved_uuid_without_touching_ongoing_entity() {
        let root = TestRoot::new();
        let manager = root.manager();
        let (ongoing_id, ongoing_dir) = manager.create_agent("Ongoing").unwrap();
        seed_ongoing_files(&ongoing_dir);
        let ongoing_files = snapshot_files(&ongoing_dir);
        let key = Uuid::new_v4().to_string();
        let reserved = reserve(&manager, &key);
        let disposable_dir = manager.identities_dir().join(&reserved.entity_id);
        // A file where the new Entity directory belongs simulates an ordinary
        // initialization failure after the durable reservation was written.
        std::fs::write(&disposable_dir, b"synthetic temporary obstruction").unwrap();
        assert!(matches!(
            manager.create_agent_idempotent("Signal", &key),
            Err(AgentCreationError::Storage(_))
        ));
        assert_eq!(snapshot_files(&ongoing_dir), ongoing_files);
        drop(manager);

        let reopened = root.manager();
        assert_eq!(reopened.list_agents().unwrap()[0].id, ongoing_id);
        assert_eq!(
            reopened.entity_creation_status(&key).unwrap(),
            AgentCreationStatus::Reserved {
                name: "Signal".to_string(),
            }
        );
        std::fs::remove_file(&disposable_dir).unwrap();
        let recovered = reopened.create_agent_idempotent("Signal", &key).unwrap();
        assert_eq!(recovered.0, reserved.entity_id);
        assert_eq!(recovered.1, disposable_dir);
        assert_ne!(recovered.0, ongoing_id);
        reopened.delete_agent(&recovered.0).unwrap();
        drop(reopened);

        let final_manager = root.manager();
        let agents = final_manager.list_agents().unwrap();
        assert_eq!(agents.len(), 1);
        assert_eq!(agents[0].id, ongoing_id);
        assert_eq!(snapshot_files(&ongoing_dir), ongoing_files);
        assert!(!disposable_dir.exists());
        assert!(matches!(
            final_manager.create_agent_idempotent("Signal", &key),
            Err(AgentCreationError::Unavailable(_))
        ));
    }
}
