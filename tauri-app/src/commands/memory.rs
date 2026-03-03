use crate::state::AppState;
use serde::{Deserialize, Serialize};
use tauri::State;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SqliteStats {
    pub size_bytes: u64,
    pub memory_count: u64,
    pub has_birth: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MemoryInfo {
    pub id: String,
    pub content: String,
    pub weight: String,
    pub created_at: String,
}

#[tauri::command]
pub fn get_sqlite_stats(state: State<AppState>) -> Result<SqliteStats, String> {
    let config = state.config.read().map_err(|e| e.to_string())?;
    let db_path = config.db_path.clone();
    drop(config);

    let size_bytes = std::fs::metadata(&db_path).map(|m| m.len()).unwrap_or(0);

    let mem = state.memory.read().map_err(|e| e.to_string())?;
    let memory_count = mem.count_memories().map_err(|e| e.to_string())?;
    let has_birth = mem.has_birth().map_err(|e| e.to_string())?;
    drop(mem);

    Ok(SqliteStats {
        size_bytes,
        memory_count,
        has_birth,
    })
}

#[tauri::command]
pub fn optimize_sqlite(state: State<AppState>) -> Result<i64, String> {
    let config = state.config.read().map_err(|e| e.to_string())?;
    let db_path = config.db_path.clone();
    drop(config);

    let size_before = std::fs::metadata(&db_path).map(|m| m.len()).unwrap_or(0);

    state.memory.read().map_err(|e| e.to_string())?.vacuum().map_err(|e| e.to_string())?;

    let size_after = std::fs::metadata(&db_path).map(|m| m.len()).unwrap_or(0);

    let saved = size_before as i64 - size_after as i64;
    tracing::info!("SQLite optimized: {} bytes saved", saved);
    Ok(saved)
}

#[tauri::command]
pub fn reset_memories(state: State<AppState>) -> Result<u64, String> {
    let deleted = state.memory.read().map_err(|e| e.to_string())?.clear_memories().map_err(|e| e.to_string())?;
    tracing::warn!("Reset memories: {} memories deleted", deleted);
    Ok(deleted)
}

#[tauri::command]
pub fn search_memories(
    state: State<AppState>,
    query: String,
    limit: Option<usize>,
) -> Result<Vec<MemoryInfo>, String> {
    let results = state
        .memory
        .read()
        .map_err(|e| e.to_string())?
        .search_memories(&query, limit.unwrap_or(10))
        .map_err(|e| e.to_string())?;

    Ok(results
        .into_iter()
        .map(|m| MemoryInfo {
            id: m.id,
            content: m.content,
            weight: m.weight.as_str().to_string(),
            created_at: m.created_at.to_rfc3339(),
        })
        .collect())
}
