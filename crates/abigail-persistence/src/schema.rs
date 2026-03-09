use crate::client::{EntityScope, PersistenceError};
use surrealdb::engine::local::Db;
use surrealdb::Surreal;

const HIVE_SCHEMA: &str = r#"
DEFINE TABLE hive_meta SCHEMALESS;
DEFINE TABLE migration_log SCHEMALESS;
DEFINE TABLE job_record SCHEMALESS;
DEFINE TABLE reflection_run SCHEMALESS;
DEFINE TABLE reflection_projection SCHEMALESS;
DEFINE INDEX idx_job_record_status ON TABLE job_record COLUMNS status;
DEFINE INDEX idx_job_record_topic ON TABLE job_record COLUMNS topic;
DEFINE INDEX idx_reflection_run_entity_day ON TABLE reflection_run COLUMNS entity_id, run_day;
"#;

const ENTITY_SCHEMA: &str = r#"
DEFINE TABLE birth SCHEMALESS;
DEFINE TABLE memory_entry SCHEMALESS;
DEFINE TABLE conversation_turn SCHEMALESS;
DEFINE TABLE protected_topic SCHEMALESS;
DEFINE TABLE protected_topic_entry SCHEMALESS;
DEFINE TABLE memory_edge SCHEMALESS;
DEFINE TABLE calendar_event SCHEMALESS;
DEFINE TABLE kb_entry SCHEMALESS;
DEFINE TABLE embedding_chunk SCHEMALESS;
DEFINE INDEX idx_memory_entry_created_at ON TABLE memory_entry COLUMNS created_at;
DEFINE INDEX idx_conversation_turn_session ON TABLE conversation_turn COLUMNS session_id, turn_number;
DEFINE INDEX idx_conversation_turn_created_at ON TABLE conversation_turn COLUMNS created_at;
DEFINE INDEX idx_protected_topic_entry_topic ON TABLE protected_topic_entry COLUMNS topic_name, created_at;
DEFINE INDEX idx_calendar_event_start_time ON TABLE calendar_event COLUMNS start_time;
DEFINE INDEX idx_kb_entry_updated_at ON TABLE kb_entry COLUMNS updated_at;
"#;

pub async fn ensure_schema(db: &Surreal<Db>, scope: &EntityScope) -> Result<(), PersistenceError> {
    let sql = match scope {
        EntityScope::Hive => HIVE_SCHEMA,
        EntityScope::Entity(_) => ENTITY_SCHEMA,
    };
    db.query(sql).await?.check()?;
    Ok(())
}
