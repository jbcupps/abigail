use crate::schema::{
    CREATE_BIRTH, CREATE_MEMORIES, CREATE_SCHEMA_VERSIONS, MIGRATION_V2_CONVERSATION_TURNS,
    MIGRATION_V3_JOB_QUEUE,
};
use abigail_core::AppConfig;
use chrono::Utc;
use rusqlite::Connection;
use std::path::Path;
use std::sync::Mutex;
use thiserror::Error;
use uuid::Uuid;

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub enum MemoryWeight {
    Ephemeral,
    Distilled,
    Crystallized,
}

impl MemoryWeight {
    pub fn as_str(&self) -> &'static str {
        match self {
            MemoryWeight::Ephemeral => "ephemeral",
            MemoryWeight::Distilled => "distilled",
            MemoryWeight::Crystallized => "crystallized",
        }
    }
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct Memory {
    pub id: String,
    pub content: String,
    pub weight: MemoryWeight,
    pub created_at: chrono::DateTime<Utc>,
}

impl Memory {
    pub fn ephemeral(content: String) -> Self {
        Self {
            id: Uuid::new_v4().to_string(),
            content,
            weight: MemoryWeight::Ephemeral,
            created_at: Utc::now(),
        }
    }

    pub fn distilled(content: String) -> Self {
        Self {
            id: Uuid::new_v4().to_string(),
            content,
            weight: MemoryWeight::Distilled,
            created_at: Utc::now(),
        }
    }

    pub fn crystallized(content: String) -> Self {
        Self {
            id: Uuid::new_v4().to_string(),
            content,
            weight: MemoryWeight::Crystallized,
            created_at: Utc::now(),
        }
    }
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct ConversationTurn {
    pub id: String,
    pub session_id: String,
    pub turn_number: u32,
    pub role: String,
    pub content: String,
    pub provider: Option<String>,
    pub model: Option<String>,
    pub tier: Option<String>,
    pub complexity_score: Option<u8>,
    pub token_estimate: Option<u32>,
    pub created_at: chrono::DateTime<Utc>,
}

impl ConversationTurn {
    pub fn new(session_id: &str, role: &str, content: &str) -> Self {
        Self {
            id: Uuid::new_v4().to_string(),
            session_id: session_id.to_string(),
            turn_number: 0,
            role: role.to_string(),
            content: content.to_string(),
            provider: None,
            model: None,
            tier: None,
            complexity_score: None,
            token_estimate: None,
            created_at: Utc::now(),
        }
    }

    pub fn with_metadata(
        mut self,
        provider: Option<String>,
        model: Option<String>,
        tier: Option<String>,
        complexity_score: Option<u8>,
    ) -> Self {
        self.provider = provider;
        self.model = model;
        self.tier = tier;
        self.complexity_score = complexity_score;
        self
    }
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct SessionSummary {
    pub session_id: String,
    pub turn_count: u64,
    pub first_at: String,
    pub last_at: String,
}

#[derive(Error, Debug)]
pub enum StoreError {
    #[error("SQLite error: {0}")]
    Sqlite(#[from] rusqlite::Error),
    #[error("Birth already recorded")]
    BirthAlreadyRecorded,
    #[error("Invalid data: {0}")]
    InvalidData(String),
}

pub type Result<T> = std::result::Result<T, StoreError>;

pub struct MemoryStore {
    conn: Mutex<Connection>,
}

impl MemoryStore {
    pub fn open(path: impl AsRef<Path>) -> Result<Self> {
        let conn = Connection::open(path)?;
        Self::init_conn(&conn)?;
        Ok(Self {
            conn: Mutex::new(conn),
        })
    }

    pub fn open_in_memory() -> Result<Self> {
        let conn = Connection::open_in_memory()?;
        Self::init_conn(&conn)?;
        Ok(Self {
            conn: Mutex::new(conn),
        })
    }

    fn init_conn(conn: &Connection) -> Result<()> {
        conn.execute_batch("PRAGMA journal_mode=WAL;")?;
        conn.execute_batch(CREATE_MEMORIES)?;
        conn.execute_batch(CREATE_BIRTH)?;
        conn.execute_batch(CREATE_SCHEMA_VERSIONS)?;
        Self::run_migrations(conn)?;
        Ok(())
    }

    /// Run pending schema migrations in order. Each migration is applied once and
    /// recorded in `schema_versions`. Version 1 is the baseline (no SQL changes).
    fn run_migrations(conn: &Connection) -> Result<()> {
        let migrations: &[(i64, &str)] = &[
            (1, ""),
            (2, MIGRATION_V2_CONVERSATION_TURNS),
            (3, MIGRATION_V3_JOB_QUEUE),
        ];

        for &(version, sql) in migrations {
            let already_applied: bool = conn.query_row(
                "SELECT COUNT(*) > 0 FROM schema_versions WHERE version = ?1",
                [version],
                |row| row.get(0),
            )?;
            if already_applied {
                continue;
            }
            if !sql.is_empty() {
                conn.execute_batch(sql)?;
            }
            conn.execute(
                "INSERT INTO schema_versions (version, applied_at) VALUES (?1, ?2)",
                rusqlite::params![version, Utc::now().to_rfc3339()],
            )?;
        }
        Ok(())
    }

    /// Return the latest applied schema version, or 0 if no migrations have run.
    pub fn schema_version(&self) -> Result<i64> {
        let conn = self.conn.lock().map_err(|e| {
            rusqlite::Error::ToSqlConversionFailure(Box::new(std::io::Error::other(e.to_string())))
        })?;
        let version: i64 = conn.query_row(
            "SELECT COALESCE(MAX(version), 0) FROM schema_versions",
            [],
            |row| row.get(0),
        )?;
        Ok(version)
    }

    pub fn open_with_config(config: &AppConfig) -> Result<Self> {
        if let Some(parent) = config.db_path.parent() {
            std::fs::create_dir_all(parent)
                .map_err(|e| rusqlite::Error::ToSqlConversionFailure(Box::new(e)))?;
        }
        Self::open(&config.db_path)
    }

    pub fn has_birth(&self) -> Result<bool> {
        let conn = self.conn.lock().map_err(|e| {
            rusqlite::Error::ToSqlConversionFailure(Box::new(std::io::Error::other(e.to_string())))
        })?;
        let count: i64 = conn.query_row("SELECT COUNT(*) FROM birth WHERE id = 1", [], |row| {
            row.get(0)
        })?;
        Ok(count > 0)
    }

    pub fn record_birth(&self, memory: &Memory) -> Result<()> {
        if self.has_birth()? {
            return Err(StoreError::BirthAlreadyRecorded);
        }
        let conn = self.conn.lock().map_err(|e| {
            rusqlite::Error::ToSqlConversionFailure(Box::new(std::io::Error::other(e.to_string())))
        })?;
        conn.execute(
            "INSERT INTO birth (id, content, created_at) VALUES (1, ?1, ?2)",
            [
                memory.content.as_str(),
                memory.created_at.to_rfc3339().as_str(),
            ],
        )?;
        Ok(())
    }

    pub fn insert_memory(&self, memory: &Memory) -> Result<()> {
        let conn = self.conn.lock().map_err(|e| {
            rusqlite::Error::ToSqlConversionFailure(Box::new(std::io::Error::other(e.to_string())))
        })?;
        conn.execute(
            "INSERT INTO memories (id, content, weight, created_at) VALUES (?1, ?2, ?3, ?4)",
            rusqlite::params![
                memory.id,
                memory.content,
                memory.weight.as_str(),
                memory.created_at.to_rfc3339(),
            ],
        )?;
        Ok(())
    }

    /// Count total memories in the store.
    pub fn count_memories(&self) -> Result<u64> {
        let conn = self.conn.lock().map_err(|e| {
            rusqlite::Error::ToSqlConversionFailure(Box::new(std::io::Error::other(e.to_string())))
        })?;
        let count: i64 = conn.query_row("SELECT COUNT(*) FROM memories", [], |row| row.get(0))?;
        Ok(count as u64)
    }

    /// Run VACUUM to reclaim space and optimize the database.
    pub fn vacuum(&self) -> Result<()> {
        let conn = self.conn.lock().map_err(|e| {
            rusqlite::Error::ToSqlConversionFailure(Box::new(std::io::Error::other(e.to_string())))
        })?;
        conn.execute("VACUUM", [])?;
        Ok(())
    }

    /// Clear all memories but keep the birth record.
    pub fn clear_memories(&self) -> Result<u64> {
        let conn = self.conn.lock().map_err(|e| {
            rusqlite::Error::ToSqlConversionFailure(Box::new(std::io::Error::other(e.to_string())))
        })?;
        let deleted = conn.execute("DELETE FROM memories", [])?;
        Ok(deleted as u64)
    }

    /// Search memories by keyword (case-insensitive LIKE).
    pub fn search_memories(&self, query: &str, limit: usize) -> Result<Vec<Memory>> {
        let conn = self.conn.lock().map_err(|e| {
            rusqlite::Error::ToSqlConversionFailure(Box::new(std::io::Error::other(e.to_string())))
        })?;
        let mut stmt = conn.prepare(
            "SELECT id, content, weight, created_at FROM memories \
             WHERE content LIKE ?1 \
             ORDER BY created_at DESC LIMIT ?2",
        )?;
        let pattern = format!("%{}%", query);
        let rows = stmt.query_map(rusqlite::params![pattern, limit as i64], |row| {
            let created_at: String = row.get(3)?;
            let created_at = chrono::DateTime::parse_from_rfc3339(&created_at)
                .map(|dt| dt.with_timezone(&Utc))
                .map_err(|e| rusqlite::Error::ToSqlConversionFailure(Box::new(e)))?;
            let weight_str: String = row.get(2)?;
            let weight = match weight_str.as_str() {
                "ephemeral" => MemoryWeight::Ephemeral,
                "distilled" => MemoryWeight::Distilled,
                "crystallized" => MemoryWeight::Crystallized,
                w => {
                    return Err(rusqlite::Error::ToSqlConversionFailure(Box::new(
                        StoreError::InvalidData(format!("unknown weight: {}", w)),
                    )))
                }
            };
            Ok(Memory {
                id: row.get(0)?,
                content: row.get(1)?,
                weight,
                created_at,
            })
        })?;
        let mut out = Vec::new();
        for row in rows {
            out.push(row?);
        }
        Ok(out)
    }

    // ── Conversation Turns ──────────────────────────────────────────

    pub fn insert_turn(&self, turn: &ConversationTurn) -> Result<()> {
        let conn = self.conn.lock().map_err(|e| {
            rusqlite::Error::ToSqlConversionFailure(Box::new(std::io::Error::other(e.to_string())))
        })?;
        conn.execute(
            "INSERT INTO conversation_turns \
             (id, session_id, turn_number, role, content, provider, model, tier, \
              complexity_score, token_estimate, created_at) \
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11)",
            rusqlite::params![
                turn.id,
                turn.session_id,
                turn.turn_number,
                turn.role,
                turn.content,
                turn.provider,
                turn.model,
                turn.tier,
                turn.complexity_score.map(|v| v as i64),
                turn.token_estimate.map(|v| v as i64),
                turn.created_at.to_rfc3339(),
            ],
        )?;
        Ok(())
    }

    /// Number of turns recorded for a given session.
    pub fn session_turn_count(&self, session_id: &str) -> Result<u64> {
        let conn = self.conn.lock().map_err(|e| {
            rusqlite::Error::ToSqlConversionFailure(Box::new(std::io::Error::other(e.to_string())))
        })?;
        let count: i64 = conn.query_row(
            "SELECT COUNT(*) FROM conversation_turns WHERE session_id = ?1",
            [session_id],
            |row| row.get(0),
        )?;
        Ok(count as u64)
    }

    /// Total turns across all sessions (used for archive scheduling).
    pub fn total_turn_count(&self) -> Result<u64> {
        let conn = self.conn.lock().map_err(|e| {
            rusqlite::Error::ToSqlConversionFailure(Box::new(std::io::Error::other(e.to_string())))
        })?;
        let count: i64 = conn.query_row("SELECT COUNT(*) FROM conversation_turns", [], |row| {
            row.get(0)
        })?;
        Ok(count as u64)
    }

    /// Recent turns from a specific session, ordered oldest-first.
    pub fn recent_turns(&self, session_id: &str, limit: usize) -> Result<Vec<ConversationTurn>> {
        let conn = self.conn.lock().map_err(|e| {
            rusqlite::Error::ToSqlConversionFailure(Box::new(std::io::Error::other(e.to_string())))
        })?;
        let mut stmt = conn.prepare(
            "SELECT id, session_id, turn_number, role, content, provider, model, \
                    tier, complexity_score, token_estimate, created_at \
             FROM conversation_turns WHERE session_id = ?1 \
             ORDER BY turn_number DESC LIMIT ?2",
        )?;
        let rows = stmt.query_map(rusqlite::params![session_id, limit as i64], Self::map_turn)?;
        let mut out: Vec<ConversationTurn> = rows.filter_map(|r| r.ok()).collect();
        out.reverse();
        Ok(out)
    }

    /// Recent turns across all sessions, most-recent first.
    pub fn recent_turns_all_sessions(&self, limit: usize) -> Result<Vec<ConversationTurn>> {
        let conn = self.conn.lock().map_err(|e| {
            rusqlite::Error::ToSqlConversionFailure(Box::new(std::io::Error::other(e.to_string())))
        })?;
        let mut stmt = conn.prepare(
            "SELECT id, session_id, turn_number, role, content, provider, model, \
                    tier, complexity_score, token_estimate, created_at \
             FROM conversation_turns ORDER BY created_at DESC LIMIT ?1",
        )?;
        let rows = stmt.query_map([limit as i64], Self::map_turn)?;
        Ok(rows.filter_map(|r| r.ok()).collect())
    }

    /// Full-text search across conversation turns (case-insensitive LIKE).
    pub fn search_turns(&self, query: &str, limit: usize) -> Result<Vec<ConversationTurn>> {
        let conn = self.conn.lock().map_err(|e| {
            rusqlite::Error::ToSqlConversionFailure(Box::new(std::io::Error::other(e.to_string())))
        })?;
        let pattern = format!("%{}%", query);
        let mut stmt = conn.prepare(
            "SELECT id, session_id, turn_number, role, content, provider, model, \
                    tier, complexity_score, token_estimate, created_at \
             FROM conversation_turns WHERE content LIKE ?1 \
             ORDER BY created_at DESC LIMIT ?2",
        )?;
        let rows = stmt.query_map(rusqlite::params![pattern, limit as i64], Self::map_turn)?;
        Ok(rows.filter_map(|r| r.ok()).collect())
    }

    /// List distinct sessions with aggregated metadata.
    pub fn list_sessions(&self, limit: usize) -> Result<Vec<SessionSummary>> {
        let conn = self.conn.lock().map_err(|e| {
            rusqlite::Error::ToSqlConversionFailure(Box::new(std::io::Error::other(e.to_string())))
        })?;
        let mut stmt = conn.prepare(
            "SELECT session_id, COUNT(*) as cnt, \
                    MIN(created_at) as first_at, MAX(created_at) as last_at \
             FROM conversation_turns GROUP BY session_id \
             ORDER BY last_at DESC LIMIT ?1",
        )?;
        let rows = stmt.query_map([limit as i64], |row| {
            Ok(SessionSummary {
                session_id: row.get(0)?,
                turn_count: row.get::<_, i64>(1)? as u64,
                first_at: row.get(2)?,
                last_at: row.get(3)?,
            })
        })?;
        Ok(rows.filter_map(|r| r.ok()).collect())
    }

    /// Retrieve all turns (for archive export).
    pub fn all_turns(&self) -> Result<Vec<ConversationTurn>> {
        let conn = self.conn.lock().map_err(|e| {
            rusqlite::Error::ToSqlConversionFailure(Box::new(std::io::Error::other(e.to_string())))
        })?;
        let mut stmt = conn.prepare(
            "SELECT id, session_id, turn_number, role, content, provider, model, \
                    tier, complexity_score, token_estimate, created_at \
             FROM conversation_turns ORDER BY created_at ASC",
        )?;
        let rows = stmt.query_map([], Self::map_turn)?;
        Ok(rows.filter_map(|r| r.ok()).collect())
    }

    /// Retrieve all memories (for archive export).
    pub fn all_memories(&self) -> Result<Vec<Memory>> {
        let conn = self.conn.lock().map_err(|e| {
            rusqlite::Error::ToSqlConversionFailure(Box::new(std::io::Error::other(e.to_string())))
        })?;
        let mut stmt = conn.prepare(
            "SELECT id, content, weight, created_at FROM memories ORDER BY created_at ASC",
        )?;
        let rows = stmt.query_map([], |row| {
            let created_at: String = row.get(3)?;
            let created_at = chrono::DateTime::parse_from_rfc3339(&created_at)
                .map(|dt| dt.with_timezone(&Utc))
                .map_err(|e| rusqlite::Error::ToSqlConversionFailure(Box::new(e)))?;
            let weight_str: String = row.get(2)?;
            let weight = match weight_str.as_str() {
                "ephemeral" => MemoryWeight::Ephemeral,
                "distilled" => MemoryWeight::Distilled,
                "crystallized" => MemoryWeight::Crystallized,
                w => {
                    return Err(rusqlite::Error::ToSqlConversionFailure(Box::new(
                        StoreError::InvalidData(format!("unknown weight: {}", w)),
                    )))
                }
            };
            Ok(Memory {
                id: row.get(0)?,
                content: row.get(1)?,
                weight,
                created_at,
            })
        })?;
        Ok(rows.filter_map(|r| r.ok()).collect())
    }

    fn map_turn(row: &rusqlite::Row<'_>) -> rusqlite::Result<ConversationTurn> {
        let created_at: String = row.get(10)?;
        let created_at = chrono::DateTime::parse_from_rfc3339(&created_at)
            .map(|dt| dt.with_timezone(&Utc))
            .map_err(|e| rusqlite::Error::ToSqlConversionFailure(Box::new(e)))?;
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
    }

    // ── Idempotent inserts (for backup import) ────────────────────

    /// Insert a conversation turn, ignoring if the ID already exists.
    /// Returns `true` if a new row was inserted.
    pub fn insert_turn_or_ignore(&self, turn: &ConversationTurn) -> Result<bool> {
        let conn = self.conn.lock().map_err(|e| {
            rusqlite::Error::ToSqlConversionFailure(Box::new(std::io::Error::other(e.to_string())))
        })?;
        let changed = conn.execute(
            "INSERT OR IGNORE INTO conversation_turns \
             (id, session_id, turn_number, role, content, provider, model, tier, \
              complexity_score, token_estimate, created_at) \
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11)",
            rusqlite::params![
                turn.id,
                turn.session_id,
                turn.turn_number,
                turn.role,
                turn.content,
                turn.provider,
                turn.model,
                turn.tier,
                turn.complexity_score.map(|v| v as i64),
                turn.token_estimate.map(|v| v as i64),
                turn.created_at.to_rfc3339(),
            ],
        )?;
        Ok(changed > 0)
    }

    /// Insert a memory, ignoring if the ID already exists.
    /// Returns `true` if a new row was inserted.
    pub fn insert_memory_or_ignore(&self, memory: &Memory) -> Result<bool> {
        let conn = self.conn.lock().map_err(|e| {
            rusqlite::Error::ToSqlConversionFailure(Box::new(std::io::Error::other(e.to_string())))
        })?;
        let changed = conn.execute(
            "INSERT OR IGNORE INTO memories (id, content, weight, created_at) \
             VALUES (?1, ?2, ?3, ?4)",
            rusqlite::params![
                memory.id,
                memory.content,
                memory.weight.as_str(),
                memory.created_at.to_rfc3339(),
            ],
        )?;
        Ok(changed > 0)
    }

    // ── Memories ──────────────────────────────────────────────────

    /// Recent memories (MVP: by created_at DESC; sqlite-vec stubbed).
    pub fn recent_memories(&self, limit: usize) -> Result<Vec<Memory>> {
        let conn = self.conn.lock().map_err(|e| {
            rusqlite::Error::ToSqlConversionFailure(Box::new(std::io::Error::other(e.to_string())))
        })?;
        let mut stmt = conn.prepare(
            "SELECT id, content, weight, created_at FROM memories ORDER BY created_at DESC LIMIT ?1",
        )?;
        let rows = stmt.query_map([limit as i64], |row| {
            let created_at: String = row.get(3)?;
            let created_at = chrono::DateTime::parse_from_rfc3339(&created_at)
                .map(|dt| dt.with_timezone(&Utc))
                .map_err(|e| rusqlite::Error::ToSqlConversionFailure(Box::new(e)))?;
            let weight_str: String = row.get(2)?;
            let weight = match weight_str.as_str() {
                "ephemeral" => MemoryWeight::Ephemeral,
                "distilled" => MemoryWeight::Distilled,
                "crystallized" => MemoryWeight::Crystallized,
                w => {
                    return Err(rusqlite::Error::ToSqlConversionFailure(Box::new(
                        StoreError::InvalidData(format!("unknown weight: {}", w)),
                    )))
                }
            };
            Ok(Memory {
                id: row.get(0)?,
                content: row.get(1)?,
                weight,
                created_at,
            })
        })?;
        let mut out = Vec::new();
        for row in rows {
            out.push(row?);
        }
        Ok(out)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_birth_record() {
        let store = MemoryStore::open_in_memory().unwrap();
        assert!(!store.has_birth().unwrap());
        store
            .record_birth(&Memory::crystallized("I was born".into()))
            .unwrap();
        assert!(store.has_birth().unwrap());
    }

    #[test]
    fn test_double_birth_rejected() {
        let store = MemoryStore::open_in_memory().unwrap();
        store
            .record_birth(&Memory::crystallized("I was born".into()))
            .unwrap();
        let result = store.record_birth(&Memory::crystallized("Born again".into()));
        assert!(result.is_err());
        match result.unwrap_err() {
            StoreError::BirthAlreadyRecorded => {}
            e => panic!("Expected BirthAlreadyRecorded, got: {:?}", e),
        }
    }

    #[test]
    fn test_insert_and_retrieve_ephemeral_memory() {
        let store = MemoryStore::open_in_memory().unwrap();

        let mem = Memory::ephemeral("user: Hello | assistant: Hi there".into());
        store.insert_memory(&mem).unwrap();

        let recent = store.recent_memories(10).unwrap();
        assert_eq!(recent.len(), 1);
        assert_eq!(recent[0].content, "user: Hello | assistant: Hi there");
        assert_eq!(recent[0].weight, MemoryWeight::Ephemeral);
    }

    #[test]
    fn test_insert_multiple_weights() {
        let store = MemoryStore::open_in_memory().unwrap();

        store
            .insert_memory(&Memory::ephemeral("ephemeral msg".into()))
            .unwrap();
        store
            .insert_memory(&Memory::distilled("distilled msg".into()))
            .unwrap();
        store
            .insert_memory(&Memory::crystallized("crystallized msg".into()))
            .unwrap();

        let recent = store.recent_memories(10).unwrap();
        assert_eq!(recent.len(), 3);

        // Verify all weight tiers are stored and retrieved correctly
        let weights: Vec<&MemoryWeight> = recent.iter().map(|m| &m.weight).collect();
        assert!(weights.contains(&&MemoryWeight::Ephemeral));
        assert!(weights.contains(&&MemoryWeight::Distilled));
        assert!(weights.contains(&&MemoryWeight::Crystallized));
    }

    #[test]
    fn test_recent_memories_ordering() {
        let store = MemoryStore::open_in_memory().unwrap();

        // Insert in order — recent_memories should return most recent first
        store
            .insert_memory(&Memory::ephemeral("first".into()))
            .unwrap();
        // Small delay to ensure different timestamps
        std::thread::sleep(std::time::Duration::from_millis(10));
        store
            .insert_memory(&Memory::ephemeral("second".into()))
            .unwrap();
        std::thread::sleep(std::time::Duration::from_millis(10));
        store
            .insert_memory(&Memory::ephemeral("third".into()))
            .unwrap();

        let recent = store.recent_memories(10).unwrap();
        assert_eq!(recent.len(), 3);
        assert_eq!(recent[0].content, "third");
        assert_eq!(recent[1].content, "second");
        assert_eq!(recent[2].content, "first");
    }

    #[test]
    fn test_recent_memories_limit() {
        let store = MemoryStore::open_in_memory().unwrap();

        for i in 0..10 {
            store
                .insert_memory(&Memory::ephemeral(format!("msg {}", i)))
                .unwrap();
        }

        let recent = store.recent_memories(3).unwrap();
        assert_eq!(recent.len(), 3);
    }

    #[test]
    fn test_recent_memories_empty_store() {
        let store = MemoryStore::open_in_memory().unwrap();
        let recent = store.recent_memories(10).unwrap();
        assert!(recent.is_empty());
    }

    #[test]
    fn test_memory_ids_are_unique() {
        let store = MemoryStore::open_in_memory().unwrap();

        let m1 = Memory::ephemeral("msg1".into());
        let m2 = Memory::ephemeral("msg2".into());
        assert_ne!(m1.id, m2.id);

        store.insert_memory(&m1).unwrap();
        store.insert_memory(&m2).unwrap();

        let recent = store.recent_memories(10).unwrap();
        assert_eq!(recent.len(), 2);
    }

    #[test]
    fn test_file_backed_store() {
        let tmp = std::env::temp_dir().join("abigail_memory_file_test");
        let _ = std::fs::remove_dir_all(&tmp);
        std::fs::create_dir_all(&tmp).unwrap();

        let db_path = tmp.join("test.db");

        // Open, insert, close
        {
            let store = MemoryStore::open(&db_path).unwrap();
            store
                .insert_memory(&Memory::ephemeral("persisted msg".into()))
                .unwrap();
            store
                .record_birth(&Memory::crystallized("born".into()))
                .unwrap();
        }

        // Reopen and verify persistence
        {
            let store = MemoryStore::open(&db_path).unwrap();
            assert!(store.has_birth().unwrap());
            let recent = store.recent_memories(10).unwrap();
            assert_eq!(recent.len(), 1);
            assert_eq!(recent[0].content, "persisted msg");
        }

        let _ = std::fs::remove_dir_all(&tmp);
    }

    #[test]
    fn test_count_memories() {
        let store = MemoryStore::open_in_memory().unwrap();
        assert_eq!(store.count_memories().unwrap(), 0);

        store
            .insert_memory(&Memory::ephemeral("msg1".into()))
            .unwrap();
        assert_eq!(store.count_memories().unwrap(), 1);

        store
            .insert_memory(&Memory::ephemeral("msg2".into()))
            .unwrap();
        store
            .insert_memory(&Memory::distilled("msg3".into()))
            .unwrap();
        assert_eq!(store.count_memories().unwrap(), 3);
    }

    #[test]
    fn test_clear_memories() {
        let store = MemoryStore::open_in_memory().unwrap();

        // Add some memories and a birth record
        store
            .insert_memory(&Memory::ephemeral("msg1".into()))
            .unwrap();
        store
            .insert_memory(&Memory::ephemeral("msg2".into()))
            .unwrap();
        store
            .record_birth(&Memory::crystallized("born".into()))
            .unwrap();

        assert_eq!(store.count_memories().unwrap(), 2);
        assert!(store.has_birth().unwrap());

        // Clear memories
        let deleted = store.clear_memories().unwrap();
        assert_eq!(deleted, 2);

        // Memories gone, but birth still there
        assert_eq!(store.count_memories().unwrap(), 0);
        assert!(store.has_birth().unwrap());
    }

    #[test]
    fn test_vacuum() {
        let store = MemoryStore::open_in_memory().unwrap();

        // Insert and delete some data
        for i in 0..10 {
            store
                .insert_memory(&Memory::ephemeral(format!("msg {}", i)))
                .unwrap();
        }
        store.clear_memories().unwrap();

        // VACUUM should succeed
        assert!(store.vacuum().is_ok());
    }

    #[test]
    fn test_insert_turn_or_ignore_idempotent() {
        let store = MemoryStore::open_in_memory().unwrap();
        let turn = ConversationTurn::new("sess1", "user", "hello");

        // First insert succeeds
        assert!(store.insert_turn_or_ignore(&turn).unwrap());
        assert_eq!(store.total_turn_count().unwrap(), 1);

        // Second insert with same ID is silently ignored
        assert!(!store.insert_turn_or_ignore(&turn).unwrap());
        assert_eq!(store.total_turn_count().unwrap(), 1);
    }

    #[test]
    fn test_insert_memory_or_ignore_idempotent() {
        let store = MemoryStore::open_in_memory().unwrap();
        let mem = Memory::ephemeral("remember this".into());

        assert!(store.insert_memory_or_ignore(&mem).unwrap());
        assert_eq!(store.count_memories().unwrap(), 1);

        // Duplicate is ignored
        assert!(!store.insert_memory_or_ignore(&mem).unwrap());
        assert_eq!(store.count_memories().unwrap(), 1);
    }
}
