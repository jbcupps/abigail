//! Birth conversation persistence.
//!
//! Saves and loads the birth conversation to/from a JSON file in the data
//! directory so that conversations survive app crashes and restarts.

use std::path::Path;

const CONVERSATION_FILE: &str = "birth_conversation.json";

#[derive(serde::Serialize, serde::Deserialize)]
struct PersistedConversation {
    stage: String,
    messages: Vec<(String, String)>,
}

/// Persist the current conversation to disk.
pub fn save_conversation(
    data_dir: &Path,
    stage: &str,
    conversation: &[(String, String)],
) -> anyhow::Result<()> {
    let path = data_dir.join(CONVERSATION_FILE);
    let payload = PersistedConversation {
        stage: stage.to_string(),
        messages: conversation.to_vec(),
    };
    let json = serde_json::to_string_pretty(&payload)?;
    std::fs::write(path, json)?;
    Ok(())
}

/// Loaded conversation: (stage_name, messages).
pub type LoadedConversation = (String, Vec<(String, String)>);

/// Load a previously persisted conversation.
///
/// Returns `Some((stage_name, messages))` if a conversation file exists,
/// `None` if no file is found.
pub fn load_conversation(data_dir: &Path) -> anyhow::Result<Option<LoadedConversation>> {
    let path = data_dir.join(CONVERSATION_FILE);
    if !path.exists() {
        return Ok(None);
    }
    let json = std::fs::read_to_string(&path)?;
    let persisted: PersistedConversation = serde_json::from_str(&json)?;
    Ok(Some((persisted.stage, persisted.messages)))
}

/// Remove the persisted conversation file (called on birth completion).
pub fn clear_conversation(data_dir: &Path) -> anyhow::Result<()> {
    let path = data_dir.join(CONVERSATION_FILE);
    if path.exists() {
        std::fs::remove_file(path)?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    #[test]
    fn test_save_and_load_conversation() {
        let tmp = std::env::temp_dir().join("abigail_persist_test_save_load");
        let _ = fs::remove_dir_all(&tmp);
        fs::create_dir_all(&tmp).unwrap();

        let messages = vec![
            ("user".to_string(), "hello".to_string()),
            ("assistant".to_string(), "hi there".to_string()),
        ];
        save_conversation(&tmp, "Connectivity", &messages).unwrap();

        let loaded = load_conversation(&tmp).unwrap().unwrap();
        assert_eq!(loaded.0, "Connectivity");
        assert_eq!(loaded.1.len(), 2);
        assert_eq!(loaded.1[0].0, "user");
        assert_eq!(loaded.1[0].1, "hello");

        let _ = fs::remove_dir_all(&tmp);
    }

    #[test]
    fn test_load_nonexistent() {
        let tmp = std::env::temp_dir().join("abigail_persist_test_nonexist");
        let _ = fs::remove_dir_all(&tmp);
        fs::create_dir_all(&tmp).unwrap();

        let result = load_conversation(&tmp).unwrap();
        assert!(result.is_none());

        let _ = fs::remove_dir_all(&tmp);
    }

    #[test]
    fn test_clear_conversation() {
        let tmp = std::env::temp_dir().join("abigail_persist_test_clear");
        let _ = fs::remove_dir_all(&tmp);
        fs::create_dir_all(&tmp).unwrap();

        save_conversation(&tmp, "Crystallization", &[]).unwrap();
        assert!(tmp.join("birth_conversation.json").exists());

        clear_conversation(&tmp).unwrap();
        assert!(!tmp.join("birth_conversation.json").exists());

        let _ = fs::remove_dir_all(&tmp);
    }

    #[test]
    fn test_clear_nonexistent_is_ok() {
        let tmp = std::env::temp_dir().join("abigail_persist_test_clear_none");
        let _ = fs::remove_dir_all(&tmp);
        fs::create_dir_all(&tmp).unwrap();

        clear_conversation(&tmp).unwrap();

        let _ = fs::remove_dir_all(&tmp);
    }
}
