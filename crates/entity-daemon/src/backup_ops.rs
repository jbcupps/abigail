use abigail_core::profile::ProfileContext;
use abigail_memory::MemoryStore;
use abigail_skills::backup::{BackupImportResult, BackupInfo, BackupOperations, BackupPreview};
use async_trait::async_trait;
use std::path::PathBuf;
use std::sync::Arc;

pub struct LocalBackupOps {
    memory: Arc<MemoryStore>,
    data_root: PathBuf,
    agent_name: Option<String>,
    profile: Option<Arc<ProfileContext>>,
}

impl LocalBackupOps {
    pub fn new_with_profile(
        memory: Arc<MemoryStore>,
        data_root: PathBuf,
        agent_name: Option<String>,
        profile: Option<Arc<ProfileContext>>,
    ) -> Result<Self, String> {
        if let Some(profile) = &profile {
            if !abigail_core::profile::same_path(&data_root, profile.root()) {
                return Err(
                    "Backup data root does not match the validated preview profile.".into(),
                );
            }
            profile
                .validate_path(&data_root)
                .map_err(|e| e.to_string())?;
        }
        Ok(Self {
            memory,
            data_root,
            agent_name,
            profile,
        })
    }

    fn backup_db_path(&self, backup_path: &str) -> Result<PathBuf, String> {
        abigail_memory::backup_import::find_memory_db_with_profile(
            &PathBuf::from(backup_path),
            self.profile.as_deref(),
        )
        .map_err(|e| e.to_string())?
        .ok_or_else(|| format!("No memory database found in {}", backup_path))
    }
}

#[async_trait]
impl BackupOperations for LocalBackupOps {
    async fn list_backups(&self) -> Result<Vec<BackupInfo>, String> {
        let entries = abigail_memory::backup_import::scan_backup_dirs_with_profile(
            &self.data_root,
            self.agent_name.as_deref(),
            self.profile.as_deref(),
        )
        .map_err(|e| e.to_string())?;

        Ok(entries
            .into_iter()
            .map(|e| BackupInfo {
                path: e.path,
                timestamp: e.timestamp,
                has_memory_db: e.has_memory_db,
            })
            .collect())
    }

    async fn preview_backup(&self, backup_path: &str) -> Result<BackupPreview, String> {
        let db_path = self.backup_db_path(backup_path)?;

        let stats = abigail_memory::preview_backup_db(&db_path).map_err(|e| e.to_string())?;

        Ok(BackupPreview {
            db_path: stats.db_path,
            turn_count: stats.turn_count,
            memory_count: stats.memory_count,
            session_count: stats.session_count,
            earliest: stats.earliest,
            latest: stats.latest,
        })
    }

    async fn import_backup(&self, backup_path: &str) -> Result<BackupImportResult, String> {
        let db_path = self.backup_db_path(backup_path)?;

        let result = abigail_memory::import_from_backup(&self.memory, &db_path)
            .map_err(|e| e.to_string())?;

        Ok(BackupImportResult {
            turns_imported: result.turns_imported,
            turns_skipped: result.turns_skipped,
            memories_imported: result.memories_imported,
            memories_skipped: result.memories_skipped,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn preview_backup_ops_rejects_external_source_before_preview_or_import() {
        let fixture =
            std::env::temp_dir().join(format!("preview-backup-ops-{}", uuid::Uuid::new_v4()));
        let profile = Arc::new(ProfileContext::preflight(&fixture.join("preview"), &[]).unwrap());
        let outside = fixture.join("stable-sentinel");
        std::fs::create_dir_all(&outside).unwrap();
        let sentinel = outside.join("memory.db");
        std::fs::write(&sentinel, b"synthetic sentinel: never a database").unwrap();
        let memory = Arc::new(MemoryStore::open_in_memory().unwrap());
        let ops = LocalBackupOps::new_with_profile(
            memory.clone(),
            profile.root().to_path_buf(),
            None,
            Some(profile.clone()),
        )
        .unwrap();
        let expected =
            abigail_memory::backup_import::validate_preview_backup_path(&profile, &outside)
                .unwrap_err()
                .to_string();
        let requested = outside.to_string_lossy();
        assert_eq!(ops.preview_backup(&requested).await.unwrap_err(), expected);
        assert_eq!(ops.import_backup(&requested).await.unwrap_err(), expected);
        assert_eq!(memory.all_turns().unwrap().len(), 0);
        assert_eq!(memory.count_memories().unwrap(), 0);
        assert_eq!(
            std::fs::read(&sentinel).unwrap(),
            b"synthetic sentinel: never a database"
        );
        assert_eq!(std::fs::read_dir(&outside).unwrap().count(), 1);
        assert!(LocalBackupOps::new_with_profile(memory, outside, None, Some(profile),).is_err());
        std::fs::remove_dir_all(fixture).unwrap();
    }

    #[test]
    fn preview_backup_ops_accepts_only_profile_backup_database_location() {
        let fixture =
            std::env::temp_dir().join(format!("preview-backup-ops-{}", uuid::Uuid::new_v4()));
        let profile = Arc::new(ProfileContext::preflight(&fixture, &[]).unwrap());
        let backup = profile.backups_dir().join("Signal/20261020");
        std::fs::create_dir_all(&backup).unwrap();
        let database = backup.join("memory.db");
        std::fs::write(&database, b"synthetic path fixture").unwrap();
        let ops = LocalBackupOps::new_with_profile(
            Arc::new(MemoryStore::open_in_memory().unwrap()),
            profile.root().to_path_buf(),
            None,
            Some(profile),
        )
        .unwrap();
        assert_eq!(
            ops.backup_db_path(&backup.to_string_lossy()).unwrap(),
            database
        );
        std::fs::remove_dir_all(fixture).unwrap();
    }
}
