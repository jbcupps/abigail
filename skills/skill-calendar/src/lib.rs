//! Calendar skill backed by the shared SurrealDB memory store.

use abigail_persistence::{EntityScope, PersistenceHandle};
use abigail_skills::{
    CapabilityDescriptor, CostEstimate, ExecutionContext, FileSystemPermission, HealthStatus,
    Permission, Skill, SkillConfig, SkillError, SkillHealth, SkillManifest, SkillResult,
    ToolDescriptor, ToolOutput, ToolParams, TriggerDescriptor,
};
use async_trait::async_trait;
use std::any::Any;
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use uuid::Uuid;

pub struct CalendarSkill {
    manifest: SkillManifest,
    data_dir: PathBuf,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
struct CalendarEventDoc {
    id: String,
    title: String,
    description: Option<String>,
    start_time: String,
    end_time: Option<String>,
    location: Option<String>,
    created_at: String,
}

impl CalendarSkill {
    pub fn default_manifest() -> SkillManifest {
        let toml_str = include_str!("../skill.toml");
        SkillManifest::parse(toml_str).expect("Failed to parse calendar skill.toml")
    }

    pub fn new(manifest: SkillManifest, data_dir: PathBuf) -> Self {
        Self { manifest, data_dir }
    }

    fn open_store(&self) -> SkillResult<PersistenceHandle> {
        std::fs::create_dir_all(&self.data_dir)
            .map_err(|e| SkillError::InitFailed(format!("Cannot create data directory: {}", e)))?;
        PersistenceHandle::open(shared_db_path(&self.data_dir), infer_scope(&self.data_dir))
            .map_err(|e| SkillError::InitFailed(format!("Cannot open memory store: {}", e)))
    }

    fn list_all(&self) -> SkillResult<Vec<CalendarEventDoc>> {
        let store = self.open_store()?;
        store
            .query_vec("SELECT * FROM calendar_event ORDER BY start_time ASC", &[])
            .map_err(|e| SkillError::ToolFailed(format!("Failed to load events: {}", e)))
    }

    fn get_event(&self, id: &str) -> SkillResult<Option<CalendarEventDoc>> {
        let store = self.open_store()?;
        store
            .select_record("calendar_event", id)
            .map_err(|e| SkillError::ToolFailed(format!("Failed to load event: {}", e)))
    }

    fn add_event(
        &self,
        title: &str,
        description: Option<&str>,
        start_time: &str,
        end_time: Option<&str>,
        location: Option<&str>,
    ) -> SkillResult<ToolOutput> {
        let store = self.open_store()?;
        let id = Uuid::new_v4().to_string();
        let created_at = chrono::Utc::now().to_rfc3339();
        let event = CalendarEventDoc {
            id: id.clone(),
            title: title.to_string(),
            description: description.map(str::to_string),
            start_time: start_time.to_string(),
            end_time: end_time.map(str::to_string),
            location: location.map(str::to_string),
            created_at,
        };
        store
            .create("calendar_event", &id, &event)
            .map_err(|e| SkillError::ToolFailed(format!("Failed to insert event: {}", e)))?;

        Ok(ToolOutput::success(serde_json::json!({
            "formatted": format!("Event '{}' created (id: {})", title, id),
            "id": id,
            "title": title,
            "start_time": start_time,
        })))
    }

    fn list_events(&self, from: Option<&str>, to: Option<&str>) -> SkillResult<ToolOutput> {
        let events: Vec<serde_json::Value> = self
            .list_all()?
            .into_iter()
            .filter(|event| {
                from.map(|min| event.start_time.as_str() >= min)
                    .unwrap_or(true)
            })
            .filter(|event| {
                to.map(|max| event.start_time.as_str() <= max)
                    .unwrap_or(true)
            })
            .map(|event| serde_json::to_value(event).unwrap_or_default())
            .collect();

        let formatted = if events.is_empty() {
            "No events found.".to_string()
        } else {
            events
                .iter()
                .map(|event| {
                    let end = event["end_time"]
                        .as_str()
                        .map(|time| format!(" - {}", time))
                        .unwrap_or_default();
                    let loc = event["location"]
                        .as_str()
                        .map(|location| format!(" @ {}", location))
                        .unwrap_or_default();
                    format!(
                        "[{}] {} ({}{}{})",
                        event["id"].as_str().unwrap_or(""),
                        event["title"].as_str().unwrap_or(""),
                        event["start_time"].as_str().unwrap_or(""),
                        end,
                        loc,
                    )
                })
                .collect::<Vec<_>>()
                .join("\n")
        };

        Ok(ToolOutput::success(serde_json::json!({
            "formatted": formatted,
            "count": events.len(),
            "events": events,
        })))
    }

    fn delete_event(&self, id: &str) -> SkillResult<ToolOutput> {
        let Some(_) = self.get_event(id)? else {
            return Ok(ToolOutput::error(format!(
                "No event found with id '{}'",
                id
            )));
        };

        self.open_store()?
            .delete_record("calendar_event", id)
            .map_err(|e| SkillError::ToolFailed(format!("Delete failed: {}", e)))?;

        Ok(ToolOutput::success(serde_json::json!({
            "formatted": format!("Event '{}' deleted.", id),
            "id": id,
            "deleted": true,
        })))
    }

    fn update_event(
        &self,
        id: &str,
        title: Option<&str>,
        description: Option<&str>,
        start_time: Option<&str>,
        end_time: Option<&str>,
        location: Option<&str>,
    ) -> SkillResult<ToolOutput> {
        let Some(mut event) = self.get_event(id)? else {
            return Ok(ToolOutput::error(format!(
                "No event found with id '{}'",
                id
            )));
        };

        if let Some(title) = title {
            event.title = title.to_string();
        }
        if let Some(description) = description {
            event.description = Some(description.to_string());
        }
        if let Some(start_time) = start_time {
            event.start_time = start_time.to_string();
        }
        if let Some(end_time) = end_time {
            event.end_time = Some(end_time.to_string());
        }
        if let Some(location) = location {
            event.location = Some(location.to_string());
        }

        self.open_store()?
            .upsert("calendar_event", id, &event)
            .map_err(|e| SkillError::ToolFailed(format!("Update failed: {}", e)))?;

        Ok(ToolOutput::success(serde_json::json!({
            "formatted": format!("Event '{}' updated.", id),
            "id": id,
            "updated": true,
        })))
    }
}

#[async_trait]
impl Skill for CalendarSkill {
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
                Some("Cannot open calendar store".to_string())
            },
            last_check: chrono::Utc::now(),
            metrics: HashMap::new(),
        }
    }

    fn tools(&self) -> Vec<ToolDescriptor> {
        vec![
            ToolDescriptor {
                name: "calendar_add_event".to_string(),
                description:
                    "Add a new calendar event with a title, start time, and optional details."
                        .to_string(),
                parameters: serde_json::json!({
                    "type": "object",
                    "properties": {
                        "title": { "type": "string", "description": "Title of the event" },
                        "description": { "type": "string", "description": "Optional description of the event" },
                        "start_time": { "type": "string", "description": "Start time in ISO 8601 format" },
                        "end_time": { "type": "string", "description": "Optional end time in ISO 8601 format" },
                        "location": { "type": "string", "description": "Optional location of the event" }
                    },
                    "required": ["title", "start_time"]
                }),
                returns: serde_json::json!({
                    "type": "object",
                    "properties": {
                        "formatted": { "type": "string" },
                        "id": { "type": "string" },
                        "title": { "type": "string" },
                        "start_time": { "type": "string" }
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
                name: "calendar_list_events".to_string(),
                description: "List calendar events, optionally filtered by a date range."
                    .to_string(),
                parameters: serde_json::json!({
                    "type": "object",
                    "properties": {
                        "from": { "type": "string", "description": "Optional start of date range in ISO 8601 format" },
                        "to": { "type": "string", "description": "Optional end of date range in ISO 8601 format" }
                    }
                }),
                returns: serde_json::json!({
                    "type": "object",
                    "properties": {
                        "formatted": { "type": "string" },
                        "count": { "type": "integer" },
                        "events": { "type": "array" }
                    }
                }),
                cost_estimate: CostEstimate {
                    latency_ms: 10,
                    network_bound: false,
                    token_cost: None,
                },
                required_permissions: vec![Permission::FileSystem(FileSystemPermission::Read(
                    vec!["~".to_string()],
                ))],
                autonomous: true,
                requires_confirmation: false,
            },
            ToolDescriptor {
                name: "calendar_delete_event".to_string(),
                description: "Delete a calendar event by its ID.".to_string(),
                parameters: serde_json::json!({
                    "type": "object",
                    "properties": { "id": { "type": "string", "description": "The unique ID of the event to delete" } },
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
                name: "calendar_update_event".to_string(),
                description: "Update one or more fields of an existing calendar event.".to_string(),
                parameters: serde_json::json!({
                    "type": "object",
                    "properties": {
                        "id": { "type": "string", "description": "The unique ID of the event to update" },
                        "title": { "type": "string", "description": "New title for the event" },
                        "description": { "type": "string", "description": "New description for the event" },
                        "start_time": { "type": "string", "description": "New start time in ISO 8601 format" },
                        "end_time": { "type": "string", "description": "New end time in ISO 8601 format" },
                        "location": { "type": "string", "description": "New location for the event" }
                    },
                    "required": ["id"]
                }),
                returns: serde_json::json!({
                    "type": "object",
                    "properties": {
                        "formatted": { "type": "string" },
                        "id": { "type": "string" },
                        "updated": { "type": "boolean" }
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
        ]
    }

    async fn execute_tool(
        &self,
        tool_name: &str,
        params: ToolParams,
        _context: &ExecutionContext,
    ) -> SkillResult<ToolOutput> {
        match tool_name {
            "calendar_add_event" => {
                let title: String = params.get("title").ok_or_else(|| {
                    SkillError::ToolFailed("Missing required parameter: title".to_string())
                })?;
                let start_time: String = params.get("start_time").ok_or_else(|| {
                    SkillError::ToolFailed("Missing required parameter: start_time".to_string())
                })?;
                let description: Option<String> = params.get("description");
                let end_time: Option<String> = params.get("end_time");
                let location: Option<String> = params.get("location");
                self.add_event(
                    &title,
                    description.as_deref(),
                    &start_time,
                    end_time.as_deref(),
                    location.as_deref(),
                )
            }
            "calendar_list_events" => {
                let from: Option<String> = params.get("from");
                let to: Option<String> = params.get("to");
                self.list_events(from.as_deref(), to.as_deref())
            }
            "calendar_delete_event" => {
                let id: String = params.get("id").ok_or_else(|| {
                    SkillError::ToolFailed("Missing required parameter: id".to_string())
                })?;
                self.delete_event(&id)
            }
            "calendar_update_event" => {
                let id: String = params.get("id").ok_or_else(|| {
                    SkillError::ToolFailed("Missing required parameter: id".to_string())
                })?;
                let title: Option<String> = params.get("title");
                let description: Option<String> = params.get("description");
                let start_time: Option<String> = params.get("start_time");
                let end_time: Option<String> = params.get("end_time");
                let location: Option<String> = params.get("location");
                self.update_event(
                    &id,
                    title.as_deref(),
                    description.as_deref(),
                    start_time.as_deref(),
                    end_time.as_deref(),
                    location.as_deref(),
                )
            }
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

    fn test_skill() -> CalendarSkill {
        let tmp = std::env::temp_dir().join(format!("abigail_calendar_test_{}", Uuid::new_v4()));
        CalendarSkill::new(CalendarSkill::default_manifest(), tmp)
    }

    #[test]
    fn test_manifest_parses() {
        let manifest = CalendarSkill::default_manifest();
        assert_eq!(manifest.name, "Calendar");
    }

    #[test]
    fn test_tools_list() {
        let skill = test_skill();
        let tools = skill.tools();
        assert_eq!(tools.len(), 4);
    }
}
