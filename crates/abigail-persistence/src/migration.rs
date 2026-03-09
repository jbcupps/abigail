use crate::{EntityScope, PersistenceHandle};
use abigail_core::AppConfig;
use chrono::Utc;
use serde::{Deserialize, Serialize};
use serde_json::json;
use sqlx::sqlite::SqliteConnectOptions;
use sqlx::{ConnectOptions, Connection, Row, SqliteConnection};
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct MigrationReport {
    pub started_at: String,
    pub completed_at: String,
    pub imported_turns: u64,
    pub imported_memories: u64,
    pub imported_jobs: u64,
    pub imported_calendar_events: u64,
    pub imported_kb_entries: u64,
    pub migrated_sources: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct RecoverySnapshot {
    pub archived_paths: Vec<String>,
    pub archived_at: String,
}

pub fn migrate_legacy_layout(config: &AppConfig) -> anyhow::Result<MigrationReport> {
    let started_at = Utc::now().to_rfc3339();
    let hive_root = infer_hive_root(config);
    let shared_path = hive_root.join("memory.db");
    let hive_handle = PersistenceHandle::open(&shared_path, EntityScope::Hive)?;
    let mut report = MigrationReport {
        started_at,
        ..MigrationReport::default()
    };

    let legacy_db = config.db_path.clone();
    if legacy_db.exists()
        && legacy_db
            .file_name()
            .and_then(|name| name.to_str())
            .is_some_and(|name| name != "memory.db")
    {
        let handle = PersistenceHandle::open(&shared_path, scope_for_config(config))?;
        import_legacy_memory_store(&handle, &legacy_db, &mut report)?;
        report
            .migrated_sources
            .push(legacy_db.to_string_lossy().to_string());
        archive_legacy_source(&legacy_db)?;
    }

    let jobs_db = config.data_dir.join("jobs.db");
    if jobs_db.exists() {
        import_legacy_jobs(&hive_handle, &jobs_db, &mut report)?;
        report
            .migrated_sources
            .push(jobs_db.to_string_lossy().to_string());
        archive_legacy_source(&jobs_db)?;
    }

    let calendar_db = config.data_dir.join("calendar.db");
    if calendar_db.exists() {
        let handle = PersistenceHandle::open(&shared_path, scope_for_config(config))?;
        import_calendar(&handle, &calendar_db, &mut report)?;
        report
            .migrated_sources
            .push(calendar_db.to_string_lossy().to_string());
        archive_legacy_source(&calendar_db)?;
    }

    let kb_db = config.data_dir.join("kb.db");
    if kb_db.exists() {
        let handle = PersistenceHandle::open(&shared_path, scope_for_config(config))?;
        import_knowledge_base(&handle, &kb_db, &mut report)?;
        report
            .migrated_sources
            .push(kb_db.to_string_lossy().to_string());
        archive_legacy_source(&kb_db)?;
    }

    report.completed_at = Utc::now().to_rfc3339();
    if !report.migrated_sources.is_empty() {
        hive_handle.upsert("migration_log", "legacy_sqlite_import", &report)?;
    }
    Ok(report)
}

fn infer_hive_root(config: &AppConfig) -> PathBuf {
    config
        .data_dir
        .parent()
        .and_then(|parent| parent.parent())
        .map(Path::to_path_buf)
        .unwrap_or_else(|| config.data_dir.clone())
}

fn scope_for_config(config: &AppConfig) -> EntityScope {
    if config.is_hive {
        EntityScope::Hive
    } else {
        let entity_id = config
            .data_dir
            .file_name()
            .and_then(|name| name.to_str())
            .unwrap_or("default")
            .to_string();
        EntityScope::Entity(entity_id)
    }
}

fn import_legacy_memory_store(
    handle: &PersistenceHandle,
    legacy_db: &Path,
    report: &mut MigrationReport,
) -> anyhow::Result<()> {
    let snapshot = read_memory_snapshot(legacy_db)?;
    for doc in snapshot.memories {
        let id = doc["id"].as_str().unwrap_or_default().to_string();
        handle.upsert("memory_entry", &id, &doc)?;
        report.imported_memories += 1;
    }
    for doc in snapshot.turns {
        let id = doc["id"].as_str().unwrap_or_default().to_string();
        handle.upsert("conversation_turn", &id, &doc)?;
        report.imported_turns += 1;
    }
    Ok(())
}

fn import_legacy_jobs(
    handle: &PersistenceHandle,
    jobs_db: &Path,
    report: &mut MigrationReport,
) -> anyhow::Result<()> {
    for doc in read_job_records(jobs_db)? {
        let id = doc["id"].as_str().unwrap_or_default().to_string();
        handle.upsert("job_record", &id, &doc)?;
        report.imported_jobs += 1;
    }
    Ok(())
}

fn import_calendar(
    handle: &PersistenceHandle,
    db: &Path,
    report: &mut MigrationReport,
) -> anyhow::Result<()> {
    for doc in read_calendar_events(db)? {
        let id = doc["id"].as_str().unwrap_or_default().to_string();
        handle.upsert("calendar_event", &id, &doc)?;
        report.imported_calendar_events += 1;
    }
    Ok(())
}

fn import_knowledge_base(
    handle: &PersistenceHandle,
    db: &Path,
    report: &mut MigrationReport,
) -> anyhow::Result<()> {
    for doc in read_knowledge_base_entries(db)? {
        let id = doc["id"].as_str().unwrap_or_default().to_string();
        handle.upsert("kb_entry", &id, &doc)?;
        report.imported_kb_entries += 1;
    }
    Ok(())
}

fn read_memory_snapshot(path: &Path) -> anyhow::Result<LegacyMemorySnapshot> {
    run_sqlite(path, |options| async move {
        let mut conn = SqliteConnection::connect_with(&options).await?;
        let mut snapshot = LegacyMemorySnapshot::default();

        if table_exists_async(&mut conn, "memories").await? {
            let rows = sqlx::query(
                "SELECT id, content, weight, created_at FROM memories ORDER BY created_at ASC",
            )
            .fetch_all(&mut conn)
            .await?;
            for row in rows {
                snapshot.memories.push(json!({
                    "id": row.try_get::<String, _>("id")?,
                    "content": row.try_get::<String, _>("content")?,
                    "weight": row.try_get::<String, _>("weight")?,
                    "created_at": row.try_get::<String, _>("created_at")?,
                }));
            }
        }

        if table_exists_async(&mut conn, "conversation_turns").await? {
            let rows = sqlx::query("SELECT id, session_id, turn_number, role, content, provider, model, tier, complexity_score, token_estimate, created_at FROM conversation_turns ORDER BY created_at ASC")
                .fetch_all(&mut conn)
                .await?;
            for row in rows {
                snapshot.turns.push(json!({
                    "id": row.try_get::<String, _>("id")?,
                    "session_id": row.try_get::<String, _>("session_id")?,
                    "turn_number": row.try_get::<i64, _>("turn_number")? as u32,
                    "role": row.try_get::<String, _>("role")?,
                    "content": row.try_get::<String, _>("content")?,
                    "provider": row.try_get::<Option<String>, _>("provider")?,
                    "model": row.try_get::<Option<String>, _>("model")?,
                    "tier": row.try_get::<Option<String>, _>("tier")?,
                    "complexity_score": row.try_get::<Option<i64>, _>("complexity_score")?.map(|value| value as u8),
                    "token_estimate": row.try_get::<Option<i64>, _>("token_estimate")?.map(|value| value as u32),
                    "created_at": row.try_get::<String, _>("created_at")?,
                }));
            }
        }

        Ok(snapshot)
    })
}

fn read_job_records(path: &Path) -> anyhow::Result<Vec<serde_json::Value>> {
    run_sqlite(path, |options| async move {
        let mut conn = SqliteConnection::connect_with(&options).await?;
        if !table_exists_async(&mut conn, "job_queue").await? {
            return Ok(Vec::new());
        }

        let rows = sqlx::query("SELECT * FROM job_queue ORDER BY created_at ASC")
            .fetch_all(&mut conn)
            .await?;
        let mut docs = Vec::with_capacity(rows.len());
        for row in rows {
            docs.push(json!({
                "id": row.try_get::<String, _>("id")?,
                "topic": row.try_get::<String, _>("topic")?,
                "goal": row.try_get::<String, _>("goal")?,
                "capability": row.try_get::<String, _>("capability")?,
                "priority": row.try_get::<i64, _>("priority")? as i32,
                "status": row.try_get::<String, _>("status")?,
                "time_budget_ms": row.try_get::<i64, _>("time_budget_ms")? as u64,
                "max_turns": row.try_get::<i64, _>("max_turns")? as u32,
                "system_context": row.try_get::<Option<String>, _>("system_context")?,
                "allowed_skill_ids": row.try_get::<Option<String>, _>("allowed_skill_ids")?,
                "input_data": row.try_get::<Option<String>, _>("input_data")?,
                "parent_job_id": row.try_get::<Option<String>, _>("parent_job_id")?,
                "agent_id": row.try_get::<Option<String>, _>("agent_id")?,
                "model_used": row.try_get::<Option<String>, _>("model_used")?,
                "provider_used": row.try_get::<Option<String>, _>("provider_used")?,
                "result": row.try_get::<Option<String>, _>("result")?,
                "error": row.try_get::<Option<String>, _>("error")?,
                "turns_consumed": row.try_get::<i64, _>("turns_consumed")? as u32,
                "ttl_seconds": row.try_get::<i64, _>("ttl_seconds")? as u64,
                "created_at": row.try_get::<String, _>("created_at")?,
                "started_at": row.try_get::<Option<String>, _>("started_at")?,
                "completed_at": row.try_get::<Option<String>, _>("completed_at")?,
                "expires_at": row.try_get::<String, _>("expires_at")?,
                "cron_expression": row.try_get::<Option<String>, _>("cron_expression").ok().flatten(),
                "is_recurring": row.try_get::<Option<i64>, _>("is_recurring").ok().flatten().map(|value| value != 0).unwrap_or(false),
                "significance_keywords": row.try_get::<Option<String>, _>("significance_keywords").ok().flatten(),
                "significance_threshold": row.try_get::<Option<f64>, _>("significance_threshold").ok().flatten().unwrap_or(0.5),
                "job_mode": row.try_get::<Option<String>, _>("job_mode").ok().flatten().unwrap_or_else(|| "agentic_run".to_string()),
                "goal_template": row.try_get::<Option<String>, _>("goal_template").ok().flatten(),
                "last_scheduled_at": row.try_get::<Option<String>, _>("last_scheduled_at").ok().flatten(),
                "depends_on": row.try_get::<Option<String>, _>("depends_on").ok().flatten(),
                "execution_mode": row.try_get::<Option<String>, _>("execution_mode").ok().flatten().unwrap_or_else(|| "mediated".to_string()),
                "direct_tool_call": row.try_get::<Option<String>, _>("direct_tool_call").ok().flatten(),
            }));
        }
        Ok(docs)
    })
}

fn read_calendar_events(path: &Path) -> anyhow::Result<Vec<serde_json::Value>> {
    run_sqlite(path, |options| async move {
        let mut conn = SqliteConnection::connect_with(&options).await?;
        if !table_exists_async(&mut conn, "events").await? {
            return Ok(Vec::new());
        }

        let rows = sqlx::query("SELECT id, title, description, start_time, end_time, location, created_at FROM events ORDER BY start_time ASC")
            .fetch_all(&mut conn)
            .await?;
        let mut docs = Vec::with_capacity(rows.len());
        for row in rows {
            docs.push(json!({
                "id": row.try_get::<String, _>("id")?,
                "title": row.try_get::<String, _>("title")?,
                "description": row.try_get::<Option<String>, _>("description")?,
                "start_time": row.try_get::<String, _>("start_time")?,
                "end_time": row.try_get::<Option<String>, _>("end_time")?,
                "location": row.try_get::<Option<String>, _>("location")?,
                "created_at": row.try_get::<String, _>("created_at")?,
            }));
        }
        Ok(docs)
    })
}

fn read_knowledge_base_entries(path: &Path) -> anyhow::Result<Vec<serde_json::Value>> {
    run_sqlite(path, |options| async move {
        let mut conn = SqliteConnection::connect_with(&options).await?;
        if !table_exists_async(&mut conn, "entries").await? {
            return Ok(Vec::new());
        }

        let rows = sqlx::query(
            "SELECT id, title, content, tags, category, created_at, updated_at FROM entries ORDER BY updated_at DESC",
        )
        .fetch_all(&mut conn)
        .await?;
        let mut docs = Vec::with_capacity(rows.len());
        for row in rows {
            docs.push(json!({
                "id": row.try_get::<String, _>("id")?,
                "title": row.try_get::<String, _>("title")?,
                "content": row.try_get::<String, _>("content")?,
                "tags": row.try_get::<Option<String>, _>("tags")?,
                "category": row.try_get::<Option<String>, _>("category")?,
                "created_at": row.try_get::<String, _>("created_at")?,
                "updated_at": row.try_get::<String, _>("updated_at")?,
            }));
        }
        Ok(docs)
    })
}

async fn table_exists_async(conn: &mut SqliteConnection, table_name: &str) -> anyhow::Result<bool> {
    let exists: i64 =
        sqlx::query_scalar("SELECT COUNT(*) FROM sqlite_master WHERE type='table' AND name = ?")
            .bind(table_name)
            .fetch_one(conn)
            .await?;
    Ok(exists > 0)
}

fn run_sqlite<T, F, Fut>(path: &Path, f: F) -> anyhow::Result<T>
where
    T: Send + 'static,
    F: FnOnce(SqliteConnectOptions) -> Fut + Send + 'static,
    Fut: std::future::Future<Output = anyhow::Result<T>> + Send + 'static,
{
    let path = path.to_path_buf();
    std::thread::spawn(move || {
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()?;
        runtime.block_on(async move {
            let options = SqliteConnectOptions::new()
                .filename(path)
                .read_only(true)
                .disable_statement_logging();
            f(options).await
        })
    })
    .join()
    .map_err(|_| anyhow::anyhow!("SQLite migration thread panicked"))?
}

fn archive_legacy_source(path: &Path) -> anyhow::Result<()> {
    if !path.exists() {
        return Ok(());
    }

    let archived = path.with_extension(format!(
        "{}.legacy-imported",
        path.extension()
            .and_then(|ext| ext.to_str())
            .unwrap_or_default()
    ));
    std::fs::rename(path, archived)?;
    Ok(())
}

#[derive(Default)]
struct LegacyMemorySnapshot {
    memories: Vec<serde_json::Value>,
    turns: Vec<serde_json::Value>,
}
