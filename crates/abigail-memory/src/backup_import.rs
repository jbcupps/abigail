//! Backup import logic — scan, preview, and import conversation turns/memories
//! from backup SQLite databases into a running MemoryStore.

use crate::store::{ConversationTurn, Memory, MemoryStore, MemoryWeight, StoreError};
use chrono::Utc;
use rusqlite::{Connection, OpenFlags};
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

/// Summary of what's in a backup database.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BackupStats {
    pub db_path: String,
    pub turn_count: u64,
    pub memory_count: u64,
    pub session_count: u64,
    pub earliest: Option<String>,
    pub latest: Option<String>,
}

/// Result of an import operation.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ImportStats {
    pub turns_imported: u64,
    pub turns_skipped: u64,
    pub memories_imported: u64,
    pub memories_skipped: u64,
}

/// A discovered backup directory entry.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BackupEntry {
    pub path: String,
    pub timestamp: String,
    pub has_memory_db: bool,
}

/// Open a backup SQLite database in read-only mode.
fn open_backup_db(db_path: &Path) -> Result<Connection, StoreError> {
    let conn = Connection::open_with_flags(db_path, OpenFlags::SQLITE_OPEN_READ_ONLY)?;
    Ok(conn)
}

/// Check if a table exists in the given connection.
fn table_exists(conn: &Connection, table_name: &str) -> bool {
    conn.query_row(
        "SELECT COUNT(*) FROM sqlite_master WHERE type='table' AND name=?1",
        [table_name],
        |row| row.get::<_, i64>(0),
    )
    .map(|c| c > 0)
    .unwrap_or(false)
}

/// Preview the contents of a backup database without importing anything.
pub fn preview_backup_db(db_path: &Path) -> Result<BackupStats, StoreError> {
    let conn = open_backup_db(db_path)?;

    let turn_count = if table_exists(&conn, "conversation_turns") {
        conn.query_row("SELECT COUNT(*) FROM conversation_turns", [], |row| {
            row.get::<_, i64>(0)
        })? as u64
    } else {
        0
    };

    let memory_count = if table_exists(&conn, "memories") {
        conn.query_row("SELECT COUNT(*) FROM memories", [], |row| {
            row.get::<_, i64>(0)
        })? as u64
    } else {
        0
    };

    let session_count = if table_exists(&conn, "conversation_turns") {
        conn.query_row(
            "SELECT COUNT(DISTINCT session_id) FROM conversation_turns",
            [],
            |row| row.get::<_, i64>(0),
        )? as u64
    } else {
        0
    };

    let (earliest, latest) = if table_exists(&conn, "conversation_turns") && turn_count > 0 {
        let earliest: Option<String> = conn
            .query_row(
                "SELECT MIN(created_at) FROM conversation_turns",
                [],
                |row| row.get(0),
            )
            .ok();
        let latest: Option<String> = conn
            .query_row(
                "SELECT MAX(created_at) FROM conversation_turns",
                [],
                |row| row.get(0),
            )
            .ok();
        (earliest, latest)
    } else {
        (None, None)
    };

    Ok(BackupStats {
        db_path: db_path.to_string_lossy().to_string(),
        turn_count,
        memory_count,
        session_count,
        earliest,
        latest,
    })
}

/// Import all conversation turns and memories from a backup database
/// into the target MemoryStore, using INSERT OR IGNORE for idempotency.
pub fn import_from_backup(
    target: &MemoryStore,
    backup_db_path: &Path,
) -> Result<ImportStats, StoreError> {
    let conn = open_backup_db(backup_db_path)?;

    let mut turns_imported = 0u64;
    let mut turns_skipped = 0u64;
    let mut memories_imported = 0u64;
    let mut memories_skipped = 0u64;

    // Import conversation turns
    if table_exists(&conn, "conversation_turns") {
        let mut stmt = conn.prepare(
            "SELECT id, session_id, turn_number, role, content, provider, model, \
                    tier, complexity_score, token_estimate, created_at \
             FROM conversation_turns ORDER BY created_at ASC",
        )?;

        let rows = stmt.query_map([], |row| {
            let created_at: String = row.get(10)?;
            let created_at = chrono::DateTime::parse_from_rfc3339(&created_at)
                .map(|dt| dt.with_timezone(&Utc))
                .unwrap_or_else(|_| Utc::now());
            Ok(ConversationTurn {
                id: row.get(0)?,
                session_id: row.get(1)?,
                turn_number: row.get::<_, i64>(2)? as u32,
                role: row.get(3)?,
                content: row.get(4)?,
                provider: row.get(5)?,
                model: row.get(6)?,
                tier: row.get(7)?,
                complexity_score: row.get::<_, Option<i64>>(8)?.map(|v| v as u8),
                token_estimate: row.get::<_, Option<i64>>(9)?.map(|v| v as u32),
                created_at,
            })
        })?;

        for row in rows {
            match row {
                Ok(turn) => {
                    if target.insert_turn_or_ignore(&turn)? {
                        turns_imported += 1;
                    } else {
                        turns_skipped += 1;
                    }
                }
                Err(e) => {
                    tracing::warn!("Skipping malformed turn row: {}", e);
                    turns_skipped += 1;
                }
            }
        }
    }

    // Import memories
    if table_exists(&conn, "memories") {
        let mut stmt = conn.prepare(
            "SELECT id, content, weight, created_at FROM memories ORDER BY created_at ASC",
        )?;

        let rows = stmt.query_map([], |row| {
            let created_at: String = row.get(3)?;
            let created_at = chrono::DateTime::parse_from_rfc3339(&created_at)
                .map(|dt| dt.with_timezone(&Utc))
                .unwrap_or_else(|_| Utc::now());
            let weight_str: String = row.get(2)?;
            let weight = match weight_str.as_str() {
                "ephemeral" => MemoryWeight::Ephemeral,
                "distilled" => MemoryWeight::Distilled,
                "crystallized" => MemoryWeight::Crystallized,
                _ => MemoryWeight::Ephemeral,
            };
            Ok(Memory {
                id: row.get(0)?,
                content: row.get(1)?,
                weight,
                created_at,
            })
        })?;

        for row in rows {
            match row {
                Ok(memory) => {
                    if target.insert_memory_or_ignore(&memory)? {
                        memories_imported += 1;
                    } else {
                        memories_skipped += 1;
                    }
                }
                Err(e) => {
                    tracing::warn!("Skipping malformed memory row: {}", e);
                    memories_skipped += 1;
                }
            }
        }
    }

    Ok(ImportStats {
        turns_imported,
        turns_skipped,
        memories_imported,
        memories_skipped,
    })
}

/// Try to find the memory database file within a backup directory.
/// Checks several known locations in order of preference.
pub fn find_memory_db(backup_dir: &Path) -> Option<PathBuf> {
    let candidates = [
        backup_dir.join("entity").join("abigail_memory.db"),
        backup_dir.join("abigail_memory.db"),
        backup_dir.join("abigail_seed.db"),
        backup_dir.join("entity").join("abigail_seed.db"),
    ];
    candidates.into_iter().find(|p| p.exists())
}

/// Scan known backup directories for available backups.
/// Looks in both `{data_root}/backups/` and `Documents/Abigail/backups/`.
pub fn scan_backup_dirs(data_root: &Path, agent_name: Option<&str>) -> Vec<BackupEntry> {
    let mut entries = Vec::new();

    let mut scan_dirs = vec![data_root.join("backups")];

    // Also check Documents/Abigail/backups/
    if let Some(user_dirs) = directories::UserDirs::new() {
        if let Some(doc_dir) = user_dirs.document_dir() {
            scan_dirs.push(doc_dir.join("Abigail").join("backups"));
        }
    }

    for base_dir in &scan_dirs {
        if !base_dir.exists() {
            continue;
        }

        // If agent_name is given, look in {base_dir}/{agent_name}/
        // Otherwise scan all subdirs
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
                .filter(|e| e.file_type().map(|ft| ft.is_dir()).unwrap_or(false))
                .map(|e| e.path())
                .collect()
        };

        for agent_dir in agent_dirs {
            let timestamp_dirs: Vec<_> = std::fs::read_dir(&agent_dir)
                .into_iter()
                .flatten()
                .flatten()
                .filter(|e| e.file_type().map(|ft| ft.is_dir()).unwrap_or(false))
                .collect();

            for ts_entry in timestamp_dirs {
                let ts_path = ts_entry.path();
                let has_db = find_memory_db(&ts_path).is_some();
                let timestamp = ts_entry
                    .file_name()
                    .to_string_lossy()
                    .to_string();

                entries.push(BackupEntry {
                    path: ts_path.to_string_lossy().to_string(),
                    timestamp,
                    has_memory_db: has_db,
                });
            }
        }
    }

    // Sort by timestamp descending (most recent first)
    entries.sort_by(|a, b| b.timestamp.cmp(&a.timestamp));
    entries
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::store::MemoryStore;

    #[test]
    fn test_preview_empty_db() {
        let tmp = std::env::temp_dir().join("abigail_backup_preview_test");
        let _ = std::fs::remove_dir_all(&tmp);
        std::fs::create_dir_all(&tmp).unwrap();

        let db_path = tmp.join("backup.db");
        // Create a minimal DB with the schema
        let store = MemoryStore::open(&db_path).unwrap();
        drop(store);

        let stats = preview_backup_db(&db_path).unwrap();
        assert_eq!(stats.turn_count, 0);
        assert_eq!(stats.memory_count, 0);
        assert_eq!(stats.session_count, 0);

        let _ = std::fs::remove_dir_all(&tmp);
    }

    #[test]
    fn test_import_from_backup_idempotent() {
        let tmp = std::env::temp_dir().join("abigail_backup_import_test");
        let _ = std::fs::remove_dir_all(&tmp);
        std::fs::create_dir_all(&tmp).unwrap();

        // Create a "backup" DB with some data
        let backup_path = tmp.join("backup.db");
        {
            let backup_store = MemoryStore::open(&backup_path).unwrap();
            let turn = ConversationTurn::new("sess1", "user", "hello from backup");
            backup_store.insert_turn(&turn).unwrap();
            let mem = Memory::ephemeral("backup memory".into());
            backup_store.insert_memory(&mem).unwrap();
        }

        // Target store (empty)
        let target = MemoryStore::open_in_memory().unwrap();

        // First import
        let stats = import_from_backup(&target, &backup_path).unwrap();
        assert_eq!(stats.turns_imported, 1);
        assert_eq!(stats.turns_skipped, 0);
        assert_eq!(stats.memories_imported, 1);
        assert_eq!(stats.memories_skipped, 0);

        // Second import — all skipped (idempotent)
        let stats2 = import_from_backup(&target, &backup_path).unwrap();
        assert_eq!(stats2.turns_imported, 0);
        assert_eq!(stats2.turns_skipped, 1);
        assert_eq!(stats2.memories_imported, 0);
        assert_eq!(stats2.memories_skipped, 1);

        // Verify target has the data
        assert_eq!(target.total_turn_count().unwrap(), 1);
        assert_eq!(target.count_memories().unwrap(), 1);

        let _ = std::fs::remove_dir_all(&tmp);
    }

    #[test]
    fn test_find_memory_db() {
        let tmp = std::env::temp_dir().join("abigail_find_db_test");
        let _ = std::fs::remove_dir_all(&tmp);
        std::fs::create_dir_all(tmp.join("entity")).unwrap();

        // No DB file yet
        assert!(find_memory_db(&tmp).is_none());

        // Create entity/abigail_memory.db
        std::fs::write(tmp.join("entity").join("abigail_memory.db"), b"").unwrap();
        let found = find_memory_db(&tmp).unwrap();
        assert!(found.ends_with("abigail_memory.db"));

        let _ = std::fs::remove_dir_all(&tmp);
    }

    #[test]
    fn test_preview_backup_with_data() {
        let tmp = std::env::temp_dir().join("abigail_backup_preview_data_test");
        let _ = std::fs::remove_dir_all(&tmp);
        std::fs::create_dir_all(&tmp).unwrap();

        let db_path = tmp.join("backup.db");
        {
            let store = MemoryStore::open(&db_path).unwrap();
            store
                .insert_turn(&ConversationTurn::new("s1", "user", "hello"))
                .unwrap();
            store
                .insert_turn(&ConversationTurn::new("s1", "assistant", "hi there"))
                .unwrap();
            store
                .insert_turn(&ConversationTurn::new("s2", "user", "different session"))
                .unwrap();
            store
                .insert_memory(&Memory::distilled("a distilled thought".into()))
                .unwrap();
        }

        let stats = preview_backup_db(&db_path).unwrap();
        assert_eq!(stats.turn_count, 3);
        assert_eq!(stats.memory_count, 1);
        assert_eq!(stats.session_count, 2);
        assert!(stats.earliest.is_some());
        assert!(stats.latest.is_some());

        let _ = std::fs::remove_dir_all(&tmp);
    }
}
