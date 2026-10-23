use crate::client::{EntityScope, PersistenceError};
use surrealdb::engine::local::Db;
use surrealdb::Surreal;

const HIVE_SCHEMA: &str = r#"
DEFINE TABLE IF NOT EXISTS hive_meta SCHEMALESS;
DEFINE TABLE IF NOT EXISTS genesis_session SCHEMALESS;
DEFINE INDEX IF NOT EXISTS idx_genesis_session_owner ON TABLE genesis_session COLUMNS owner_id;
DEFINE TABLE IF NOT EXISTS entity_journey SCHEMALESS;
DEFINE INDEX IF NOT EXISTS idx_entity_journey_owner ON TABLE entity_journey COLUMNS owner_id;
DEFINE TABLE OVERWRITE genesis_legacy_migration SCHEMAFULL;
DEFINE FIELD OVERWRITE owner_id ON TABLE genesis_legacy_migration TYPE string;
DEFINE FIELD OVERWRITE entity_id ON TABLE genesis_legacy_migration TYPE string;
DEFINE FIELD OVERWRITE request_digest ON TABLE genesis_legacy_migration TYPE string;
DEFINE FIELD OVERWRITE state ON TABLE genesis_legacy_migration TYPE string;
DEFINE FIELD OVERWRITE created_at ON TABLE genesis_legacy_migration TYPE string;
DEFINE FIELD OVERWRITE updated_at ON TABLE genesis_legacy_migration TYPE string;
UPDATE genesis_legacy_migration CONTENT {
    owner_id: owner_id,
    entity_id: entity_id,
    request_digest: request_digest,
    state: state,
    created_at: created_at,
    updated_at: updated_at
};
DEFINE INDEX IF NOT EXISTS idx_genesis_legacy_migration_owner ON TABLE genesis_legacy_migration COLUMNS owner_id;
DEFINE INDEX IF NOT EXISTS idx_genesis_legacy_migration_entity ON TABLE genesis_legacy_migration COLUMNS owner_id, entity_id;
DEFINE TABLE OVERWRITE entity_journey_event SCHEMAFULL;
DEFINE FIELD OVERWRITE record_id ON TABLE entity_journey_event TYPE string;
DEFINE FIELD OVERWRITE owner_id ON TABLE entity_journey_event TYPE string;
DEFINE FIELD OVERWRITE entity_id ON TABLE entity_journey_event TYPE string;
DEFINE FIELD OVERWRITE kind ON TABLE entity_journey_event TYPE string;
DEFINE FIELD OVERWRITE occurred_at ON TABLE entity_journey_event TYPE string;
DEFINE FIELD OVERWRITE session_id ON TABLE entity_journey_event TYPE option<string>;
DEFINE FIELD OVERWRITE skill_id ON TABLE entity_journey_event TYPE option<string>;
DEFINE FIELD OVERWRITE tool_name ON TABLE entity_journey_event TYPE option<string>;
DEFINE FIELD OVERWRITE successful_skill_count ON TABLE entity_journey_event TYPE option<int>;
DEFINE FIELD OVERWRITE memory_id ON TABLE entity_journey_event TYPE option<string>;
DEFINE FIELD OVERWRITE memory_weight ON TABLE entity_journey_event TYPE option<string>;
UPDATE entity_journey_event CONTENT {
    record_id: record_id,
    owner_id: owner_id,
    entity_id: entity_id,
    kind: kind,
    occurred_at: occurred_at,
    session_id: session_id,
    skill_id: skill_id,
    tool_name: tool_name,
    successful_skill_count: successful_skill_count,
    memory_id: memory_id,
    memory_weight: memory_weight
};
DEFINE INDEX IF NOT EXISTS idx_entity_journey_event_owner ON TABLE entity_journey_event COLUMNS owner_id;
DEFINE INDEX IF NOT EXISTS idx_entity_journey_event_entity ON TABLE entity_journey_event COLUMNS owner_id, entity_id;
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
DEFINE TABLE IF NOT EXISTS job_record SCHEMALESS;
DEFINE TABLE IF NOT EXISTS reflection_run SCHEMALESS;
DEFINE TABLE IF NOT EXISTS reflection_projection SCHEMALESS;
DEFINE INDEX IF NOT EXISTS idx_job_record_status ON TABLE job_record COLUMNS status;
DEFINE INDEX IF NOT EXISTS idx_job_record_topic ON TABLE job_record COLUMNS topic;
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::PersistenceHandle;

    fn valid_event(record_id: &str) -> serde_json::Value {
        serde_json::json!({
            "record_id": record_id,
            "owner_id": "local",
            "entity_id": uuid::Uuid::new_v4().to_string(),
            "kind": "skill_used",
            "occurred_at": "2026-10-22T12:00:00Z",
            "session_id": "session-1",
            "skill_id": "com.abigail.calendar",
            "tool_name": "list_events",
            "successful_skill_count": 3
        })
    }

    fn assert_field_was_rejected_or_removed(
        handle: &PersistenceHandle,
        record_id: &str,
        value: &serde_json::Value,
        field: &str,
    ) {
        let result = handle.upsert("entity_journey_event", record_id, value);
        if result.is_ok() {
            let stored = handle
                .select_record::<serde_json::Value>("entity_journey_event", record_id)
                .unwrap()
                .unwrap();
            assert!(
                stored.get(field).is_none(),
                "SCHEMAFULL journey row silently retained forbidden field {field}: {stored}"
            );
        }
    }

    #[test]
    fn journey_event_schema_allows_only_whitelisted_metadata() {
        let handle = PersistenceHandle::open_ephemeral(EntityScope::Hive).unwrap();
        let allowed_id = uuid::Uuid::new_v4().to_string();
        handle
            .upsert(
                "entity_journey_event",
                &allowed_id,
                &valid_event(&allowed_id),
            )
            .unwrap();

        for field in [
            "payload",
            "message",
            "content",
            "reply",
            "prompt",
            "model_output",
            "arbitrary",
        ] {
            let record_id = uuid::Uuid::new_v4().to_string();
            let mut injected = valid_event(&record_id);
            injected[field] = serde_json::json!("private value");
            assert_field_was_rejected_or_removed(&handle, &record_id, &injected, field);
        }
    }

    #[test]
    fn hive_schema_migrates_legacy_journey_rows_to_the_closed_shape() {
        let handle = PersistenceHandle::open_ephemeral(EntityScope::Hive).unwrap();
        handle
            .execute(
                "REMOVE TABLE entity_journey_event; DEFINE TABLE entity_journey_event SCHEMALESS;",
                &[],
            )
            .unwrap();
        let record_id = uuid::Uuid::new_v4().to_string();
        let mut legacy = valid_event(&record_id);
        legacy["memory_id"] = serde_json::json!("memory-1");
        legacy["memory_weight"] = serde_json::json!("distilled");
        legacy["payload"] = serde_json::json!({"message": "private prompt"});
        legacy["message"] = serde_json::json!("private message");
        legacy["content"] = serde_json::json!("private memory");
        legacy["reply"] = serde_json::json!("private reply");
        legacy["prompt"] = serde_json::json!("private prompt");
        legacy["model_output"] = serde_json::json!("private model output");
        legacy["arbitrary"] = serde_json::json!({"reply": "private model output"});
        handle
            .upsert("entity_journey_event", &record_id, &legacy)
            .unwrap();

        handle.execute(HIVE_SCHEMA, &[]).unwrap();

        let migrated = handle
            .select_record::<serde_json::Value>("entity_journey_event", &record_id)
            .unwrap()
            .unwrap();
        for allowed in [
            "record_id",
            "owner_id",
            "entity_id",
            "kind",
            "occurred_at",
            "session_id",
            "skill_id",
            "tool_name",
            "successful_skill_count",
            "memory_id",
            "memory_weight",
        ] {
            assert_eq!(
                migrated.get(allowed),
                legacy.get(allowed),
                "migration changed allowed field {allowed}: {migrated}"
            );
        }
        for forbidden in [
            "payload",
            "message",
            "content",
            "reply",
            "prompt",
            "model_output",
            "arbitrary",
        ] {
            assert!(
                migrated.get(forbidden).is_none(),
                "migration retained forbidden field {forbidden}: {migrated}"
            );
        }
        let new_id = uuid::Uuid::new_v4().to_string();
        let mut injected = valid_event(&new_id);
        injected["prompt"] = serde_json::json!("private prompt");
        assert_field_was_rejected_or_removed(&handle, &new_id, &injected, "prompt");
    }

    #[test]
    fn journey_legacy_migration_schema_keeps_only_binding_state_and_timestamps() {
        let handle = PersistenceHandle::open_ephemeral(EntityScope::Hive).unwrap();
        let record = serde_json::json!({
            "owner_id": "local",
            "entity_id": uuid::Uuid::new_v4().to_string(),
            "request_digest": "ab".repeat(32),
            "state": "pending",
            "created_at": "2026-10-23T00:00:00Z",
            "updated_at": "2026-10-23T00:00:00Z",
        });
        let id = "cd".repeat(32);
        handle
            .upsert("genesis_legacy_migration", &id, &record)
            .unwrap();
        let mut stored = handle
            .select_record::<serde_json::Value>("genesis_legacy_migration", &id)
            .unwrap()
            .unwrap();
        stored.as_object_mut().unwrap().remove("id");
        assert_eq!(stored, record);

        for field in [
            "attempt_key",
            "name",
            "purpose",
            "boundaries",
            "presence",
            "weights",
            "deontology",
            "teleology",
            "areteology",
            "welfare",
            "message",
            "reply",
            "content",
            "prompt",
            "model_output",
            "memory",
            "payload",
        ] {
            let next_id = uuid::Uuid::new_v4().simple().to_string();
            let mut injected = record.clone();
            injected[field] = serde_json::json!("private value");
            let result = handle.upsert("genesis_legacy_migration", &next_id, &injected);
            if result.is_ok() {
                let saved = handle
                    .select_record::<serde_json::Value>("genesis_legacy_migration", &next_id)
                    .unwrap()
                    .unwrap();
                assert!(
                    saved.get(field).is_none(),
                    "SCHEMAFULL legacy migration row retained forbidden field {field}: {saved}"
                );
            }
        }
    }
}
