use crate::schema::{CREATE_BIRTH, CREATE_MEMORIES};
use ao_core::AppConfig;
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

#[derive(Debug, Clone)]
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
        Ok(())
    }

    pub fn open_with_config(config: &AppConfig) -> Result<Self> {
        if let Some(parent) = config.db_path.parent() {
            std::fs::create_dir_all(parent).map_err(|e| {
                rusqlite::Error::ToSqlConversionFailure(Box::new(e))
            })?;
        }
        Self::open(&config.db_path)
    }

    pub fn has_birth(&self) -> Result<bool> {
        let conn = self.conn.lock().map_err(|e| {
            rusqlite::Error::ToSqlConversionFailure(Box::new(std::io::Error::new(
                std::io::ErrorKind::Other,
                e.to_string(),
            )))
        })?;
        let count: i64 = conn.query_row(
            "SELECT COUNT(*) FROM birth WHERE id = 1",
            [],
            |row| row.get(0),
        )?;
        Ok(count > 0)
    }

    pub fn record_birth(&self, memory: &Memory) -> Result<()> {
        if self.has_birth()? {
            return Err(StoreError::BirthAlreadyRecorded);
        }
        let conn = self.conn.lock().map_err(|e| {
            rusqlite::Error::ToSqlConversionFailure(Box::new(std::io::Error::new(
                std::io::ErrorKind::Other,
                e.to_string(),
            )))
        })?;
        conn.execute(
            "INSERT INTO birth (id, content, created_at) VALUES (1, ?1, ?2)",
            [memory.content.as_str(), memory.created_at.to_rfc3339().as_str()],
        )?;
        Ok(())
    }

    pub fn insert_memory(&self, memory: &Memory) -> Result<()> {
        let conn = self.conn.lock().map_err(|e| {
            rusqlite::Error::ToSqlConversionFailure(Box::new(std::io::Error::new(
                std::io::ErrorKind::Other,
                e.to_string(),
            )))
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

    /// Recent memories (MVP: by created_at DESC; sqlite-vec stubbed).
    pub fn recent_memories(&self, limit: usize) -> Result<Vec<Memory>> {
        let conn = self.conn.lock().map_err(|e| {
            rusqlite::Error::ToSqlConversionFailure(Box::new(std::io::Error::new(
                std::io::ErrorKind::Other,
                e.to_string(),
            )))
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
                w => return Err(rusqlite::Error::ToSqlConversionFailure(Box::new(
                    StoreError::InvalidData(format!("unknown weight: {}", w)),
                ))),
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

        store.insert_memory(&Memory::ephemeral("ephemeral msg".into())).unwrap();
        store.insert_memory(&Memory::distilled("distilled msg".into())).unwrap();
        store.insert_memory(&Memory::crystallized("crystallized msg".into())).unwrap();

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
        store.insert_memory(&Memory::ephemeral("first".into())).unwrap();
        // Small delay to ensure different timestamps
        std::thread::sleep(std::time::Duration::from_millis(10));
        store.insert_memory(&Memory::ephemeral("second".into())).unwrap();
        std::thread::sleep(std::time::Duration::from_millis(10));
        store.insert_memory(&Memory::ephemeral("third".into())).unwrap();

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
            store.insert_memory(&Memory::ephemeral(format!("msg {}", i))).unwrap();
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
        let tmp = std::env::temp_dir().join("ao_memory_file_test");
        let _ = std::fs::remove_dir_all(&tmp);
        std::fs::create_dir_all(&tmp).unwrap();

        let db_path = tmp.join("test.db");

        // Open, insert, close
        {
            let store = MemoryStore::open(&db_path).unwrap();
            store.insert_memory(&Memory::ephemeral("persisted msg".into())).unwrap();
            store.record_birth(&Memory::crystallized("born".into())).unwrap();
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
}
