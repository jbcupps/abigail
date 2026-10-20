//! Backup import logic for Surreal-backed memory snapshots.

use crate::store::MemoryStore;
use abigail_core::profile::ProfileContext;
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
    preview_backup_store(&store, &db_path.to_string_lossy())
}

pub fn preview_backup_store(store: &MemoryStore, label: &str) -> anyhow::Result<BackupStats> {
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
        db_path: label.to_string(),
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
    import_from_store(target, &source)
}

pub fn import_from_store(
    target: &MemoryStore,
    source: &MemoryStore,
) -> anyhow::Result<ImportStats> {
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
    memory_db_candidates(backup_dir)
        .into_iter()
        .find(|path| path.exists())
}

fn memory_db_candidates(backup_dir: &Path) -> [PathBuf; 6] {
    [
        backup_dir.join("memory.db"),
        backup_dir.join("entity").join("memory.db"),
        backup_dir.join("entity").join("abigail_memory.db"),
        backup_dir.join("abigail_memory.db"),
        backup_dir.join("abigail_seed.db"),
        backup_dir.join("entity").join("abigail_seed.db"),
    ]
}

/// Validate a preview backup path before enumeration or database access.
pub fn validate_preview_backup_path(profile: &ProfileContext, path: &Path) -> anyhow::Result<()> {
    let backup_root = profile.backups_dir();
    profile.validate_path(&backup_root)?;
    profile.validate_path(path)?;
    let canonical_root = abigail_core::profile::canonical_without_links(&backup_root)?;
    let canonical_path = abigail_core::profile::canonical_without_links(path)?;
    abigail_core::path_guard::ensure_path_within_root(
        &canonical_root,
        &canonical_path,
        "Preview backup",
    )?;
    Ok(())
}

pub fn find_memory_db_with_profile(
    backup_dir: &Path,
    profile: Option<&ProfileContext>,
) -> anyhow::Result<Option<PathBuf>> {
    let Some(profile) = profile else {
        return Ok(find_memory_db(backup_dir));
    };
    validate_preview_backup_path(profile, backup_dir)?;
    for path in memory_db_candidates(backup_dir) {
        validate_preview_backup_path(profile, &path)?;
        if path.exists() {
            return Ok(Some(path));
        }
    }
    Ok(None)
}

pub fn scan_backup_dirs(data_root: &Path, agent_name: Option<&str>) -> Vec<BackupEntry> {
    let mut scan_dirs = vec![data_root.join("backups")];

    if let Some(user_dirs) = directories::UserDirs::new() {
        if let Some(doc_dir) = user_dirs.document_dir() {
            scan_dirs.push(doc_dir.join("Abigail").join("backups"));
        }
    }

    // Stable discovery retains its historical best-effort behavior.
    scan_backup_roots(&scan_dirs, agent_name, None).unwrap_or_default()
}

pub fn scan_backup_dirs_with_profile(
    data_root: &Path,
    agent_name: Option<&str>,
    profile: Option<&ProfileContext>,
) -> anyhow::Result<Vec<BackupEntry>> {
    let Some(profile) = profile else {
        return Ok(scan_backup_dirs(data_root, agent_name));
    };
    if !abigail_core::profile::same_path(data_root, profile.root()) {
        anyhow::bail!("Backup data root does not match the validated preview profile.");
    }
    scan_backup_roots(&[profile.backups_dir()], agent_name, Some(profile))
}

fn scan_backup_roots(
    scan_dirs: &[PathBuf],
    agent_name: Option<&str>,
    profile: Option<&ProfileContext>,
) -> anyhow::Result<Vec<BackupEntry>> {
    let mut entries = Vec::new();
    for base_dir in scan_dirs {
        if let Some(profile) = profile {
            validate_preview_backup_path(profile, base_dir)?;
        }
        if !base_dir.exists() {
            continue;
        }

        let agent_dirs: Vec<PathBuf> = if let Some(name) = agent_name {
            let specific = base_dir.join(name);
            if let Some(profile) = profile {
                validate_preview_backup_path(profile, &specific)?;
            }
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
            if let Some(profile) = profile {
                validate_preview_backup_path(profile, &agent_dir)?;
            }
            let timestamp_dirs: Vec<_> = std::fs::read_dir(&agent_dir)
                .into_iter()
                .flatten()
                .flatten()
                .filter(|entry| entry.file_type().map(|ty| ty.is_dir()).unwrap_or(false))
                .collect();

            for ts_entry in timestamp_dirs {
                let ts_path = ts_entry.path();
                let memory_db = find_memory_db_with_profile(&ts_path, profile)?;
                entries.push(BackupEntry {
                    path: ts_path.to_string_lossy().to_string(),
                    timestamp: ts_entry.file_name().to_string_lossy().to_string(),
                    has_memory_db: memory_db.is_some(),
                });
            }
        }
    }

    entries.sort_by(|left, right| right.timestamp.cmp(&left.timestamp));
    Ok(entries)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        store::{MemoryStore, MemoryWeight},
        ConversationTurn, Memory,
    };

    #[test]
    fn preview_backup_scan_uses_only_profile_backups_and_preserves_sibling_fixture() {
        let fixture =
            std::env::temp_dir().join(format!("preview-backups-{}", uuid::Uuid::new_v4()));
        let stable = fixture.join("stable-sentinel");
        let stable_backup = stable.join("backups/Signal/20990101");
        std::fs::create_dir_all(&stable_backup).unwrap();
        let sentinel = stable_backup.join("memory.db");
        std::fs::write(&sentinel, b"synthetic stable sentinel").unwrap();
        let profile =
            ProfileContext::preflight(&fixture.join("preview"), std::slice::from_ref(&stable))
                .unwrap();
        let backup = profile.backups_dir().join("Signal/20261020");
        std::fs::create_dir_all(&backup).unwrap();
        std::fs::write(backup.join("memory.db"), b"synthetic listing fixture").unwrap();

        for agent_name in [Some("Signal"), None] {
            let entries =
                scan_backup_dirs_with_profile(profile.root(), agent_name, Some(&profile)).unwrap();
            assert_eq!(entries.len(), 1);
            assert_eq!(Path::new(&entries[0].path), backup);
            assert!(entries[0].has_memory_db);
        }
        assert_eq!(
            std::fs::read(&sentinel).unwrap(),
            b"synthetic stable sentinel"
        );
        assert_eq!(std::fs::read_dir(&stable_backup).unwrap().count(), 1);
        std::fs::remove_dir_all(fixture).unwrap();
    }

    #[test]
    fn preview_backup_selection_rejects_outside_paths_and_conflicting_roots() {
        let fixture =
            std::env::temp_dir().join(format!("preview-backups-{}", uuid::Uuid::new_v4()));
        let profile = ProfileContext::preflight(&fixture.join("preview"), &[]).unwrap();
        let outside = fixture.join("outside");
        std::fs::create_dir_all(&outside).unwrap();
        let sentinel = outside.join("memory.db");
        std::fs::write(&sentinel, b"must never open as a database").unwrap();
        let internal_non_backup = profile.identities_dir().join("not-a-backup");
        assert!(find_memory_db_with_profile(&outside, Some(&profile)).is_err());
        assert!(find_memory_db_with_profile(&internal_non_backup, Some(&profile)).is_err());
        assert!(scan_backup_dirs_with_profile(&outside, None, Some(&profile)).is_err());
        std::fs::create_dir_all(profile.backups_dir()).unwrap();
        assert!(scan_backup_dirs_with_profile(
            profile.root(),
            Some("../identities"),
            Some(&profile)
        )
        .is_err());
        assert_eq!(
            std::fs::read(&sentinel).unwrap(),
            b"must never open as a database"
        );
        assert_eq!(std::fs::read_dir(&outside).unwrap().count(), 1);
        std::fs::remove_dir_all(fixture).unwrap();
    }

    #[test]
    fn preview_backup_selection_keeps_supported_nested_database_layout() {
        let fixture =
            std::env::temp_dir().join(format!("preview-backups-{}", uuid::Uuid::new_v4()));
        let profile = ProfileContext::preflight(&fixture, &[]).unwrap();
        let backup = profile.backups_dir().join("Signal/20261020");
        let database = backup.join("entity/abigail_memory.db");
        std::fs::create_dir_all(database.parent().unwrap()).unwrap();
        std::fs::write(&database, b"synthetic path fixture").unwrap();
        assert_eq!(
            find_memory_db_with_profile(&backup, Some(&profile)).unwrap(),
            Some(database)
        );
        std::fs::remove_dir_all(fixture).unwrap();
    }

    #[test]
    fn test_preview_backup_with_data() {
        let store = MemoryStore::open_in_memory().unwrap();
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

        let stats = preview_backup_store(&store, ":memory:").unwrap();
        assert_eq!(stats.turn_count, 1);
        assert_eq!(stats.memory_count, 1);
    }

    #[test]
    fn test_import_from_backup_idempotent() {
        let source = MemoryStore::open_in_memory().unwrap();
        source
            .insert_turn(&ConversationTurn::new("s1", "user", "hello"))
            .unwrap();
        source
            .insert_memory(&Memory::ephemeral("remember".into()))
            .unwrap();

        let target = MemoryStore::open_in_memory().unwrap();
        let stats = import_from_store(&target, &source).unwrap();
        assert_eq!(stats.turns_imported, 1);
        assert_eq!(stats.memories_imported, 1);

        let stats2 = import_from_store(&target, &source).unwrap();
        assert_eq!(stats2.turns_skipped, 1);
        assert_eq!(stats2.memories_skipped, 1);
    }
}
