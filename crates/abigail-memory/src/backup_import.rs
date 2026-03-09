//! Backup import logic for Surreal-backed memory snapshots.

use crate::store::MemoryStore;
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BackupStats {
    pub db_path: String,
    pub turn_count: u64,
    pub memory_count: u64,
    pub session_count: u64,
    pub earliest: Option<String>,
    pub latest: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ImportStats {
    pub turns_imported: u64,
    pub turns_skipped: u64,
    pub memories_imported: u64,
    pub memories_skipped: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BackupEntry {
    pub path: String,
    pub timestamp: String,
    pub has_memory_db: bool,
}

pub fn preview_backup_db(db_path: &Path) -> anyhow::Result<BackupStats> {
    let store = MemoryStore::open(db_path)?;
    let turns = store.all_turns()?;
    let memories = store.all_memories()?;

    let earliest = turns.iter().map(|turn| turn.created_at.to_rfc3339()).min();
    let latest = turns.iter().map(|turn| turn.created_at.to_rfc3339()).max();

    let session_count = turns
        .iter()
        .map(|turn| turn.session_id.clone())
        .collect::<std::collections::BTreeSet<_>>()
        .len() as u64;

    Ok(BackupStats {
        db_path: db_path.to_string_lossy().to_string(),
        turn_count: turns.len() as u64,
        memory_count: memories.len() as u64,
        session_count,
        earliest,
        latest,
    })
}

pub fn import_from_backup(
    target: &MemoryStore,
    backup_db_path: &Path,
) -> anyhow::Result<ImportStats> {
    let source = MemoryStore::open(backup_db_path)?;
    let turns = source.all_turns()?;
    let memories = source.all_memories()?;

    let mut turns_imported = 0u64;
    let mut turns_skipped = 0u64;
    let mut memories_imported = 0u64;
    let mut memories_skipped = 0u64;

    for turn in turns {
        if target.insert_turn_or_ignore(&turn)? {
            turns_imported += 1;
        } else {
            turns_skipped += 1;
        }
    }

    for memory in memories {
        if target.insert_memory_or_ignore(&memory)? {
            memories_imported += 1;
        } else {
            memories_skipped += 1;
        }
    }

    Ok(ImportStats {
        turns_imported,
        turns_skipped,
        memories_imported,
        memories_skipped,
    })
}

pub fn find_memory_db(backup_dir: &Path) -> Option<PathBuf> {
    let candidates = [
        backup_dir.join("memory.db"),
        backup_dir.join("entity").join("memory.db"),
        backup_dir.join("entity").join("abigail_memory.db"),
        backup_dir.join("abigail_memory.db"),
        backup_dir.join("abigail_seed.db"),
        backup_dir.join("entity").join("abigail_seed.db"),
    ];
    candidates.into_iter().find(|path| path.exists())
}

pub fn scan_backup_dirs(data_root: &Path, agent_name: Option<&str>) -> Vec<BackupEntry> {
    let mut entries = Vec::new();
    let mut scan_dirs = vec![data_root.join("backups")];

    if let Some(user_dirs) = directories::UserDirs::new() {
        if let Some(doc_dir) = user_dirs.document_dir() {
            scan_dirs.push(doc_dir.join("Abigail").join("backups"));
        }
    }

    for base_dir in &scan_dirs {
        if !base_dir.exists() {
            continue;
        }

        let agent_dirs: Vec<PathBuf> = if let Some(name) = agent_name {
            let specific = base_dir.join(name);
            if specific.exists() {
                vec![specific]
            } else {
                vec![]
            }
        } else {
            std::fs::read_dir(base_dir)
                .into_iter()
                .flatten()
                .flatten()
                .filter(|entry| entry.file_type().map(|ty| ty.is_dir()).unwrap_or(false))
                .map(|entry| entry.path())
                .collect()
        };

        for agent_dir in agent_dirs {
            let timestamp_dirs: Vec<_> = std::fs::read_dir(&agent_dir)
                .into_iter()
                .flatten()
                .flatten()
                .filter(|entry| entry.file_type().map(|ty| ty.is_dir()).unwrap_or(false))
                .collect();

            for ts_entry in timestamp_dirs {
                let ts_path = ts_entry.path();
                entries.push(BackupEntry {
                    path: ts_path.to_string_lossy().to_string(),
                    timestamp: ts_entry.file_name().to_string_lossy().to_string(),
                    has_memory_db: find_memory_db(&ts_path).is_some(),
                });
            }
        }
    }

    entries.sort_by(|left, right| right.timestamp.cmp(&left.timestamp));
    entries
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        store::{MemoryStore, MemoryWeight},
        ConversationTurn, Memory,
    };

    #[test]
    #[cfg_attr(
        target_os = "windows",
        ignore = "Windows SurrealKV CI flakiness – tracked upstream"
    )]
    fn test_preview_backup_with_data() {
        let tmp =
            std::env::temp_dir().join(format!("abigail_backup_preview_{}", uuid::Uuid::new_v4()));
        let db_path = tmp.join("backup-store");
        std::fs::create_dir_all(&tmp).unwrap();

        {
            let store = MemoryStore::open(&db_path).unwrap();
            store
                .insert_turn(&ConversationTurn::new("s1", "user", "hello"))
                .unwrap();
            store
                .insert_memory(&Memory {
                    id: uuid::Uuid::new_v4().to_string(),
                    content: "memory".into(),
                    weight: MemoryWeight::Distilled,
                    created_at: chrono::Utc::now(),
                })
                .unwrap();
        }

        let stats = preview_backup_db(&db_path).unwrap();
        assert_eq!(stats.turn_count, 1);
        assert_eq!(stats.memory_count, 1);

        let _ = std::fs::remove_dir_all(tmp);
    }

    #[test]
    #[cfg_attr(
        target_os = "windows",
        ignore = "Windows SurrealKV CI flakiness – tracked upstream"
    )]
    fn test_import_from_backup_idempotent() {
        let tmp =
            std::env::temp_dir().join(format!("abigail_backup_import_{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&tmp).unwrap();
        let backup_path = tmp.join("backup-store");

        {
            let backup = MemoryStore::open(&backup_path).unwrap();
            backup
                .insert_turn(&ConversationTurn::new("s1", "user", "hello"))
                .unwrap();
            backup
                .insert_memory(&Memory::ephemeral("remember".into()))
                .unwrap();
        }

        let target = MemoryStore::open_in_memory().unwrap();
        let stats = import_from_backup(&target, &backup_path).unwrap();
        assert_eq!(stats.turns_imported, 1);
        assert_eq!(stats.memories_imported, 1);

        let stats2 = import_from_backup(&target, &backup_path).unwrap();
        assert_eq!(stats2.turns_skipped, 1);
        assert_eq!(stats2.memories_skipped, 1);

        let _ = std::fs::remove_dir_all(tmp);
    }
}
