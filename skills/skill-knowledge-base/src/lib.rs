//! Knowledge Base skill: store, search, retrieve, delete, and list tags for
//! structured knowledge entries persisted in a local SQLite database (`kb.db`).

use abigail_skills::{
    CapabilityDescriptor, CostEstimate, ExecutionContext, FileSystemPermission, HealthStatus,
    Permission, Skill, SkillConfig, SkillError, SkillHealth, SkillManifest, SkillResult,
    ToolDescriptor, ToolOutput, ToolParams, TriggerDescriptor,
};
use async_trait::async_trait;
use std::any::Any;
use std::collections::HashMap;
use std::path::PathBuf;

/// Knowledge Base skill backed by a per-user SQLite database.
pub struct KnowledgeBaseSkill {
    manifest: SkillManifest,
    /// Directory where `kb.db` will be created/opened.
    data_dir: PathBuf,
}

impl KnowledgeBaseSkill {
    /// Parse the embedded `skill.toml` manifest.
    pub fn default_manifest() -> SkillManifest {
        let toml_str = include_str!("../skill.toml");
        SkillManifest::parse(toml_str).expect("Failed to parse knowledge-base skill.toml")
    }

    /// Create a new Knowledge Base skill that stores its database in `data_dir`.
    pub fn new(manifest: SkillManifest, data_dir: PathBuf) -> Self {
        Self { manifest, data_dir }
    }

    // ── Database helpers ────────────────────────────────────────────────

    /// Return the path to the SQLite database file.
    fn db_path(&self) -> PathBuf {
        self.data_dir.join("kb.db")
    }

    /// Open (or create) the database and ensure the schema exists.
    fn ensure_db(&self) -> SkillResult<rusqlite::Connection> {
        let path = self.db_path();
        let conn = rusqlite::Connection::open(&path).map_err(|e| {
            SkillError::ToolFailed(format!("Cannot open knowledge base database: {}", e))
        })?;

        conn.execute_batch(
            "CREATE TABLE IF NOT EXISTS entries (
                id         TEXT PRIMARY KEY,
                title      TEXT NOT NULL,
                content    TEXT NOT NULL,
                tags       TEXT,
                category   TEXT,
                created_at TEXT NOT NULL,
                updated_at TEXT NOT NULL
            );",
        )
        .map_err(|e| SkillError::ToolFailed(format!("Cannot create entries table: {}", e)))?;

        Ok(conn)
    }

    // ── Tool implementations ────────────────────────────────────────────

    /// Store a new knowledge entry and return its generated ID.
    fn kb_store(&self, params: &ToolParams) -> SkillResult<ToolOutput> {
        let title: String = params.get("title").ok_or_else(|| {
            SkillError::ToolFailed("Missing required parameter: title".to_string())
        })?;
        let content: String = params.get("content").ok_or_else(|| {
            SkillError::ToolFailed("Missing required parameter: content".to_string())
        })?;
        let tags: Option<Vec<String>> = params.get("tags");
        let category: Option<String> = params.get("category");

        let conn = self.ensure_db()?;
        let id = uuid::Uuid::new_v4().to_string();
        let now = chrono::Utc::now().to_rfc3339();
        let tags_csv = tags.map(|t| t.join(","));

        conn.execute(
            "INSERT INTO entries (id, title, content, tags, category, created_at, updated_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
            rusqlite::params![id, title, content, tags_csv, category, now, now],
        )
        .map_err(|e| SkillError::ToolFailed(format!("Insert failed: {}", e)))?;

        tracing::info!("Stored knowledge entry '{}' with id {}", title, id);

        Ok(ToolOutput::success(serde_json::json!({
            "formatted": format!("Stored entry '{}' (id: {})", title, id),
            "id": id,
            "title": title,
        })))
    }

    /// Search entries by title/content (LIKE) with optional tag filter.
    fn kb_search(&self, params: &ToolParams) -> SkillResult<ToolOutput> {
        let query: String = params.get("query").ok_or_else(|| {
            SkillError::ToolFailed("Missing required parameter: query".to_string())
        })?;
        let tag: Option<String> = params.get("tag");

        let conn = self.ensure_db()?;
        let like_pattern = format!("%{}%", query);

        let mut results: Vec<serde_json::Value> = Vec::new();

        if let Some(ref tag_filter) = tag {
            let tag_like = format!("%{}%", tag_filter);
            let mut stmt = conn
                .prepare(
                    "SELECT id, title, content, tags, category, created_at, updated_at
                     FROM entries
                     WHERE (title LIKE ?1 OR content LIKE ?1)
                       AND tags LIKE ?2
                     ORDER BY updated_at DESC",
                )
                .map_err(|e| SkillError::ToolFailed(format!("Query prepare failed: {}", e)))?;

            let rows = stmt
                .query_map(rusqlite::params![like_pattern, tag_like], |row| {
                    Ok(serde_json::json!({
                        "id": row.get::<_, String>(0)?,
                        "title": row.get::<_, String>(1)?,
                        "content": row.get::<_, String>(2)?,
                        "tags": row.get::<_, Option<String>>(3)?,
                        "category": row.get::<_, Option<String>>(4)?,
                        "created_at": row.get::<_, String>(5)?,
                        "updated_at": row.get::<_, String>(6)?,
                    }))
                })
                .map_err(|e| SkillError::ToolFailed(format!("Query failed: {}", e)))?;

            for val in rows.flatten() {
                results.push(val);
            }
        } else {
            let mut stmt = conn
                .prepare(
                    "SELECT id, title, content, tags, category, created_at, updated_at
                     FROM entries
                     WHERE title LIKE ?1 OR content LIKE ?1
                     ORDER BY updated_at DESC",
                )
                .map_err(|e| SkillError::ToolFailed(format!("Query prepare failed: {}", e)))?;

            let rows = stmt
                .query_map(rusqlite::params![like_pattern], |row| {
                    Ok(serde_json::json!({
                        "id": row.get::<_, String>(0)?,
                        "title": row.get::<_, String>(1)?,
                        "content": row.get::<_, String>(2)?,
                        "tags": row.get::<_, Option<String>>(3)?,
                        "category": row.get::<_, Option<String>>(4)?,
                        "created_at": row.get::<_, String>(5)?,
                        "updated_at": row.get::<_, String>(6)?,
                    }))
                })
                .map_err(|e| SkillError::ToolFailed(format!("Query failed: {}", e)))?;

            for val in rows.flatten() {
                results.push(val);
            }
        }

        let count = results.len();
        let formatted = if results.is_empty() {
            format!("No entries found matching '{}'.", query)
        } else {
            results
                .iter()
                .map(|r| {
                    format!(
                        "- [{}] {} (tags: {})",
                        r["id"].as_str().unwrap_or("?"),
                        r["title"].as_str().unwrap_or("?"),
                        r["tags"].as_str().unwrap_or("none"),
                    )
                })
                .collect::<Vec<_>>()
                .join("\n")
        };

        Ok(ToolOutput::success(serde_json::json!({
            "formatted": formatted,
            "count": count,
            "entries": results,
        })))
    }

    /// Retrieve a single entry by ID.
    fn kb_get(&self, params: &ToolParams) -> SkillResult<ToolOutput> {
        let id: String = params
            .get("id")
            .ok_or_else(|| SkillError::ToolFailed("Missing required parameter: id".to_string()))?;

        let conn = self.ensure_db()?;
        let mut stmt = conn
            .prepare(
                "SELECT id, title, content, tags, category, created_at, updated_at
                 FROM entries WHERE id = ?1",
            )
            .map_err(|e| SkillError::ToolFailed(format!("Query prepare failed: {}", e)))?;

        let entry = stmt
            .query_row(rusqlite::params![id], |row| {
                Ok(serde_json::json!({
                    "id": row.get::<_, String>(0)?,
                    "title": row.get::<_, String>(1)?,
                    "content": row.get::<_, String>(2)?,
                    "tags": row.get::<_, Option<String>>(3)?,
                    "category": row.get::<_, Option<String>>(4)?,
                    "created_at": row.get::<_, String>(5)?,
                    "updated_at": row.get::<_, String>(6)?,
                }))
            })
            .map_err(|e| match e {
                rusqlite::Error::QueryReturnedNoRows => {
                    SkillError::ToolFailed(format!("Entry not found: {}", id))
                }
                other => SkillError::ToolFailed(format!("Query failed: {}", other)),
            })?;

        let formatted = format!(
            "# {}\n\n{}\n\nTags: {}\nCategory: {}\nCreated: {}\nUpdated: {}",
            entry["title"].as_str().unwrap_or("?"),
            entry["content"].as_str().unwrap_or(""),
            entry["tags"].as_str().unwrap_or("none"),
            entry["category"].as_str().unwrap_or("none"),
            entry["created_at"].as_str().unwrap_or("?"),
            entry["updated_at"].as_str().unwrap_or("?"),
        );

        Ok(ToolOutput::success(serde_json::json!({
            "formatted": formatted,
            "entry": entry,
        })))
    }

    /// Delete an entry by ID.
    fn kb_delete(&self, params: &ToolParams) -> SkillResult<ToolOutput> {
        let id: String = params
            .get("id")
            .ok_or_else(|| SkillError::ToolFailed("Missing required parameter: id".to_string()))?;

        let conn = self.ensure_db()?;
        let affected = conn
            .execute("DELETE FROM entries WHERE id = ?1", rusqlite::params![id])
            .map_err(|e| SkillError::ToolFailed(format!("Delete failed: {}", e)))?;

        if affected == 0 {
            return Ok(ToolOutput::error(format!("Entry not found: {}", id)));
        }

        tracing::info!("Deleted knowledge entry {}", id);

        Ok(ToolOutput::success(serde_json::json!({
            "formatted": format!("Deleted entry {}", id),
            "id": id,
            "deleted": true,
        })))
    }

    /// List all distinct tags across every entry.
    fn kb_list_tags(&self) -> SkillResult<ToolOutput> {
        let conn = self.ensure_db()?;
        let mut stmt = conn
            .prepare("SELECT DISTINCT tags FROM entries WHERE tags IS NOT NULL AND tags != ''")
            .map_err(|e| SkillError::ToolFailed(format!("Query prepare failed: {}", e)))?;

        let rows = stmt
            .query_map([], |row| row.get::<_, String>(0))
            .map_err(|e| SkillError::ToolFailed(format!("Query failed: {}", e)))?;

        let mut tag_set = std::collections::BTreeSet::new();
        for csv in rows.flatten() {
            for tag in csv.split(',') {
                let trimmed = tag.trim();
                if !trimmed.is_empty() {
                    tag_set.insert(trimmed.to_string());
                }
            }
        }

        let tags: Vec<String> = tag_set.into_iter().collect();
        let formatted = if tags.is_empty() {
            "No tags found.".to_string()
        } else {
            tags.join(", ")
        };

        Ok(ToolOutput::success(serde_json::json!({
            "formatted": formatted,
            "count": tags.len(),
            "tags": tags,
        })))
    }
}

#[async_trait]
impl Skill for KnowledgeBaseSkill {
    fn manifest(&self) -> &SkillManifest {
        &self.manifest
    }

    async fn initialize(&mut self, _config: SkillConfig) -> SkillResult<()> {
        Ok(())
    }

    async fn shutdown(&mut self) -> SkillResult<()> {
        Ok(())
    }

    fn health(&self) -> SkillHealth {
        let parent_ok = self.data_dir.exists();
        SkillHealth {
            status: if parent_ok {
                HealthStatus::Healthy
            } else {
                HealthStatus::Degraded
            },
            message: if !parent_ok {
                Some("Data directory does not exist".to_string())
            } else {
                None
            },
            last_check: chrono::Utc::now(),
            metrics: HashMap::new(),
        }
    }

    fn tools(&self) -> Vec<ToolDescriptor> {
        vec![
            ToolDescriptor {
                name: "kb_store".to_string(),
                description: "Store a new knowledge entry with title, content, optional tags, and optional category.".to_string(),
                parameters: serde_json::json!({
                    "type": "object",
                    "properties": {
                        "title": {
                            "type": "string",
                            "description": "Title of the knowledge entry"
                        },
                        "content": {
                            "type": "string",
                            "description": "Body content of the knowledge entry"
                        },
                        "tags": {
                            "type": "array",
                            "items": { "type": "string" },
                            "description": "Optional list of tags"
                        },
                        "category": {
                            "type": "string",
                            "description": "Optional category for the entry"
                        }
                    },
                    "required": ["title", "content"]
                }),
                returns: serde_json::json!({
                    "type": "object",
                    "properties": {
                        "formatted": { "type": "string" },
                        "id": { "type": "string" },
                        "title": { "type": "string" }
                    }
                }),
                cost_estimate: CostEstimate {
                    latency_ms: 10,
                    network_bound: false,
                    token_cost: None,
                },
                required_permissions: vec![
                    Permission::FileSystem(FileSystemPermission::Read(vec!["~".to_string()])),
                    Permission::FileSystem(FileSystemPermission::Write(vec!["~".to_string()])),
                ],
                autonomous: true,
                requires_confirmation: false,
            },
            ToolDescriptor {
                name: "kb_search".to_string(),
                description: "Search knowledge entries by title/content with optional tag filter.".to_string(),
                parameters: serde_json::json!({
                    "type": "object",
                    "properties": {
                        "query": {
                            "type": "string",
                            "description": "Search text to match in title or content"
                        },
                        "tag": {
                            "type": "string",
                            "description": "Optional tag to filter results"
                        }
                    },
                    "required": ["query"]
                }),
                returns: serde_json::json!({
                    "type": "object",
                    "properties": {
                        "formatted": { "type": "string" },
                        "count": { "type": "integer" },
                        "entries": { "type": "array" }
                    }
                }),
                cost_estimate: CostEstimate {
                    latency_ms: 10,
                    network_bound: false,
                    token_cost: None,
                },
                required_permissions: vec![
                    Permission::FileSystem(FileSystemPermission::Read(vec!["~".to_string()])),
                ],
                autonomous: true,
                requires_confirmation: false,
            },
            ToolDescriptor {
                name: "kb_get".to_string(),
                description: "Retrieve a single knowledge entry by its ID.".to_string(),
                parameters: serde_json::json!({
                    "type": "object",
                    "properties": {
                        "id": {
                            "type": "string",
                            "description": "UUID of the entry to retrieve"
                        }
                    },
                    "required": ["id"]
                }),
                returns: serde_json::json!({
                    "type": "object",
                    "properties": {
                        "formatted": { "type": "string" },
                        "entry": { "type": "object" }
                    }
                }),
                cost_estimate: CostEstimate {
                    latency_ms: 10,
                    network_bound: false,
                    token_cost: None,
                },
                required_permissions: vec![
                    Permission::FileSystem(FileSystemPermission::Read(vec!["~".to_string()])),
                ],
                autonomous: true,
                requires_confirmation: false,
            },
            ToolDescriptor {
                name: "kb_delete".to_string(),
                description: "Delete a knowledge entry by its ID.".to_string(),
                parameters: serde_json::json!({
                    "type": "object",
                    "properties": {
                        "id": {
                            "type": "string",
                            "description": "UUID of the entry to delete"
                        }
                    },
                    "required": ["id"]
                }),
                returns: serde_json::json!({
                    "type": "object",
                    "properties": {
                        "formatted": { "type": "string" },
                        "id": { "type": "string" },
                        "deleted": { "type": "boolean" }
                    }
                }),
                cost_estimate: CostEstimate {
                    latency_ms: 10,
                    network_bound: false,
                    token_cost: None,
                },
                required_permissions: vec![
                    Permission::FileSystem(FileSystemPermission::Read(vec!["~".to_string()])),
                    Permission::FileSystem(FileSystemPermission::Write(vec!["~".to_string()])),
                ],
                autonomous: false,
                requires_confirmation: true,
            },
            ToolDescriptor {
                name: "kb_list_tags".to_string(),
                description: "List all distinct tags across all knowledge entries.".to_string(),
                parameters: serde_json::json!({
                    "type": "object",
                    "properties": {},
                    "required": []
                }),
                returns: serde_json::json!({
                    "type": "object",
                    "properties": {
                        "formatted": { "type": "string" },
                        "count": { "type": "integer" },
                        "tags": { "type": "array" }
                    }
                }),
                cost_estimate: CostEstimate {
                    latency_ms: 10,
                    network_bound: false,
                    token_cost: None,
                },
                required_permissions: vec![
                    Permission::FileSystem(FileSystemPermission::Read(vec!["~".to_string()])),
                ],
                autonomous: true,
                requires_confirmation: false,
            },
        ]
    }

    async fn execute_tool(
        &self,
        tool_name: &str,
        params: ToolParams,
        _context: &ExecutionContext,
    ) -> SkillResult<ToolOutput> {
        match tool_name {
            "kb_store" => self.kb_store(&params),
            "kb_search" => self.kb_search(&params),
            "kb_get" => self.kb_get(&params),
            "kb_delete" => self.kb_delete(&params),
            "kb_list_tags" => self.kb_list_tags(),
            other => Err(SkillError::ToolFailed(format!("Unknown tool: {}", other))),
        }
    }

    fn capabilities(&self) -> Vec<CapabilityDescriptor> {
        vec![]
    }

    fn get_capability(&self, _cap_type: &str) -> Option<&dyn Any> {
        None
    }

    fn triggers(&self) -> Vec<TriggerDescriptor> {
        vec![]
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    fn test_skill(data_dir: PathBuf) -> KnowledgeBaseSkill {
        KnowledgeBaseSkill::new(KnowledgeBaseSkill::default_manifest(), data_dir)
    }

    #[test]
    fn test_manifest_parses() {
        let manifest = KnowledgeBaseSkill::default_manifest();
        assert_eq!(manifest.name, "Knowledge Base");
    }

    #[test]
    fn test_tools_list() {
        let tmp = std::env::temp_dir().join("abigail_kb_test_tools");
        let skill = test_skill(tmp);
        let tools = skill.tools();
        assert_eq!(tools.len(), 5);
        let names: Vec<&str> = tools.iter().map(|t| t.name.as_str()).collect();
        assert!(names.contains(&"kb_store"));
        assert!(names.contains(&"kb_search"));
        assert!(names.contains(&"kb_get"));
        assert!(names.contains(&"kb_delete"));
        assert!(names.contains(&"kb_list_tags"));
    }

    #[test]
    fn test_store_and_search() {
        let tmp = std::env::temp_dir().join("abigail_kb_test_store_search");
        let _ = fs::remove_dir_all(&tmp);
        fs::create_dir_all(&tmp).unwrap();

        let skill = test_skill(tmp.clone());

        // Store an entry
        let store_params = ToolParams::new()
            .with("title", "Rust Ownership")
            .with("content", "Rust uses an ownership model for memory safety.")
            .with("tags", vec!["rust", "memory"])
            .with("category", "Programming");

        let store_result = skill.kb_store(&store_params).unwrap();
        assert!(store_result.success);
        let store_data = store_result.data.unwrap();
        let stored_id = store_data["id"].as_str().unwrap().to_string();
        assert!(!stored_id.is_empty());

        // Search by title keyword
        let search_params = ToolParams::new().with("query", "Ownership");
        let search_result = skill.kb_search(&search_params).unwrap();
        assert!(search_result.success);
        let search_data = search_result.data.unwrap();
        assert_eq!(search_data["count"], 1);

        // Search by content keyword
        let search_params2 = ToolParams::new().with("query", "memory safety");
        let search_result2 = skill.kb_search(&search_params2).unwrap();
        assert!(search_result2.success);
        let search_data2 = search_result2.data.unwrap();
        assert_eq!(search_data2["count"], 1);

        // Search with tag filter
        let search_params3 = ToolParams::new()
            .with("query", "Rust")
            .with("tag", "memory");
        let search_result3 = skill.kb_search(&search_params3).unwrap();
        assert!(search_result3.success);
        let search_data3 = search_result3.data.unwrap();
        assert_eq!(search_data3["count"], 1);

        // Search that returns nothing
        let search_params4 = ToolParams::new().with("query", "nonexistent topic");
        let search_result4 = skill.kb_search(&search_params4).unwrap();
        assert!(search_result4.success);
        let search_data4 = search_result4.data.unwrap();
        assert_eq!(search_data4["count"], 0);

        // Get by ID
        let get_params = ToolParams::new().with("id", &stored_id);
        let get_result = skill.kb_get(&get_params).unwrap();
        assert!(get_result.success);
        let get_data = get_result.data.unwrap();
        assert_eq!(
            get_data["entry"]["title"].as_str().unwrap(),
            "Rust Ownership"
        );

        // List tags
        let tags_result = skill.kb_list_tags().unwrap();
        assert!(tags_result.success);
        let tags_data = tags_result.data.unwrap();
        let tags: Vec<String> = serde_json::from_value(tags_data["tags"].clone()).unwrap();
        assert!(tags.contains(&"rust".to_string()));
        assert!(tags.contains(&"memory".to_string()));

        let _ = fs::remove_dir_all(&tmp);
    }

    #[test]
    fn test_store_and_delete() {
        let tmp = std::env::temp_dir().join("abigail_kb_test_store_delete");
        let _ = fs::remove_dir_all(&tmp);
        fs::create_dir_all(&tmp).unwrap();

        let skill = test_skill(tmp.clone());

        // Store an entry
        let store_params = ToolParams::new()
            .with("title", "Temporary Note")
            .with("content", "This will be deleted.");
        let store_result = skill.kb_store(&store_params).unwrap();
        assert!(store_result.success);
        let store_data = store_result.data.unwrap();
        let stored_id = store_data["id"].as_str().unwrap().to_string();

        // Verify it exists
        let get_params = ToolParams::new().with("id", &stored_id);
        let get_result = skill.kb_get(&get_params).unwrap();
        assert!(get_result.success);

        // Delete it
        let delete_params = ToolParams::new().with("id", &stored_id);
        let delete_result = skill.kb_delete(&delete_params).unwrap();
        assert!(delete_result.success);
        let delete_data = delete_result.data.unwrap();
        assert_eq!(delete_data["deleted"], true);

        // Verify it is gone
        let get_result2 = skill.kb_get(&get_params);
        assert!(get_result2.is_err());

        // Delete non-existent entry returns error output (not Err)
        let delete_result2 = skill
            .kb_delete(&ToolParams::new().with("id", "does-not-exist"))
            .unwrap();
        assert!(!delete_result2.success);

        let _ = fs::remove_dir_all(&tmp);
    }
}
