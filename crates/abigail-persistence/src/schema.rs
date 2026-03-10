use crate::client::{EntityScope, PersistenceError};
use surrealdb::engine::local::Db;
use surrealdb::Surreal;

const HIVE_SCHEMA: &str = r#"
DEFINE TABLE IF NOT EXISTS hive_meta SCHEMALESS;
DEFINE TABLE IF NOT EXISTS migration_log SCHEMALESS;
DEFINE TABLE IF NOT EXISTS job_record SCHEMALESS;
DEFINE TABLE IF NOT EXISTS reflection_run SCHEMALESS;
DEFINE TABLE IF NOT EXISTS reflection_projection SCHEMALESS;
DEFINE INDEX IF NOT EXISTS idx_job_record_status ON TABLE job_record COLUMNS status;
DEFINE INDEX IF NOT EXISTS idx_job_record_topic ON TABLE job_record COLUMNS topic;
DEFINE INDEX IF NOT EXISTS idx_reflection_run_entity_day ON TABLE reflection_run COLUMNS entity_id, run_day;
"#;

const ENTITY_SCHEMA: &str = r#"
DEFINE TABLE IF NOT EXISTS birth SCHEMALESS;
DEFINE TABLE IF NOT EXISTS memory_entry SCHEMALESS;
DEFINE TABLE IF NOT EXISTS conversation_turn SCHEMALESS;
DEFINE TABLE IF NOT EXISTS protected_topic SCHEMALESS;
DEFINE TABLE IF NOT EXISTS protected_topic_entry SCHEMALESS;
DEFINE TABLE IF NOT EXISTS memory_edge SCHEMALESS;
DEFINE TABLE IF NOT EXISTS calendar_event SCHEMALESS;
DEFINE TABLE IF NOT EXISTS kb_entry SCHEMALESS;
DEFINE TABLE IF NOT EXISTS embedding_chunk SCHEMALESS;
DEFINE INDEX IF NOT EXISTS idx_memory_entry_created_at ON TABLE memory_entry COLUMNS created_at;
DEFINE INDEX IF NOT EXISTS idx_conversation_turn_session ON TABLE conversation_turn COLUMNS session_id, turn_number;
DEFINE INDEX IF NOT EXISTS idx_conversation_turn_created_at ON TABLE conversation_turn COLUMNS created_at;
DEFINE INDEX IF NOT EXISTS idx_protected_topic_entry_topic ON TABLE protected_topic_entry COLUMNS topic_name, created_at;
DEFINE INDEX IF NOT EXISTS idx_calendar_event_start_time ON TABLE calendar_event COLUMNS start_time;
DEFINE INDEX IF NOT EXISTS idx_kb_entry_updated_at ON TABLE kb_entry COLUMNS updated_at;
"#;

pub async fn ensure_schema(db: &Surreal<Db>, scope: &EntityScope) -> Result<(), PersistenceError> {
    let sql = match scope {
        EntityScope::Hive => HIVE_SCHEMA,
        EntityScope::Entity(_) => ENTITY_SCHEMA,
    };
    db.query(sql).await?.check()?;
    Ok(())
}
