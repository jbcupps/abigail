use std::sync::Arc;

use abigail_core::{PassphraseUnlockProvider, UnlockProvider};
use abigail_memory::{ConversationTurn, MemoryStore};
use uuid::Uuid;

#[test]
fn secret_moves_without_mentor_and_survives_restart() {
    let entity_id = Uuid::new_v4().to_string();
    let root =
        std::env::temp_dir().join(format!("abigail_secret_topic_auto_move_{}", Uuid::new_v4()));
    let entity_dir = root.join(&entity_id);
    std::fs::create_dir_all(&entity_dir).unwrap();
    let db_path = entity_dir.join("abigail_memory.db");
    let unlock: Arc<dyn UnlockProvider> =
        Arc::new(PassphraseUnlockProvider::new("secret-topic-auto-move"));

    let store = MemoryStore::open_with_unlock(&db_path, unlock.clone()).unwrap();
    store
        .insert_turn(&ConversationTurn::new(
            "session-secret-1",
            "user",
            "Here is my IMAP password: mentor-email-app-password",
        ))
        .unwrap();

    let turns = store.recent_turns("session-secret-1", 10).unwrap();
    assert_eq!(turns.len(), 1);
    assert!(turns[0].content.contains("Secrets Vault"));
    assert!(!turns[0].content.contains("mentor-email-app-password"));

    let topics = store.list_protected_topics(10).unwrap();
    assert_eq!(topics.len(), 1);
    assert_eq!(topics[0].topic_name, format!("secrets-{}", entity_id));
    assert_eq!(topics[0].entry_count, 1);
    assert!(topics[0].last_preview.total > 0);

    let entries = store
        .protected_topic_entries(&topics[0].topic_name, 10)
        .unwrap();
    assert_eq!(entries.len(), 1);
    assert_eq!(entries[0].session_id, "session-secret-1");
    assert!(entries[0]
        .content
        .contains("Here is my IMAP password: mentor-email-app-password"));

    drop(store);

    let reopened = MemoryStore::open_with_unlock(&db_path, unlock).unwrap();
    let reopened_topics = reopened.list_protected_topics(10).unwrap();
    assert_eq!(reopened_topics.len(), 1);
    assert_eq!(
        reopened_topics[0].topic_name,
        format!("secrets-{}", entity_id)
    );

    let reopened_entries = reopened
        .protected_topic_entries(&reopened_topics[0].topic_name, 10)
        .unwrap();
    assert_eq!(reopened_entries.len(), 1);
    assert!(reopened_entries[0]
        .content
        .contains("mentor-email-app-password"));

    let _ = std::fs::remove_dir_all(root);
}
