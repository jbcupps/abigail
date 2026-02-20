//! Database skill: query, execute, and inspect SQLite databases within sandboxed directories.
//!
//! All database operations are restricted to allowed root directories to prevent
//! path traversal and unauthorized file access.

use abigail_skills::{
    CapabilityDescriptor, CostEstimate, ExecutionContext, FileSystemPermission, HealthStatus,
    Permission, Skill, SkillConfig, SkillError, SkillHealth, SkillManifest, SkillResult,
    ToolDescriptor, ToolOutput, ToolParams, TriggerDescriptor,
};
use async_trait::async_trait;
use std::any::Any;
use std::collections::HashMap;
use std::path::{Path, PathBuf};

/// Maximum number of rows returned by a single query.
const MAX_ROWS: usize = 1000;

/// Database skill with sandboxed directory access.
pub struct DatabaseSkill {
    manifest: SkillManifest,
    /// Root directories where database operations are allowed.
    allowed_roots: Vec<PathBuf>,
}

impl DatabaseSkill {
    /// Parse the embedded skill.toml manifest.
    pub fn default_manifest() -> SkillManifest {
        let toml_str = include_str!("../skill.toml");
        SkillManifest::parse(toml_str).expect("Failed to parse database skill.toml")
    }

    /// Create a new database skill with the given allowed root directories.
    pub fn new(manifest: SkillManifest, allowed_roots: Vec<PathBuf>) -> Self {
        Self {
            manifest,
            allowed_roots,
        }
    }

    /// Validate that a path is within one of the allowed roots.
    /// Returns the canonicalized path if valid, or an error.
    fn validate_path(&self, path_str: &str) -> SkillResult<PathBuf> {
        let path = PathBuf::from(path_str);

        // Reject obviously malicious patterns
        let normalized = path_str.replace('\\', "/");
        if normalized.contains("../") || normalized.contains("/..") {
            return Err(SkillError::PermissionDenied(
                "Path traversal (../) is not allowed".to_string(),
            ));
        }

        // For existing paths, canonicalize and check containment
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

        // For new paths (db_execute creating a new database), check parent exists and is allowed
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

    /// Strip the Windows extended-length path prefix (`\\?\`) if present.
    #[cfg(target_os = "windows")]
    fn strip_unc_prefix(p: &Path) -> PathBuf {
        let s = p.to_string_lossy();
        if let Some(stripped) = s.strip_prefix(r"\\?\") {
            PathBuf::from(stripped)
        } else {
            p.to_path_buf()
        }
    }

    #[cfg(not(target_os = "windows"))]
    fn strip_unc_prefix(p: &Path) -> PathBuf {
        p.to_path_buf()
    }

    /// Check if a canonicalized path is within any allowed root.
    fn is_within_allowed_roots(&self, canonical_path: &Path) -> bool {
        let canonical_path = Self::strip_unc_prefix(canonical_path);
        for root in &self.allowed_roots {
            let canonical_root = match root.canonicalize() {
                Ok(r) => Self::strip_unc_prefix(&r),
                Err(_) => root.clone(),
            };
            if canonical_path.starts_with(&canonical_root) {
                return true;
            }
        }
        false
    }

    /// Convert a rusqlite Value to a serde_json Value.
    fn sqlite_value_to_json(val: rusqlite::types::Value) -> serde_json::Value {
        match val {
            rusqlite::types::Value::Null => serde_json::Value::Null,
            rusqlite::types::Value::Integer(i) => serde_json::json!(i),
            rusqlite::types::Value::Real(f) => serde_json::json!(f),
            rusqlite::types::Value::Text(s) => serde_json::json!(s),
            rusqlite::types::Value::Blob(b) => {
                serde_json::json!(format!("<blob {} bytes>", b.len()))
            }
        }
    }

    /// Execute a read-only SELECT query against a database.
    fn db_query(&self, db_path: &str, query: &str, params: &[String]) -> SkillResult<ToolOutput> {
        let path = self.validate_path(db_path)?;

        if !path.is_file() {
            return Ok(ToolOutput::error(format!("'{}' is not a file", db_path)));
        }

        let conn = rusqlite::Connection::open_with_flags(
            &path,
            rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY | rusqlite::OpenFlags::SQLITE_OPEN_NO_MUTEX,
        )
        .map_err(|e| SkillError::ToolFailed(format!("Cannot open database: {}", e)))?;

        let mut stmt = conn
            .prepare(query)
            .map_err(|e| SkillError::ToolFailed(format!("Invalid query: {}", e)))?;

        let column_names: Vec<String> = stmt.column_names().iter().map(|s| s.to_string()).collect();

        let param_refs: Vec<&dyn rusqlite::types::ToSql> = params
            .iter()
            .map(|s| s as &dyn rusqlite::types::ToSql)
            .collect();

        let mut rows_out: Vec<serde_json::Value> = Vec::new();
        let mut rows = stmt
            .query(param_refs.as_slice())
            .map_err(|e| SkillError::ToolFailed(format!("Query execution failed: {}", e)))?;

        while let Some(row) = rows
            .next()
            .map_err(|e| SkillError::ToolFailed(format!("Error reading row: {}", e)))?
        {
            if rows_out.len() >= MAX_ROWS {
                break;
            }
            let mut row_map = serde_json::Map::new();
            for (i, col_name) in column_names.iter().enumerate() {
                let val: rusqlite::types::Value = row
                    .get(i)
                    .map_err(|e| SkillError::ToolFailed(format!("Error reading column: {}", e)))?;
                row_map.insert(col_name.clone(), Self::sqlite_value_to_json(val));
            }
            rows_out.push(serde_json::Value::Object(row_map));
        }

        let row_count = rows_out.len();
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
            "rows": rows_out,
            "row_count": row_count,
            "truncated": row_count >= MAX_ROWS,
        })))
    }

    /// Execute a write statement (INSERT, UPDATE, DELETE) against a database.
    fn db_execute(
        &self,
        db_path: &str,
        statement: &str,
        params: &[String],
    ) -> SkillResult<ToolOutput> {
        // Safety: block DROP TABLE / DROP DATABASE
        let upper = statement.to_uppercase();
        if upper.contains("DROP TABLE") || upper.contains("DROP DATABASE") {
            return Err(SkillError::PermissionDenied(
                "DROP TABLE and DROP DATABASE statements are not allowed".to_string(),
            ));
        }

        let path = self.validate_path(db_path)?;

        let conn = rusqlite::Connection::open_with_flags(
            &path,
            rusqlite::OpenFlags::SQLITE_OPEN_READ_WRITE
                | rusqlite::OpenFlags::SQLITE_OPEN_CREATE
                | rusqlite::OpenFlags::SQLITE_OPEN_NO_MUTEX,
        )
        .map_err(|e| SkillError::ToolFailed(format!("Cannot open database: {}", e)))?;

        let param_refs: Vec<&dyn rusqlite::types::ToSql> = params
            .iter()
            .map(|s| s as &dyn rusqlite::types::ToSql)
            .collect();

        let rows_affected = conn
            .execute(statement, param_refs.as_slice())
            .map_err(|e| SkillError::ToolFailed(format!("Statement execution failed: {}", e)))?;

        let formatted = format!(
            "Statement executed successfully. {} row{} affected.",
            rows_affected,
            if rows_affected == 1 { "" } else { "s" }
        );

        tracing::info!(
            "Executed statement on {}: {} rows affected",
            path.display(),
            rows_affected
        );

        Ok(ToolOutput::success(serde_json::json!({
            "formatted": formatted,
            "rows_affected": rows_affected,
            "db_path": path.display().to_string(),
        })))
    }

    /// Inspect the schema of a database: list all tables and their columns.
    fn db_schema(&self, db_path: &str) -> SkillResult<ToolOutput> {
        let path = self.validate_path(db_path)?;

        if !path.is_file() {
            return Ok(ToolOutput::error(format!("'{}' is not a file", db_path)));
        }

        let conn = rusqlite::Connection::open_with_flags(
            &path,
            rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY | rusqlite::OpenFlags::SQLITE_OPEN_NO_MUTEX,
        )
        .map_err(|e| SkillError::ToolFailed(format!("Cannot open database: {}", e)))?;

        // Get all table names from sqlite_master
        let mut table_stmt = conn
            .prepare("SELECT name FROM sqlite_master WHERE type='table' ORDER BY name")
            .map_err(|e| SkillError::ToolFailed(format!("Cannot query sqlite_master: {}", e)))?;

        let table_names: Vec<String> = table_stmt
            .query_map([], |row| row.get(0))
            .map_err(|e| SkillError::ToolFailed(format!("Error reading tables: {}", e)))?
            .filter_map(|r| r.ok())
            .collect();

        let mut tables: Vec<serde_json::Value> = Vec::new();
        let mut formatted_lines: Vec<String> = Vec::new();

        for table_name in &table_names {
            // PRAGMA table_info returns: cid, name, type, notnull, dflt_value, pk
            let pragma_query = format!("PRAGMA table_info(\"{}\")", table_name);
            let mut col_stmt = conn
                .prepare(&pragma_query)
                .map_err(|e| SkillError::ToolFailed(format!("Cannot get table info: {}", e)))?;

            let columns: Vec<serde_json::Value> = col_stmt
                .query_map([], |row| {
                    let name: String = row.get(1)?;
                    let col_type: String = row.get(2)?;
                    let notnull: bool = row.get(3)?;
                    let default: rusqlite::types::Value = row.get(4)?;
                    let pk: bool = row.get(5)?;
                    Ok(serde_json::json!({
                        "name": name,
                        "type": col_type,
                        "not_null": notnull,
                        "default": Self::sqlite_value_to_json(default),
                        "primary_key": pk,
                    }))
                })
                .map_err(|e| SkillError::ToolFailed(format!("Error reading columns: {}", e)))?
                .filter_map(|r| r.ok())
                .collect();

            // Build formatted output
            formatted_lines.push(format!("Table: {}", table_name));
            for col in &columns {
                let name = col["name"].as_str().unwrap_or("?");
                let col_type = col["type"].as_str().unwrap_or("?");
                let pk = if col["primary_key"].as_bool().unwrap_or(false) {
                    " [PK]"
                } else {
                    ""
                };
                let nn = if col["not_null"].as_bool().unwrap_or(false) {
                    " NOT NULL"
                } else {
                    ""
                };
                formatted_lines.push(format!("  {} {}{}{}", name, col_type, nn, pk));
            }
            formatted_lines.push(String::new());

            tables.push(serde_json::json!({
                "name": table_name,
                "columns": columns,
            }));
        }

        let formatted = if tables.is_empty() {
            "Database has no tables.".to_string()
        } else {
            formatted_lines.join("\n")
        };

        Ok(ToolOutput::success(serde_json::json!({
            "formatted": formatted,
            "db_path": path.display().to_string(),
            "table_count": tables.len(),
            "tables": tables,
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
        let all_accessible = self.allowed_roots.iter().all(|r| r.exists());
        SkillHealth {
            status: if all_accessible {
                HealthStatus::Healthy
            } else {
                HealthStatus::Degraded
            },
            message: if !all_accessible {
                Some("Some allowed root directories are not accessible".to_string())
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
                name: "db_query".to_string(),
                description: "Execute a read-only SELECT query against a SQLite database. Returns up to 1000 rows as a JSON array.".to_string(),
                parameters: serde_json::json!({
                    "type": "object",
                    "properties": {
                        "db_path": {
                            "type": "string",
                            "description": "Absolute path to the SQLite database file"
                        },
                        "query": {
                            "type": "string",
                            "description": "SQL SELECT query to execute"
                        },
                        "params": {
                            "type": "array",
                            "items": { "type": "string" },
                            "description": "Optional positional parameters for the query"
                        }
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
                cost_estimate: CostEstimate {
                    latency_ms: 50,
                    network_bound: false,
                    token_cost: None,
                },
                required_permissions: vec![Permission::FileSystem(
                    FileSystemPermission::Read(vec!["~".to_string()]),
                )],
                autonomous: true,
                requires_confirmation: false,
            },
            ToolDescriptor {
                name: "db_execute".to_string(),
                description: "Execute a write statement (INSERT, UPDATE, DELETE, CREATE TABLE) against a SQLite database. DROP TABLE and DROP DATABASE are blocked for safety.".to_string(),
                parameters: serde_json::json!({
                    "type": "object",
                    "properties": {
                        "db_path": {
                            "type": "string",
                            "description": "Absolute path to the SQLite database file"
                        },
                        "statement": {
                            "type": "string",
                            "description": "SQL statement to execute (INSERT, UPDATE, DELETE, CREATE TABLE)"
                        },
                        "params": {
                            "type": "array",
                            "items": { "type": "string" },
                            "description": "Optional positional parameters for the statement"
                        }
                    },
                    "required": ["db_path", "statement"]
                }),
                returns: serde_json::json!({
                    "type": "object",
                    "properties": {
                        "formatted": { "type": "string" },
                        "rows_affected": { "type": "integer" },
                        "db_path": { "type": "string" }
                    }
                }),
                cost_estimate: CostEstimate {
                    latency_ms: 50,
                    network_bound: false,
                    token_cost: None,
                },
                required_permissions: vec![
                    Permission::FileSystem(
                        FileSystemPermission::Read(vec!["~".to_string()]),
                    ),
                    Permission::FileSystem(
                        FileSystemPermission::Write(vec!["~".to_string()]),
                    ),
                ],
                autonomous: false,
                requires_confirmation: true,
            },
            ToolDescriptor {
                name: "db_schema".to_string(),
                description: "Inspect the schema of a SQLite database: list all tables and their columns with types, constraints, and primary keys.".to_string(),
                parameters: serde_json::json!({
                    "type": "object",
                    "properties": {
                        "db_path": {
                            "type": "string",
                            "description": "Absolute path to the SQLite database file"
                        }
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
                cost_estimate: CostEstimate {
                    latency_ms: 50,
                    network_bound: false,
                    token_cost: None,
                },
                required_permissions: vec![Permission::FileSystem(
                    FileSystemPermission::Read(vec!["~".to_string()]),
                )],
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

#[cfg(test)]
mod tests {
    use super::*;

    fn test_skill(roots: Vec<PathBuf>) -> DatabaseSkill {
        DatabaseSkill::new(DatabaseSkill::default_manifest(), roots)
    }

    #[test]
    fn test_manifest_parses() {
        let manifest = DatabaseSkill::default_manifest();
        assert_eq!(manifest.name, "Database");
    }

    #[test]
    fn test_tools_list() {
        let skill = test_skill(vec![]);
        let tools = skill.tools();
        assert_eq!(tools.len(), 3);
        let names: Vec<&str> = tools.iter().map(|t| t.name.as_str()).collect();
        assert!(names.contains(&"db_query"));
        assert!(names.contains(&"db_execute"));
        assert!(names.contains(&"db_schema"));
    }

    #[test]
    fn test_db_execute_requires_confirmation() {
        let skill = test_skill(vec![]);
        let tools = skill.tools();
        let execute_tool = tools.iter().find(|t| t.name == "db_execute").unwrap();
        assert!(execute_tool.requires_confirmation);
        assert!(!execute_tool.autonomous);
    }

    #[test]
    fn test_db_query_is_autonomous() {
        let skill = test_skill(vec![]);
        let tools = skill.tools();
        let query_tool = tools.iter().find(|t| t.name == "db_query").unwrap();
        assert!(query_tool.autonomous);
        assert!(!query_tool.requires_confirmation);
    }

    #[test]
    fn test_db_schema_is_autonomous() {
        let skill = test_skill(vec![]);
        let tools = skill.tools();
        let schema_tool = tools.iter().find(|t| t.name == "db_schema").unwrap();
        assert!(schema_tool.autonomous);
        assert!(!schema_tool.requires_confirmation);
    }

    #[test]
    fn test_path_traversal_blocked() {
        let tmp = std::env::temp_dir().join("abigail_db_test_traversal");
        let _ = std::fs::remove_dir_all(&tmp);
        std::fs::create_dir_all(&tmp).unwrap();

        let skill = test_skill(vec![tmp.clone()]);

        let result = skill.validate_path("../../etc/passwd");
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("traversal"));

        let _ = std::fs::remove_dir_all(&tmp);
    }

    #[test]
    fn test_drop_table_blocked() {
        let tmp = std::env::temp_dir().join("abigail_db_test_drop");
        let _ = std::fs::remove_dir_all(&tmp);
        std::fs::create_dir_all(&tmp).unwrap();

        let db_path = tmp.join("test.db");
        // Create a database with a table
        {
            let conn = rusqlite::Connection::open(&db_path).unwrap();
            conn.execute("CREATE TABLE users (id INTEGER PRIMARY KEY, name TEXT)", [])
                .unwrap();
        }

        let skill = test_skill(vec![tmp.clone()]);

        let result = skill.db_execute(&db_path.display().to_string(), "DROP TABLE users", &[]);
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("DROP TABLE"));

        let _ = std::fs::remove_dir_all(&tmp);
    }

    #[test]
    fn test_query_and_schema() {
        let tmp = std::env::temp_dir().join("abigail_db_test_query");
        let _ = std::fs::remove_dir_all(&tmp);
        std::fs::create_dir_all(&tmp).unwrap();

        let db_path = tmp.join("test.db");
        // Create and populate a database
        {
            let conn = rusqlite::Connection::open(&db_path).unwrap();
            conn.execute(
                "CREATE TABLE items (id INTEGER PRIMARY KEY, name TEXT NOT NULL, value REAL)",
                [],
            )
            .unwrap();
            conn.execute(
                "INSERT INTO items (name, value) VALUES (?1, ?2)",
                rusqlite::params!["alpha", 1.5],
            )
            .unwrap();
            conn.execute(
                "INSERT INTO items (name, value) VALUES (?1, ?2)",
                rusqlite::params!["beta", 2.5],
            )
            .unwrap();
        }

        let skill = test_skill(vec![tmp.clone()]);

        // Test db_query
        let result = skill
            .db_query(&db_path.display().to_string(), "SELECT * FROM items", &[])
            .unwrap();
        assert!(result.success);
        let data = result.data.unwrap();
        assert_eq!(data["row_count"], 2);

        // Test db_schema
        let result = skill.db_schema(&db_path.display().to_string()).unwrap();
        assert!(result.success);
        let data = result.data.unwrap();
        assert_eq!(data["table_count"], 1);
        let tables = data["tables"].as_array().unwrap();
        assert_eq!(tables[0]["name"], "items");

        let _ = std::fs::remove_dir_all(&tmp);
    }

    #[test]
    fn test_health_check() {
        let tmp = std::env::temp_dir().join("abigail_db_test_health");
        let _ = std::fs::remove_dir_all(&tmp);
        std::fs::create_dir_all(&tmp).unwrap();

        let skill = test_skill(vec![tmp.clone()]);
        let health = skill.health();
        assert_eq!(health.status, HealthStatus::Healthy);

        let _ = std::fs::remove_dir_all(&tmp);
    }
}
