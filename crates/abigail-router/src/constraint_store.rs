//! Persistent constraint store -- tracks learned constraints from failures.
//!
//! Constraints are lessons learned during execution: facts about the environment
//! or task that narrow what approaches are valid. They are persisted to a JSON
//! file so they survive across sessions.

use chrono::Utc;
use serde::{Deserialize, Serialize};
use std::path::PathBuf;
use uuid::Uuid;

/// A single constraint discovered during execution.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct Constraint {
    /// Unique identifier (UUID v4).
    pub id: String,
    /// Human-readable description of the constraint.
    pub description: String,
    /// What failure taught us this constraint.
    pub learned_from: String,
    /// ISO 8601 timestamp of when this constraint was created.
    pub created_at: String,
}

/// Persists learned constraints to a JSON file.
///
/// Constraints accumulate over the lifetime of the application and feed into
/// planning decisions so the agent avoids repeating known-bad strategies.
pub struct ConstraintStore {
    constraints: Vec<Constraint>,
    file_path: PathBuf,
}

impl ConstraintStore {
    /// Create a new store rooted in `data_dir`.
    ///
    /// If a constraints file already exists on disk, it is loaded automatically.
    /// If the file does not exist or is corrupt, the store starts empty.
    pub fn new(data_dir: PathBuf) -> Self {
        match Self::load(data_dir.clone()) {
            Ok(store) => store,
            Err(_) => Self {
                constraints: Vec::new(),
                file_path: data_dir.join("constraints.json"),
            },
        }
    }

    /// Load an existing store from disk.
    ///
    /// Returns an error if the file does not exist or cannot be parsed.
    pub fn load(data_dir: PathBuf) -> anyhow::Result<Self> {
        let file_path = data_dir.join("constraints.json");
        let content = std::fs::read_to_string(&file_path)?;
        let constraints: Vec<Constraint> = serde_json::from_str(&content)?;
        Ok(Self {
            constraints,
            file_path,
        })
    }

    /// Add a new constraint learned from a specific failure.
    ///
    /// Returns the UUID of the newly created constraint.
    pub fn add(&mut self, description: &str, learned_from: &str) -> String {
        let id = Uuid::new_v4().to_string();
        self.constraints.push(Constraint {
            id: id.clone(),
            description: description.to_string(),
            learned_from: learned_from.to_string(),
            created_at: Utc::now().to_rfc3339(),
        });
        tracing::debug!(
            "ConstraintStore: added constraint '{}' (learned from: {})",
            description,
            learned_from
        );
        id
    }

    /// Remove a constraint by its ID.
    ///
    /// Returns `true` if the constraint was found and removed, `false` otherwise.
    pub fn remove(&mut self, id: &str) -> bool {
        let before = self.constraints.len();
        self.constraints.retain(|c| c.id != id);
        let removed = self.constraints.len() < before;
        if removed {
            tracing::debug!("ConstraintStore: removed constraint {}", id);
        }
        removed
    }

    /// Return a slice of all constraints.
    pub fn all(&self) -> &[Constraint] {
        &self.constraints
    }

    /// Remove all constraints.
    pub fn clear(&mut self) {
        self.constraints.clear();
        tracing::debug!("ConstraintStore: cleared all constraints");
    }

    /// Persist the current constraints to disk as JSON.
    pub fn save(&self) -> anyhow::Result<()> {
        // Ensure the parent directory exists.
        if let Some(parent) = self.file_path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        let json = serde_json::to_string_pretty(&self.constraints)?;
        std::fs::write(&self.file_path, json)?;
        tracing::debug!(
            "ConstraintStore: saved {} constraints to {:?}",
            self.constraints.len(),
            self.file_path
        );
        Ok(())
    }

    /// Return just the description strings, suitable for feeding into a planner prompt.
    pub fn as_strings(&self) -> Vec<String> {
        self.constraints
            .iter()
            .map(|c| c.description.clone())
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    /// Helper: create a temporary directory for testing.
    fn test_dir(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("abigail_constraint_store_test_{}", name));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        dir
    }

    /// Helper: clean up a test directory.
    fn cleanup(dir: &PathBuf) {
        let _ = fs::remove_dir_all(dir);
    }

    #[test]
    fn test_new_empty_store() {
        let dir = test_dir("new_empty");
        let store = ConstraintStore::new(dir.clone());
        assert!(store.all().is_empty());
        assert!(store.as_strings().is_empty());
        cleanup(&dir);
    }

    #[test]
    fn test_add_constraint() {
        let dir = test_dir("add");
        let mut store = ConstraintStore::new(dir.clone());

        let id = store.add("API requires auth token", "HTTP 401 on attempt #1");
        assert!(!id.is_empty());
        assert_eq!(store.all().len(), 1);

        let constraint = &store.all()[0];
        assert_eq!(constraint.id, id);
        assert_eq!(constraint.description, "API requires auth token");
        assert_eq!(constraint.learned_from, "HTTP 401 on attempt #1");
        assert!(!constraint.created_at.is_empty());

        cleanup(&dir);
    }

    #[test]
    fn test_add_multiple_constraints() {
        let dir = test_dir("add_multiple");
        let mut store = ConstraintStore::new(dir.clone());

        let id1 = store.add("Rate limit is 100/min", "HTTP 429");
        let id2 = store.add("Response must be JSON", "Parse error on attempt #2");

        assert_ne!(id1, id2);
        assert_eq!(store.all().len(), 2);

        cleanup(&dir);
    }

    #[test]
    fn test_remove_constraint() {
        let dir = test_dir("remove");
        let mut store = ConstraintStore::new(dir.clone());

        let id = store.add("Temporary constraint", "test");
        assert_eq!(store.all().len(), 1);

        let removed = store.remove(&id);
        assert!(removed);
        assert!(store.all().is_empty());

        cleanup(&dir);
    }

    #[test]
    fn test_remove_nonexistent_constraint() {
        let dir = test_dir("remove_nonexistent");
        let mut store = ConstraintStore::new(dir.clone());

        store.add("Keep me", "test");
        let removed = store.remove("nonexistent-id");
        assert!(!removed);
        assert_eq!(store.all().len(), 1);

        cleanup(&dir);
    }

    #[test]
    fn test_clear() {
        let dir = test_dir("clear");
        let mut store = ConstraintStore::new(dir.clone());

        store.add("Constraint A", "failure A");
        store.add("Constraint B", "failure B");
        assert_eq!(store.all().len(), 2);

        store.clear();
        assert!(store.all().is_empty());

        cleanup(&dir);
    }

    #[test]
    fn test_save_and_load() {
        let dir = test_dir("save_load");
        let mut store = ConstraintStore::new(dir.clone());

        store.add("Persisted constraint", "saved from test");
        store.add("Another constraint", "also saved");
        store.save().unwrap();

        // Load into a new store instance.
        let loaded = ConstraintStore::load(dir.clone()).unwrap();
        assert_eq!(loaded.all().len(), 2);
        assert_eq!(loaded.all()[0].description, "Persisted constraint");
        assert_eq!(loaded.all()[1].description, "Another constraint");

        cleanup(&dir);
    }

    #[test]
    fn test_new_loads_existing_file() {
        let dir = test_dir("new_loads");
        {
            let mut store = ConstraintStore::new(dir.clone());
            store.add("Pre-existing constraint", "from previous session");
            store.save().unwrap();
        }

        // ConstraintStore::new should auto-load the file.
        let store = ConstraintStore::new(dir.clone());
        assert_eq!(store.all().len(), 1);
        assert_eq!(store.all()[0].description, "Pre-existing constraint");

        cleanup(&dir);
    }

    #[test]
    fn test_load_nonexistent_file() {
        let dir = test_dir("load_nonexistent");
        let result = ConstraintStore::load(dir.clone());
        assert!(result.is_err());
        cleanup(&dir);
    }

    #[test]
    fn test_load_corrupt_file() {
        let dir = test_dir("load_corrupt");
        fs::create_dir_all(&dir).unwrap();
        fs::write(dir.join("constraints.json"), "not valid json {{{").unwrap();

        let result = ConstraintStore::load(dir.clone());
        assert!(result.is_err());

        // new() should fall back to empty store.
        let store = ConstraintStore::new(dir.clone());
        assert!(store.all().is_empty());

        cleanup(&dir);
    }

    #[test]
    fn test_as_strings() {
        let dir = test_dir("as_strings");
        let mut store = ConstraintStore::new(dir.clone());

        store.add("First constraint", "failure 1");
        store.add("Second constraint", "failure 2");
        store.add("Third constraint", "failure 3");

        let strings = store.as_strings();
        assert_eq!(strings.len(), 3);
        assert_eq!(strings[0], "First constraint");
        assert_eq!(strings[1], "Second constraint");
        assert_eq!(strings[2], "Third constraint");

        cleanup(&dir);
    }

    #[test]
    fn test_as_strings_empty() {
        let dir = test_dir("as_strings_empty");
        let store = ConstraintStore::new(dir.clone());
        assert!(store.as_strings().is_empty());
        cleanup(&dir);
    }

    #[test]
    fn test_constraint_id_is_uuid() {
        let dir = test_dir("uuid");
        let mut store = ConstraintStore::new(dir.clone());

        let id = store.add("Test", "test");
        // UUIDs are 36 chars with dashes (8-4-4-4-12).
        assert_eq!(id.len(), 36);
        assert_eq!(id.chars().filter(|c| *c == '-').count(), 4);

        cleanup(&dir);
    }

    #[test]
    fn test_constraint_created_at_is_iso8601() {
        let dir = test_dir("iso8601");
        let mut store = ConstraintStore::new(dir.clone());

        store.add("Test", "test");
        let created_at = &store.all()[0].created_at;
        // Should be parseable as an RFC 3339 / ISO 8601 datetime.
        let parsed = chrono::DateTime::parse_from_rfc3339(created_at);
        assert!(
            parsed.is_ok(),
            "created_at should be valid RFC 3339: {}",
            created_at
        );

        cleanup(&dir);
    }

    #[test]
    fn test_save_creates_parent_directories() {
        let dir = test_dir("nested_parent");
        let nested = dir.join("deep").join("nested").join("dir");
        let mut store = ConstraintStore::new(nested.clone());

        store.add("Nested test", "testing save");
        store.save().unwrap();

        // Verify the file was created.
        assert!(nested.join("constraints.json").exists());

        cleanup(&dir);
    }

    #[test]
    fn test_round_trip_preserves_data() {
        let dir = test_dir("round_trip");
        let mut store = ConstraintStore::new(dir.clone());

        let id = store.add("Exact description", "Exact failure source");
        store.save().unwrap();

        let loaded = ConstraintStore::load(dir.clone()).unwrap();
        let c = &loaded.all()[0];
        assert_eq!(c.id, id);
        assert_eq!(c.description, "Exact description");
        assert_eq!(c.learned_from, "Exact failure source");

        cleanup(&dir);
    }

    #[test]
    fn test_save_after_remove() {
        let dir = test_dir("save_after_remove");
        let mut store = ConstraintStore::new(dir.clone());

        let id1 = store.add("Keep this", "test");
        let id2 = store.add("Remove this", "test");
        store.remove(&id2);
        store.save().unwrap();

        let loaded = ConstraintStore::load(dir.clone()).unwrap();
        assert_eq!(loaded.all().len(), 1);
        assert_eq!(loaded.all()[0].id, id1);

        cleanup(&dir);
    }

    #[test]
    fn test_save_after_clear() {
        let dir = test_dir("save_after_clear");
        let mut store = ConstraintStore::new(dir.clone());

        store.add("Will be cleared", "test");
        store.clear();
        store.save().unwrap();

        let loaded = ConstraintStore::load(dir.clone()).unwrap();
        assert!(loaded.all().is_empty());

        cleanup(&dir);
    }
}
