use crate::state::AppState;
use abigail_skills::backup::{BackupImportResult, BackupInfo, BackupOperations, BackupPreview};
use async_trait::async_trait;
use tauri::{AppHandle, Manager};

pub struct TauriBackupOps {
    app_handle: AppHandle,
}

impl TauriBackupOps {
    pub fn new(app_handle: AppHandle) -> Self {
        Self { app_handle }
    }
}

#[async_trait]
impl BackupOperations for TauriBackupOps {
    async fn list_backups(&self) -> Result<Vec<BackupInfo>, String> {
        let state = self.app_handle.state::<AppState>();

        let (data_dir, agent_name) = {
            let config = state.config.read().map_err(|e| e.to_string())?;
            (config.data_dir.clone(), config.agent_name.clone())
        };

        let entries =
            abigail_memory::scan_backup_dirs(&data_dir, agent_name.as_deref());

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
        let backup_dir = std::path::PathBuf::from(backup_path);
        let db_path = abigail_memory::find_memory_db(&backup_dir)
            .ok_or_else(|| format!("No memory database found in {}", backup_path))?;

        let stats =
            abigail_memory::preview_backup_db(&db_path).map_err(|e| e.to_string())?;

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
        let state = self.app_handle.state::<AppState>();
        let backup_dir = std::path::PathBuf::from(backup_path);
        let db_path = abigail_memory::find_memory_db(&backup_dir)
            .ok_or_else(|| format!("No memory database found in {}", backup_path))?;

        let memory_guard = state.memory.read().map_err(|e| e.to_string())?;

        let result = abigail_memory::import_from_backup(&memory_guard, &db_path)
            .map_err(|e| e.to_string())?;

        Ok(BackupImportResult {
            turns_imported: result.turns_imported,
            turns_skipped: result.turns_skipped,
            memories_imported: result.memories_imported,
            memories_skipped: result.memories_skipped,
        })
    }
}
