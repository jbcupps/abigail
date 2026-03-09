//! Database skill: inspect and administer local embedded SurrealDB stores within sandboxed directories.

use abigail_persistence::{EntityScope, PersistenceHandle, QueryBinding};
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

const MAX_ROWS: usize = 1000;

pub struct DatabaseSkill {
    manifest: SkillManifest,
    allowed_roots: Vec<PathBuf>,
}

impl DatabaseSkill {
    pub fn default_manifest() -> SkillManifest {
        let toml_str = include_str!("../skill.toml");
        SkillManifest::parse(toml_str).expect("Failed to parse database skill.toml")
    }

    pub fn new(manifest: SkillManifest, allowed_roots: Vec<PathBuf>) -> Self {
        Self {
            manifest,
            allowed_roots,
        }
    }

    fn validate_path(&self, path_str: &str) -> SkillResult<PathBuf> {
        let path = PathBuf::from(path_str);
        let normalized = path_str.replace('\\', "/");
        if normalized.contains("../") || normalized.contains("/..") {
            return Err(SkillError::PermissionDenied(
                "Path traversal (../) is not allowed".to_string(),
            ));
        }

        if path.exists() {
            let canonical = path
                .canonicalize()
                .map_err(|e| SkillError::ToolFailed(format!("Cannot resolve path: {}", e)))?;
            if self.is_within_allowed_roots(&canonical) {
                return Ok(canonical);
            }
            return Err(SkillError::PermissionDenied(format!(
                "Path '{}' is outside allowed directories",
                path_str
            )));
        }

        if let Some(parent) = path.parent() {
            if parent.exists() {
                let canonical_parent = parent.canonicalize().map_err(|e| {
                    SkillError::ToolFailed(format!("Cannot resolve parent path: {}", e))
                })?;
                if self.is_within_allowed_roots(&canonical_parent) {
                    return Ok(canonical_parent.join(path.file_name().unwrap_or_default()));
                }
            }
        }

        Err(SkillError::PermissionDenied(format!(
            "Path '{}' is outside allowed directories",
            path_str
        )))
    }

    #[cfg(target_os = "windows")]
    fn strip_unc_prefix(path: &Path) -> PathBuf {
        let display = path.to_string_lossy();
        display
            .strip_prefix(r"\\?\")
            .map(PathBuf::from)
            .unwrap_or_else(|| path.to_path_buf())
    }

    #[cfg(not(target_os = "windows"))]
    fn strip_unc_prefix(path: &Path) -> PathBuf {
        path.to_path_buf()
    }

    fn is_within_allowed_roots(&self, canonical_path: &Path) -> bool {
        let canonical_path = Self::strip_unc_prefix(canonical_path);
        self.allowed_roots.iter().any(|root| {
            let canonical_root = root
                .canonicalize()
                .map(|value| Self::strip_unc_prefix(&value))
                .unwrap_or_else(|_| root.clone());
            canonical_path.starts_with(&canonical_root)
        })
    }

    fn open_store(&self, db_path: &str) -> SkillResult<PersistenceHandle> {
        let path = self.validate_path(db_path)?;
        PersistenceHandle::open(&path, infer_scope(&path))
            .map_err(|e| SkillError::ToolFailed(format!("Cannot open database: {}", e)))
    }

    fn rewrite_query(
        &self,
        statement: &str,
        params: &[String],
    ) -> SkillResult<(String, Vec<QueryBinding>)> {
        let mut query = statement.to_string();
        let mut bindings = Vec::with_capacity(params.len());
        for (index, value) in params.iter().enumerate() {
            let token = format!("?{}", index + 1);
            let name = format!("p{}", index);
            query = query.replace(&token, &format!("${}", name));
            bindings.push(
                QueryBinding::new(&name, value)
                    .map_err(|e| SkillError::ToolFailed(format!("Binding failed: {}", e)))?,
            );
        }
        Ok((query, bindings))
    }

    fn db_query(&self, db_path: &str, query: &str, params: &[String]) -> SkillResult<ToolOutput> {
        let upper = query.trim_start().to_ascii_uppercase();
        if !upper.starts_with("SELECT")
            && !upper.starts_with("RETURN")
            && !upper.starts_with("INFO")
            && !upper.starts_with("DEFINE")
        {
            return Err(SkillError::PermissionDenied(
                "db_query only allows read-oriented SurrealQL statements".to_string(),
            ));
        }

        let store = self.open_store(db_path)?;
        let (query, bindings) = self.rewrite_query(query, params)?;
        let rows: Vec<serde_json::Value> = store
            .query_vec(&query, &bindings)
            .map_err(|e| SkillError::ToolFailed(format!("Query execution failed: {}", e)))?;

        let column_names = rows
            .iter()
            .find_map(|row| {
                row.as_object()
                    .map(|object| object.keys().cloned().collect::<Vec<_>>())
            })
            .unwrap_or_default();

        let row_count = rows.len().min(MAX_ROWS);
        let truncated = rows.len() > MAX_ROWS;
        let rows = rows.into_iter().take(MAX_ROWS).collect::<Vec<_>>();
        let formatted = if row_count == 0 {
            "Query returned no results.".to_string()
        } else {
            format!(
                "Query returned {} row{}.",
                row_count,
                if row_count == 1 { "" } else { "s" }
            )
        };

        Ok(ToolOutput::success(serde_json::json!({
            "formatted": formatted,
            "columns": column_names,
            "rows": rows,
            "row_count": row_count,
            "truncated": truncated,
        })))
    }

    fn db_execute(
        &self,
        db_path: &str,
        statement: &str,
        params: &[String],
    ) -> SkillResult<ToolOutput> {
        let upper = statement.to_ascii_uppercase();
        if upper.contains("DROP TABLE") || upper.contains("DROP DATABASE") {
            return Err(SkillError::PermissionDenied(
                "DROP TABLE and DROP DATABASE statements are not allowed".to_string(),
            ));
        }

        let store = self.open_store(db_path)?;
        let (statement, bindings) = self.rewrite_query(statement, params)?;
        store
            .execute(&statement, &bindings)
            .map_err(|e| SkillError::ToolFailed(format!("Statement execution failed: {}", e)))?;

        Ok(ToolOutput::success(serde_json::json!({
            "formatted": "Statement executed successfully.".to_string(),
            "rows_affected": serde_json::Value::Null,
            "db_path": self.validate_path(db_path)?.display().to_string(),
        })))
    }

    fn db_schema(&self, db_path: &str) -> SkillResult<ToolOutput> {
        let store = self.open_store(db_path)?;
        let rows: Vec<serde_json::Value> = store
            .query_vec("INFO FOR DB", &[])
            .map_err(|e| SkillError::ToolFailed(format!("Schema inspection failed: {}", e)))?;
        let details = rows.into_iter().next().unwrap_or(serde_json::Value::Null);
        let tables = details
            .get("tables")
            .and_then(|value| value.as_object())
            .map(|tables| {
                tables
                    .iter()
                    .map(|(name, definition)| {
                        serde_json::json!({
                            "name": name,
                            "definition": definition,
                        })
                    })
                    .collect::<Vec<_>>()
            })
            .unwrap_or_default();

        Ok(ToolOutput::success(serde_json::json!({
            "formatted": format!("Database has {} table definition(s).", tables.len()),
            "db_path": self.validate_path(db_path)?.display().to_string(),
            "table_count": tables.len(),
            "tables": tables,
            "details": details,
        })))
    }
}

#[async_trait]
impl Skill for DatabaseSkill {
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
        let all_accessible = self.allowed_roots.iter().all(|root| root.exists());
        SkillHealth {
            status: if all_accessible {
                HealthStatus::Healthy
            } else {
                HealthStatus::Degraded
            },
            message: if all_accessible {
                None
            } else {
                Some("Some allowed root directories are not accessible".to_string())
            },
            last_check: chrono::Utc::now(),
            metrics: HashMap::new(),
        }
    }

    fn tools(&self) -> Vec<ToolDescriptor> {
        vec![
            ToolDescriptor {
                name: "db_query".to_string(),
                description: "Execute a read-oriented SurrealQL query against a local embedded database.".to_string(),
                parameters: serde_json::json!({
                    "type": "object",
                    "properties": {
                        "db_path": { "type": "string", "description": "Path to the embedded database store" },
                        "query": { "type": "string", "description": "SurrealQL SELECT/INFO/RETURN query to execute" },
                        "params": { "type": "array", "items": { "type": "string" }, "description": "Optional positional parameters (?1, ?2, ...)" }
                    },
                    "required": ["db_path", "query"]
                }),
                returns: serde_json::json!({
                    "type": "object",
                    "properties": {
                        "formatted": { "type": "string" },
                        "columns": { "type": "array" },
                        "rows": { "type": "array" },
                        "row_count": { "type": "integer" },
                        "truncated": { "type": "boolean" }
                    }
                }),
                cost_estimate: CostEstimate { latency_ms: 50, network_bound: false, token_cost: None },
                required_permissions: vec![Permission::FileSystem(FileSystemPermission::Read(
                    vec!["~".to_string()],
                ))],
                autonomous: true,
                requires_confirmation: false,
            },
            ToolDescriptor {
                name: "db_execute".to_string(),
                description: "Execute a mutating SurrealQL statement against a local embedded database. DROP TABLE and DROP DATABASE are blocked.".to_string(),
                parameters: serde_json::json!({
                    "type": "object",
                    "properties": {
                        "db_path": { "type": "string", "description": "Path to the embedded database store" },
                        "statement": { "type": "string", "description": "SurrealQL statement to execute" },
                        "params": { "type": "array", "items": { "type": "string" }, "description": "Optional positional parameters (?1, ?2, ...)" }
                    },
                    "required": ["db_path", "statement"]
                }),
                returns: serde_json::json!({
                    "type": "object",
                    "properties": {
                        "formatted": { "type": "string" },
                        "rows_affected": { "type": ["integer", "null"] },
                        "db_path": { "type": "string" }
                    }
                }),
                cost_estimate: CostEstimate { latency_ms: 50, network_bound: false, token_cost: None },
                required_permissions: vec![
                    Permission::FileSystem(FileSystemPermission::Read(vec!["~".to_string()])),
                    Permission::FileSystem(FileSystemPermission::Write(vec!["~".to_string()])),
                ],
                autonomous: false,
                requires_confirmation: true,
            },
            ToolDescriptor {
                name: "db_schema".to_string(),
                description: "Inspect SurrealDB database metadata and table definitions.".to_string(),
                parameters: serde_json::json!({
                    "type": "object",
                    "properties": {
                        "db_path": { "type": "string", "description": "Path to the embedded database store" }
                    },
                    "required": ["db_path"]
                }),
                returns: serde_json::json!({
                    "type": "object",
                    "properties": {
                        "formatted": { "type": "string" },
                        "db_path": { "type": "string" },
                        "table_count": { "type": "integer" },
                        "tables": { "type": "array" }
                    }
                }),
                cost_estimate: CostEstimate { latency_ms: 50, network_bound: false, token_cost: None },
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
            "db_query" => {
                let db_path: String = params.get("db_path").ok_or_else(|| {
                    SkillError::ToolFailed("Missing required parameter: db_path".to_string())
                })?;
                let query: String = params.get("query").ok_or_else(|| {
                    SkillError::ToolFailed("Missing required parameter: query".to_string())
                })?;
                let query_params: Vec<String> = params.get("params").unwrap_or_default();
                self.db_query(&db_path, &query, &query_params)
            }
            "db_execute" => {
                let db_path: String = params.get("db_path").ok_or_else(|| {
                    SkillError::ToolFailed("Missing required parameter: db_path".to_string())
                })?;
                let statement: String = params.get("statement").ok_or_else(|| {
                    SkillError::ToolFailed("Missing required parameter: statement".to_string())
                })?;
                let query_params: Vec<String> = params.get("params").unwrap_or_default();
                self.db_execute(&db_path, &statement, &query_params)
            }
            "db_schema" => {
                let db_path: String = params.get("db_path").ok_or_else(|| {
                    SkillError::ToolFailed("Missing required parameter: db_path".to_string())
                })?;
                self.db_schema(&db_path)
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

fn infer_scope(path: &Path) -> EntityScope {
    path.parent()
        .and_then(|parent| parent.file_name())
        .and_then(|name| name.to_str())
        .filter(|candidate| Uuid::parse_str(candidate).is_ok())
        .map(|id| EntityScope::Entity(id.to_string()))
        .unwrap_or(EntityScope::Hive)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn test_skill(root: PathBuf) -> DatabaseSkill {
        DatabaseSkill::new(DatabaseSkill::default_manifest(), vec![root])
    }

    #[test]
    fn test_manifest_parses() {
        let manifest = DatabaseSkill::default_manifest();
        assert_eq!(manifest.name, "Database");
    }

    #[test]
    fn test_drop_table_blocked() {
        let tmp = std::env::temp_dir().join(format!("abigail_db_test_{}", Uuid::new_v4()));
        std::fs::create_dir_all(&tmp).unwrap();
        let db_path = tmp.join("test.db");
        let skill = test_skill(tmp.clone());
        let result = skill.db_execute(&db_path.display().to_string(), "DROP TABLE users", &[]);
        assert!(result.is_err());
        let _ = std::fs::remove_dir_all(tmp);
    }
}
