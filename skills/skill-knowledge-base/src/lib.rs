//! Knowledge Base skill backed by the shared SurrealDB memory store.

use abigail_persistence::{EntityScope, PersistenceHandle};
use abigail_skills::{
    CapabilityDescriptor, CostEstimate, ExecutionContext, FileSystemPermission, HealthStatus,
    Permission, Skill, SkillConfig, SkillError, SkillHealth, SkillManifest, SkillResult,
    ToolDescriptor, ToolOutput, ToolParams, TriggerDescriptor,
};
use async_trait::async_trait;
use std::any::Any;
use std::collections::{BTreeSet, HashMap};
use std::path::{Path, PathBuf};
use uuid::Uuid;

pub struct KnowledgeBaseSkill {
    manifest: SkillManifest,
    data_dir: PathBuf,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
struct KnowledgeEntryDoc {
    id: String,
    title: String,
    content: String,
    tags: Option<String>,
    category: Option<String>,
    created_at: String,
    updated_at: String,
}

impl KnowledgeBaseSkill {
    pub fn default_manifest() -> SkillManifest {
        let toml_str = include_str!("../skill.toml");
        SkillManifest::parse(toml_str).expect("Failed to parse knowledge-base skill.toml")
    }

    pub fn new(manifest: SkillManifest, data_dir: PathBuf) -> Self {
        Self { manifest, data_dir }
    }

    fn open_store(&self) -> SkillResult<PersistenceHandle> {
        std::fs::create_dir_all(&self.data_dir)
            .map_err(|e| SkillError::InitFailed(format!("Cannot create data directory: {}", e)))?;
        PersistenceHandle::open(shared_db_path(&self.data_dir), infer_scope(&self.data_dir))
            .map_err(|e| SkillError::InitFailed(format!("Cannot open knowledge base store: {}", e)))
    }

    fn list_all(&self) -> SkillResult<Vec<KnowledgeEntryDoc>> {
        self.open_store()?
            .query_vec("SELECT * FROM kb_entry ORDER BY updated_at DESC", &[])
            .map_err(|e| SkillError::ToolFailed(format!("Failed to load knowledge entries: {}", e)))
    }

    fn get_entry(&self, id: &str) -> SkillResult<Option<KnowledgeEntryDoc>> {
        self.open_store()?
            .select_record("kb_entry", id)
            .map_err(|e| SkillError::ToolFailed(format!("Failed to load entry: {}", e)))
    }

    fn kb_store(&self, params: &ToolParams) -> SkillResult<ToolOutput> {
        let title: String = params.get("title").ok_or_else(|| {
            SkillError::ToolFailed("Missing required parameter: title".to_string())
        })?;
        let content: String = params.get("content").ok_or_else(|| {
            SkillError::ToolFailed("Missing required parameter: content".to_string())
        })?;
        let tags: Option<Vec<String>> = params.get("tags");
        let category: Option<String> = params.get("category");

        let now = chrono::Utc::now().to_rfc3339();
        let id = Uuid::new_v4().to_string();
        let entry = KnowledgeEntryDoc {
            id: id.clone(),
            title: title.clone(),
            content,
            tags: tags.map(|tags| tags.join(",")),
            category,
            created_at: now.clone(),
            updated_at: now,
        };

        self.open_store()?
            .create("kb_entry", &id, &entry)
            .map_err(|e| SkillError::ToolFailed(format!("Insert failed: {}", e)))?;

        Ok(ToolOutput::success(serde_json::json!({
            "formatted": format!("Stored entry '{}' (id: {})", title, id),
            "id": id,
            "title": title,
        })))
    }

    fn kb_search(&self, params: &ToolParams) -> SkillResult<ToolOutput> {
        let query: String = params.get("query").ok_or_else(|| {
            SkillError::ToolFailed("Missing required parameter: query".to_string())
        })?;
        let tag: Option<String> = params.get("tag");
        let query_lower = query.to_ascii_lowercase();

        let results: Vec<serde_json::Value> = self
            .list_all()?
            .into_iter()
            .filter(|entry| {
                entry.title.to_ascii_lowercase().contains(&query_lower)
                    || entry.content.to_ascii_lowercase().contains(&query_lower)
            })
            .filter(|entry| {
                tag.as_ref()
                    .map(|tag| {
                        entry
                            .tags
                            .as_ref()
                            .map(|csv| csv.split(',').any(|item| item.trim() == tag))
                            .unwrap_or(false)
                    })
                    .unwrap_or(true)
            })
            .map(|entry| serde_json::to_value(entry).unwrap_or_default())
            .collect();

        let formatted = if results.is_empty() {
            format!("No entries found matching '{}'.", query)
        } else {
            results
                .iter()
                .map(|entry| {
                    format!(
                        "- [{}] {} (tags: {})",
                        entry["id"].as_str().unwrap_or("?"),
                        entry["title"].as_str().unwrap_or("?"),
                        entry["tags"].as_str().unwrap_or("none"),
                    )
                })
                .collect::<Vec<_>>()
                .join("\n")
        };

        Ok(ToolOutput::success(serde_json::json!({
            "formatted": formatted,
            "count": results.len(),
            "entries": results,
        })))
    }

    fn kb_get(&self, params: &ToolParams) -> SkillResult<ToolOutput> {
        let id: String = params
            .get("id")
            .ok_or_else(|| SkillError::ToolFailed("Missing required parameter: id".to_string()))?;

        let entry = self
            .get_entry(&id)?
            .ok_or_else(|| SkillError::ToolFailed(format!("Entry not found: {}", id)))?;

        let value = serde_json::to_value(&entry).unwrap_or_default();
        let formatted = format!(
            "# {}\n\n{}\n\nTags: {}\nCategory: {}\nCreated: {}\nUpdated: {}",
            entry.title,
            entry.content,
            entry.tags.clone().unwrap_or_else(|| "none".to_string()),
            entry.category.clone().unwrap_or_else(|| "none".to_string()),
            entry.created_at,
            entry.updated_at,
        );

        Ok(ToolOutput::success(serde_json::json!({
            "formatted": formatted,
            "entry": value,
        })))
    }

    fn kb_delete(&self, params: &ToolParams) -> SkillResult<ToolOutput> {
        let id: String = params
            .get("id")
            .ok_or_else(|| SkillError::ToolFailed("Missing required parameter: id".to_string()))?;

        if self.get_entry(&id)?.is_none() {
            return Ok(ToolOutput::error(format!("Entry not found: {}", id)));
        }

        self.open_store()?
            .delete_record("kb_entry", &id)
            .map_err(|e| SkillError::ToolFailed(format!("Delete failed: {}", e)))?;

        Ok(ToolOutput::success(serde_json::json!({
            "formatted": format!("Deleted entry {}", id),
            "id": id,
            "deleted": true,
        })))
    }

    fn kb_list_tags(&self) -> SkillResult<ToolOutput> {
        let mut tags = BTreeSet::new();
        for entry in self.list_all()? {
            if let Some(csv) = entry.tags {
                for tag in csv.split(',') {
                    let trimmed = tag.trim();
                    if !trimmed.is_empty() {
                        tags.insert(trimmed.to_string());
                    }
                }
            }
        }
        let tags: Vec<String> = tags.into_iter().collect();
        Ok(ToolOutput::success(serde_json::json!({
            "formatted": if tags.is_empty() { "No tags found.".to_string() } else { tags.join(", ") },
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
        let _ = self.open_store()?;
        Ok(())
    }

    async fn shutdown(&mut self) -> SkillResult<()> {
        Ok(())
    }

    fn health(&self) -> SkillHealth {
        let healthy = self.open_store().is_ok();
        SkillHealth {
            status: if healthy {
                HealthStatus::Healthy
            } else {
                HealthStatus::Degraded
            },
            message: if healthy {
                None
            } else {
                Some("Knowledge base store is unavailable".to_string())
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
                        "title": { "type": "string", "description": "Title of the knowledge entry" },
                        "content": { "type": "string", "description": "Body content of the knowledge entry" },
                        "tags": { "type": "array", "items": { "type": "string" }, "description": "Optional list of tags" },
                        "category": { "type": "string", "description": "Optional category for the entry" }
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
                cost_estimate: CostEstimate { latency_ms: 10, network_bound: false, token_cost: None },
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
                        "query": { "type": "string", "description": "Search text to match in title or content" },
                        "tag": { "type": "string", "description": "Optional tag to filter results" }
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
                cost_estimate: CostEstimate { latency_ms: 10, network_bound: false, token_cost: None },
                required_permissions: vec![Permission::FileSystem(FileSystemPermission::Read(
                    vec!["~".to_string()],
                ))],
                autonomous: true,
                requires_confirmation: false,
            },
            ToolDescriptor {
                name: "kb_get".to_string(),
                description: "Retrieve a single knowledge entry by its ID.".to_string(),
                parameters: serde_json::json!({
                    "type": "object",
                    "properties": { "id": { "type": "string", "description": "UUID of the entry to retrieve" } },
                    "required": ["id"]
                }),
                returns: serde_json::json!({
                    "type": "object",
                    "properties": { "formatted": { "type": "string" }, "entry": { "type": "object" } }
                }),
                cost_estimate: CostEstimate { latency_ms: 10, network_bound: false, token_cost: None },
                required_permissions: vec![Permission::FileSystem(FileSystemPermission::Read(
                    vec!["~".to_string()],
                ))],
                autonomous: true,
                requires_confirmation: false,
            },
            ToolDescriptor {
                name: "kb_delete".to_string(),
                description: "Delete a knowledge entry by its ID.".to_string(),
                parameters: serde_json::json!({
                    "type": "object",
                    "properties": { "id": { "type": "string", "description": "UUID of the entry to delete" } },
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
                cost_estimate: CostEstimate { latency_ms: 10, network_bound: false, token_cost: None },
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
                parameters: serde_json::json!({ "type": "object", "properties": {}, "required": [] }),
                returns: serde_json::json!({
                    "type": "object",
                    "properties": {
                        "formatted": { "type": "string" },
                        "count": { "type": "integer" },
                        "tags": { "type": "array" }
                    }
                }),
                cost_estimate: CostEstimate { latency_ms: 10, network_bound: false, token_cost: None },
                required_permissions: vec![Permission::FileSystem(FileSystemPermission::Read(
                    vec!["~".to_string()],
                ))],
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

fn infer_scope(data_dir: &Path) -> EntityScope {
    data_dir
        .file_name()
        .and_then(|name| name.to_str())
        .filter(|candidate| Uuid::parse_str(candidate).is_ok())
        .map(|id| EntityScope::Entity(id.to_string()))
        .or_else(|| {
            data_dir
                .parent()
                .and_then(|parent| parent.file_name())
                .and_then(|name| name.to_str())
                .filter(|candidate| Uuid::parse_str(candidate).is_ok())
                .map(|id| EntityScope::Entity(id.to_string()))
        })
        .unwrap_or(EntityScope::Hive)
}

fn shared_db_path(data_dir: &Path) -> PathBuf {
    data_dir
        .parent()
        .and_then(|parent| parent.parent())
        .map(|root| root.join("memory.db"))
        .unwrap_or_else(|| data_dir.join("memory.db"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_manifest_parses() {
        let manifest = KnowledgeBaseSkill::default_manifest();
        assert_eq!(manifest.name, "Knowledge Base");
    }
}
