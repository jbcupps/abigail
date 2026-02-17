//! PostgreSQL memory backend — optional alternative to SQLite.
//!
//! Feature-gated behind `postgres` Cargo feature.
//! Implements the same operations as the SQLite MemoryStore but against Postgres.

#[cfg(feature = "postgres")]
use chrono::Utc;
#[cfg(feature = "postgres")]
use sqlx::postgres::PgPoolOptions;
#[cfg(feature = "postgres")]
use sqlx::PgPool;

#[cfg(feature = "postgres")]
use crate::store::{Memory, MemoryWeight, StoreError};

/// PostgreSQL-backed memory store.
#[cfg(feature = "postgres")]
pub struct PostgresMemoryStore {
    pool: PgPool,
    agent_id: String,
}

#[cfg(feature = "postgres")]
impl PostgresMemoryStore {
    /// Connect to Postgres and run migrations.
    pub async fn connect(database_url: &str, agent_id: &str) -> Result<Self, StoreError> {
        let pool = PgPoolOptions::new()
            .max_connections(5)
            .connect(database_url)
            .await
            .map_err(|e| StoreError::InvalidData(format!("Postgres connection failed: {}", e)))?;

        let store = Self {
            pool,
            agent_id: agent_id.to_string(),
        };

        store.run_migrations().await?;
        Ok(store)
    }

    /// Run schema migrations.
    async fn run_migrations(&self) -> Result<(), StoreError> {
        // Migration 1: Base memories + birth tables
        sqlx::query(
            r#"
            CREATE TABLE IF NOT EXISTS memories (
                id TEXT PRIMARY KEY,
                agent_id TEXT NOT NULL,
                content TEXT NOT NULL,
                weight TEXT NOT NULL CHECK (weight IN ('ephemeral', 'distilled', 'crystallized')),
                created_at TIMESTAMPTZ NOT NULL DEFAULT NOW()
            )
            "#,
        )
        .execute(&self.pool)
        .await
        .map_err(|e| StoreError::InvalidData(format!("Migration 1a failed: {}", e)))?;

        sqlx::query(
            r#"
            CREATE TABLE IF NOT EXISTS birth (
                id TEXT PRIMARY KEY,
                agent_id TEXT NOT NULL,
                content TEXT NOT NULL,
                created_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
                UNIQUE(agent_id)
            )
            "#,
        )
        .execute(&self.pool)
        .await
        .map_err(|e| StoreError::InvalidData(format!("Migration 1b failed: {}", e)))?;

        // Migration 2: Embeddings table (requires pgvector extension)
        // Silently skip if pgvector is not installed
        let _ = sqlx::query(
            r#"
            CREATE EXTENSION IF NOT EXISTS vector;
            CREATE TABLE IF NOT EXISTS memory_embeddings (
                memory_id TEXT PRIMARY KEY REFERENCES memories(id) ON DELETE CASCADE,
                embedding vector(384) NOT NULL
            );
            CREATE INDEX IF NOT EXISTS idx_memory_embeddings_hnsw
                ON memory_embeddings USING hnsw (embedding vector_cosine_ops);
            "#,
        )
        .execute(&self.pool)
        .await;

        // Migration 3: Graph edges table
        sqlx::query(
            r#"
            CREATE TABLE IF NOT EXISTS memory_edges (
                id TEXT PRIMARY KEY,
                from_id TEXT NOT NULL REFERENCES memories(id) ON DELETE CASCADE,
                to_id TEXT NOT NULL REFERENCES memories(id) ON DELETE CASCADE,
                edge_type TEXT NOT NULL CHECK (edge_type IN ('derived_from', 'critiqued_by', 'refined_to')),
                weight REAL NOT NULL DEFAULT 1.0,
                metadata JSONB,
                created_at TIMESTAMPTZ NOT NULL DEFAULT NOW()
            )
            "#,
        )
        .execute(&self.pool)
        .await
        .map_err(|e| StoreError::InvalidData(format!("Migration 3 failed: {}", e)))?;

        Ok(())
    }

    /// Store a memory.
    pub async fn store(&self, memory: &Memory) -> Result<(), StoreError> {
        sqlx::query(
            "INSERT INTO memories (id, agent_id, content, weight, created_at) VALUES ($1, $2, $3, $4, $5)",
        )
        .bind(&memory.id)
        .bind(&self.agent_id)
        .bind(&memory.content)
        .bind(memory.weight.as_str())
        .bind(memory.created_at)
        .execute(&self.pool)
        .await
        .map_err(|e| StoreError::InvalidData(format!("Store failed: {}", e)))?;

        Ok(())
    }

    /// Fetch recent memories.
    pub async fn recent(&self, limit: i64) -> Result<Vec<Memory>, StoreError> {
        let rows = sqlx::query_as::<_, MemoryRow>(
            "SELECT id, content, weight, created_at FROM memories WHERE agent_id = $1 ORDER BY created_at DESC LIMIT $2",
        )
        .bind(&self.agent_id)
        .bind(limit)
        .fetch_all(&self.pool)
        .await
        .map_err(|e| StoreError::InvalidData(format!("Fetch failed: {}", e)))?;

        Ok(rows.into_iter().map(Into::into).collect())
    }

    /// Check if birth has been recorded for this agent.
    pub async fn has_birth(&self) -> Result<bool, StoreError> {
        let count: (i64,) = sqlx::query_as("SELECT COUNT(*) FROM birth WHERE agent_id = $1")
            .bind(&self.agent_id)
            .fetch_one(&self.pool)
            .await
            .map_err(|e| StoreError::InvalidData(format!("Birth check failed: {}", e)))?;

        Ok(count.0 > 0)
    }

    /// Record birth.
    pub async fn record_birth(&self, memory: &Memory) -> Result<(), StoreError> {
        if self.has_birth().await? {
            return Err(StoreError::BirthAlreadyRecorded);
        }

        sqlx::query("INSERT INTO birth (id, agent_id, content) VALUES ($1, $2, $3)")
            .bind(&memory.id)
            .bind(&self.agent_id)
            .bind(&memory.content)
            .execute(&self.pool)
            .await
            .map_err(|e| StoreError::InvalidData(format!("Record birth failed: {}", e)))?;

        Ok(())
    }

    /// Search memories by content substring.
    pub async fn search(&self, query: &str, limit: i64) -> Result<Vec<Memory>, StoreError> {
        let pattern = format!("%{}%", query);
        let rows = sqlx::query_as::<_, MemoryRow>(
            "SELECT id, content, weight, created_at FROM memories WHERE agent_id = $1 AND content ILIKE $2 ORDER BY created_at DESC LIMIT $3",
        )
        .bind(&self.agent_id)
        .bind(&pattern)
        .bind(limit)
        .fetch_all(&self.pool)
        .await
        .map_err(|e| StoreError::InvalidData(format!("Search failed: {}", e)))?;

        Ok(rows.into_iter().map(Into::into).collect())
    }

    /// Delete a memory by ID.
    pub async fn delete(&self, id: &str) -> Result<bool, StoreError> {
        let result = sqlx::query("DELETE FROM memories WHERE id = $1 AND agent_id = $2")
            .bind(id)
            .bind(&self.agent_id)
            .execute(&self.pool)
            .await
            .map_err(|e| StoreError::InvalidData(format!("Delete failed: {}", e)))?;

        Ok(result.rows_affected() > 0)
    }
}

/// Internal row type for sqlx mapping.
#[cfg(feature = "postgres")]
#[derive(sqlx::FromRow)]
struct MemoryRow {
    id: String,
    content: String,
    weight: String,
    created_at: chrono::DateTime<Utc>,
}

#[cfg(feature = "postgres")]
impl From<MemoryRow> for Memory {
    fn from(row: MemoryRow) -> Self {
        let weight = match row.weight.as_str() {
            "ephemeral" => MemoryWeight::Ephemeral,
            "distilled" => MemoryWeight::Distilled,
            "crystallized" => MemoryWeight::Crystallized,
            _ => MemoryWeight::Ephemeral,
        };
        Memory {
            id: row.id,
            content: row.content,
            weight,
            created_at: row.created_at,
        }
    }
}

// When postgres feature is not enabled, provide a stub
#[cfg(not(feature = "postgres"))]
pub struct PostgresMemoryStore;

#[cfg(not(feature = "postgres"))]
impl PostgresMemoryStore {
    pub fn unavailable() -> &'static str {
        "PostgreSQL backend requires the 'postgres' feature flag"
    }
}
