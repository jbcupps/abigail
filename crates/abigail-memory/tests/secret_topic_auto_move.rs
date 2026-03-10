use std::sync::Arc;

use abigail_core::{PassphraseUnlockProvider, UnlockProvider};
use abigail_memory::{ConversationTurn, MemoryStore};
use uuid::Uuid;

/// Validates that secrets are detected, redacted from the conversation turn,
/// and stored in protected topics — all using an in-memory store so there is
/// no file-lock contention.
#[test]
fn secret_detection_and_protected_topic_storage() {
    let entity_id = Uuid::new_v4().to_string();
    let unlock: Arc<dyn UnlockProvider> =
        Arc::new(PassphraseUnlockProvider::new("secret-topic-auto-move"));

    let store = MemoryStore::open_in_memory_with_entity_and_unlock(&entity_id, unlock).unwrap();
    store
        .insert_turn(&ConversationTurn::new(
            "session-secret-1",
            "user",
            "Here is my IMAP password: mentor-email-app-password",
        ))
        .unwrap();

    // The stored turn should have the secret redacted.
    let turns = store.recent_turns("session-secret-1", 10).unwrap();
    assert_eq!(turns.len(), 1);
    assert!(turns[0].content.contains("Secrets Vault"));
    assert!(!turns[0].content.contains("mentor-email-app-password"));

    // A protected topic should exist scoped to this entity.
    let topics = store.list_protected_topics(10).unwrap();
    assert_eq!(topics.len(), 1);
    assert_eq!(topics[0].topic_name, format!("secrets-{}", entity_id));
    assert_eq!(topics[0].entry_count, 1);
    assert!(topics[0].last_preview.total > 0);

    // The protected topic entry should hold the original content.
    let entries = store
        .protected_topic_entries(&topics[0].topic_name, 10)
        .unwrap();
    assert_eq!(entries.len(), 1);
    assert_eq!(entries[0].session_id, "session-secret-1");
    assert!(entries[0]
        .content
        .contains("Here is my IMAP password: mentor-email-app-password"));
}

/// Validates that secrets survive a store restart — requires file-backed
/// storage and the close-then-reopen pattern.
///
/// Ignored on all platforms: SurrealKV releases its file lock asynchronously
/// on `Drop`, so immediately reopening the same path races with the async
/// lock release.  There is no public `close()` API to block on cleanup.
#[test]
#[ignore = "SurrealKV releases file locks asynchronously on drop; close-and-reopen races in CI"]
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
