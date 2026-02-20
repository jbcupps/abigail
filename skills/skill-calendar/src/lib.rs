//! Calendar skill: manage events and schedules with a local SQLite database.
//!
//! Provides tools to add, list, update, and delete calendar events. Events are
//! stored in a `calendar.db` SQLite database within the configured data directory.

use abigail_skills::{
    CapabilityDescriptor, CostEstimate, ExecutionContext, FileSystemPermission, HealthStatus,
    Permission, Skill, SkillConfig, SkillError, SkillHealth, SkillManifest, SkillResult,
    ToolDescriptor, ToolOutput, ToolParams, TriggerDescriptor,
};
use async_trait::async_trait;
use std::any::Any;
use std::collections::HashMap;
use std::path::PathBuf;

/// Calendar skill backed by a local SQLite database.
pub struct CalendarSkill {
    manifest: SkillManifest,
    /// Directory where calendar.db will be stored.
    data_dir: PathBuf,
}

impl CalendarSkill {
    /// Parse the embedded skill.toml manifest.
    pub fn default_manifest() -> SkillManifest {
        let toml_str = include_str!("../skill.toml");
        SkillManifest::parse(toml_str).expect("Failed to parse calendar skill.toml")
    }

    /// Create a new calendar skill storing its database in `data_dir`.
    pub fn new(manifest: SkillManifest, data_dir: PathBuf) -> Self {
        Self { manifest, data_dir }
    }

    /// Open (or create) the calendar database and ensure the schema exists.
    fn ensure_db(&self) -> Result<rusqlite::Connection, SkillError> {
        std::fs::create_dir_all(&self.data_dir)
            .map_err(|e| SkillError::InitFailed(format!("Cannot create data directory: {}", e)))?;

        let db_path = self.data_dir.join("calendar.db");
        let conn = rusqlite::Connection::open(&db_path)
            .map_err(|e| SkillError::InitFailed(format!("Cannot open calendar.db: {}", e)))?;

        conn.execute_batch(
            "CREATE TABLE IF NOT EXISTS events (
                id          TEXT PRIMARY KEY,
                title       TEXT NOT NULL,
                description TEXT,
                start_time  TEXT NOT NULL,
                end_time    TEXT,
                location    TEXT,
                created_at  TEXT NOT NULL
            );",
        )
        .map_err(|e| SkillError::InitFailed(format!("Cannot create events table: {}", e)))?;

        Ok(conn)
    }

    /// Add a new calendar event.
    fn add_event(
        &self,
        title: &str,
        description: Option<&str>,
        start_time: &str,
        end_time: Option<&str>,
        location: Option<&str>,
    ) -> SkillResult<ToolOutput> {
        let conn = self.ensure_db()?;
        let id = uuid::Uuid::new_v4().to_string();
        let created_at = chrono::Utc::now().to_rfc3339();

        conn.execute(
            "INSERT INTO events (id, title, description, start_time, end_time, location, created_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
            rusqlite::params![id, title, description, start_time, end_time, location, created_at],
        )
        .map_err(|e| SkillError::ToolFailed(format!("Failed to insert event: {}", e)))?;

        tracing::info!("Created calendar event '{}' ({})", title, id);

        Ok(ToolOutput::success(serde_json::json!({
            "formatted": format!("Event '{}' created (id: {})", title, id),
            "id": id,
            "title": title,
            "start_time": start_time,
        })))
    }

    /// List calendar events, optionally filtered by a date range.
    fn list_events(&self, from: Option<&str>, to: Option<&str>) -> SkillResult<ToolOutput> {
        let conn = self.ensure_db()?;

        let (sql, params): (String, Vec<Box<dyn rusqlite::types::ToSql>>) = match (from, to) {
            (Some(f), Some(t)) => (
                "SELECT id, title, description, start_time, end_time, location, created_at \
                 FROM events WHERE start_time >= ?1 AND start_time <= ?2 ORDER BY start_time"
                    .to_string(),
                vec![Box::new(f.to_string()), Box::new(t.to_string())],
            ),
            (Some(f), None) => (
                "SELECT id, title, description, start_time, end_time, location, created_at \
                 FROM events WHERE start_time >= ?1 ORDER BY start_time"
                    .to_string(),
                vec![Box::new(f.to_string())],
            ),
            (None, Some(t)) => (
                "SELECT id, title, description, start_time, end_time, location, created_at \
                 FROM events WHERE start_time <= ?1 ORDER BY start_time"
                    .to_string(),
                vec![Box::new(t.to_string())],
            ),
            (None, None) => (
                "SELECT id, title, description, start_time, end_time, location, created_at \
                 FROM events ORDER BY start_time"
                    .to_string(),
                vec![],
            ),
        };

        let param_refs: Vec<&dyn rusqlite::types::ToSql> =
            params.iter().map(|p| p.as_ref()).collect();

        let mut stmt = conn
            .prepare(&sql)
            .map_err(|e| SkillError::ToolFailed(format!("Query prepare failed: {}", e)))?;

        let rows = stmt
            .query_map(param_refs.as_slice(), |row| {
                Ok(serde_json::json!({
                    "id": row.get::<_, String>(0)?,
                    "title": row.get::<_, String>(1)?,
                    "description": row.get::<_, Option<String>>(2)?,
                    "start_time": row.get::<_, String>(3)?,
                    "end_time": row.get::<_, Option<String>>(4)?,
                    "location": row.get::<_, Option<String>>(5)?,
                    "created_at": row.get::<_, String>(6)?,
                }))
            })
            .map_err(|e| SkillError::ToolFailed(format!("Query failed: {}", e)))?;

        let events: Vec<serde_json::Value> = rows.filter_map(|r| r.ok()).collect();

        let formatted = if events.is_empty() {
            "No events found.".to_string()
        } else {
            events
                .iter()
                .map(|e| {
                    let end = e["end_time"]
                        .as_str()
                        .map(|t| format!(" - {}", t))
                        .unwrap_or_default();
                    let loc = e["location"]
                        .as_str()
                        .map(|l| format!(" @ {}", l))
                        .unwrap_or_default();
                    format!(
                        "[{}] {} ({}{}{})",
                        e["id"].as_str().unwrap_or(""),
                        e["title"].as_str().unwrap_or(""),
                        e["start_time"].as_str().unwrap_or(""),
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

    /// Delete a calendar event by ID.
    fn delete_event(&self, id: &str) -> SkillResult<ToolOutput> {
        let conn = self.ensure_db()?;

        let affected = conn
            .execute("DELETE FROM events WHERE id = ?1", rusqlite::params![id])
            .map_err(|e| SkillError::ToolFailed(format!("Delete failed: {}", e)))?;

        if affected == 0 {
            return Ok(ToolOutput::error(format!(
                "No event found with id '{}'",
                id
            )));
        }

        tracing::info!("Deleted calendar event {}", id);

        Ok(ToolOutput::success(serde_json::json!({
            "formatted": format!("Event '{}' deleted.", id),
            "id": id,
            "deleted": true,
        })))
    }

    /// Update specified fields of a calendar event.
    fn update_event(
        &self,
        id: &str,
        title: Option<&str>,
        description: Option<&str>,
        start_time: Option<&str>,
        end_time: Option<&str>,
        location: Option<&str>,
    ) -> SkillResult<ToolOutput> {
        let conn = self.ensure_db()?;

        // Build SET clause dynamically for provided fields.
        let mut sets: Vec<String> = Vec::new();
        let mut params: Vec<Box<dyn rusqlite::types::ToSql>> = Vec::new();
        let mut idx = 1u32;

        if let Some(v) = title {
            sets.push(format!("title = ?{}", idx));
            params.push(Box::new(v.to_string()));
            idx += 1;
        }
        if let Some(v) = description {
            sets.push(format!("description = ?{}", idx));
            params.push(Box::new(v.to_string()));
            idx += 1;
        }
        if let Some(v) = start_time {
            sets.push(format!("start_time = ?{}", idx));
            params.push(Box::new(v.to_string()));
            idx += 1;
        }
        if let Some(v) = end_time {
            sets.push(format!("end_time = ?{}", idx));
            params.push(Box::new(v.to_string()));
            idx += 1;
        }
        if let Some(v) = location {
            sets.push(format!("location = ?{}", idx));
            params.push(Box::new(v.to_string()));
            idx += 1;
        }

        if sets.is_empty() {
            return Ok(ToolOutput::error(
                "No fields provided to update.".to_string(),
            ));
        }

        let sql = format!("UPDATE events SET {} WHERE id = ?{}", sets.join(", "), idx);
        params.push(Box::new(id.to_string()));

        let param_refs: Vec<&dyn rusqlite::types::ToSql> =
            params.iter().map(|p| p.as_ref()).collect();

        let affected = conn
            .execute(&sql, param_refs.as_slice())
            .map_err(|e| SkillError::ToolFailed(format!("Update failed: {}", e)))?;

        if affected == 0 {
            return Ok(ToolOutput::error(format!(
                "No event found with id '{}'",
                id
            )));
        }

        tracing::info!("Updated calendar event {}", id);

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
        // Ensure the database and schema exist on initialization.
        let _conn = self.ensure_db()?;
        Ok(())
    }

    async fn shutdown(&mut self) -> SkillResult<()> {
        Ok(())
    }

    fn health(&self) -> SkillHealth {
        let db_ok = self.ensure_db().is_ok();
        SkillHealth {
            status: if db_ok {
                HealthStatus::Healthy
            } else {
                HealthStatus::Degraded
            },
            message: if !db_ok {
                Some("Cannot open or create calendar database".to_string())
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
                name: "calendar_add_event".to_string(),
                description:
                    "Add a new calendar event with a title, start time, and optional details."
                        .to_string(),
                parameters: serde_json::json!({
                    "type": "object",
                    "properties": {
                        "title": {
                            "type": "string",
                            "description": "Title of the event"
                        },
                        "description": {
                            "type": "string",
                            "description": "Optional description of the event"
                        },
                        "start_time": {
                            "type": "string",
                            "description": "Start time in ISO 8601 format (e.g. 2026-02-19T14:00:00Z)"
                        },
                        "end_time": {
                            "type": "string",
                            "description": "Optional end time in ISO 8601 format"
                        },
                        "location": {
                            "type": "string",
                            "description": "Optional location of the event"
                        }
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
                        "from": {
                            "type": "string",
                            "description": "Optional start of date range in ISO 8601 format"
                        },
                        "to": {
                            "type": "string",
                            "description": "Optional end of date range in ISO 8601 format"
                        }
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
                    "properties": {
                        "id": {
                            "type": "string",
                            "description": "The unique ID of the event to delete"
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
                name: "calendar_update_event".to_string(),
                description: "Update one or more fields of an existing calendar event.".to_string(),
                parameters: serde_json::json!({
                    "type": "object",
                    "properties": {
                        "id": {
                            "type": "string",
                            "description": "The unique ID of the event to update"
                        },
                        "title": {
                            "type": "string",
                            "description": "New title for the event"
                        },
                        "description": {
                            "type": "string",
                            "description": "New description for the event"
                        },
                        "start_time": {
                            "type": "string",
                            "description": "New start time in ISO 8601 format"
                        },
                        "end_time": {
                            "type": "string",
                            "description": "New end time in ISO 8601 format"
                        },
                        "location": {
                            "type": "string",
                            "description": "New location for the event"
                        }
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

#[cfg(test)]
mod tests {
    use super::*;

    fn test_skill() -> CalendarSkill {
        let tmp = std::env::temp_dir().join("abigail_calendar_test");
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
        let names: Vec<&str> = tools.iter().map(|t| t.name.as_str()).collect();
        assert!(names.contains(&"calendar_add_event"));
        assert!(names.contains(&"calendar_list_events"));
        assert!(names.contains(&"calendar_delete_event"));
        assert!(names.contains(&"calendar_update_event"));
    }

    #[test]
    fn test_delete_requires_confirmation() {
        let skill = test_skill();
        let tools = skill.tools();
        let delete_tool = tools
            .iter()
            .find(|t| t.name == "calendar_delete_event")
            .unwrap();
        assert!(delete_tool.requires_confirmation);
        assert!(!delete_tool.autonomous);
    }

    #[test]
    fn test_update_requires_confirmation() {
        let skill = test_skill();
        let tools = skill.tools();
        let update_tool = tools
            .iter()
            .find(|t| t.name == "calendar_update_event")
            .unwrap();
        assert!(update_tool.requires_confirmation);
        assert!(!update_tool.autonomous);
    }
}
