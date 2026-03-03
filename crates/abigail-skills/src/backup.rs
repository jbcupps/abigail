use crate::channel::TriggerDescriptor;
use crate::manifest::CapabilityDescriptor;
use crate::manifest::{SkillId, SkillManifest};
use crate::skill::{
    CostEstimate, ExecutionContext, HealthStatus, Skill, SkillConfig, SkillError, SkillHealth,
    SkillResult, ToolDescriptor, ToolOutput, ToolParams,
};
use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::Arc;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BackupInfo {
    pub path: String,
    pub timestamp: String,
    pub has_memory_db: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BackupPreview {
    pub db_path: String,
    pub turn_count: u64,
    pub memory_count: u64,
    pub session_count: u64,
    pub earliest: Option<String>,
    pub latest: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BackupImportResult {
    pub turns_imported: u64,
    pub turns_skipped: u64,
    pub memories_imported: u64,
    pub memories_skipped: u64,
}

#[async_trait]
pub trait BackupOperations: Send + Sync {
    async fn list_backups(&self) -> Result<Vec<BackupInfo>, String>;
    async fn preview_backup(&self, backup_path: &str) -> Result<BackupPreview, String>;
    async fn import_backup(&self, backup_path: &str) -> Result<BackupImportResult, String>;
}

pub struct BackupManagementSkill {
    manifest: SkillManifest,
    ops: Arc<dyn BackupOperations>,
}

impl BackupManagementSkill {
    pub fn new(ops: Arc<dyn BackupOperations>) -> Self {
        let manifest = SkillManifest {
            id: SkillId("builtin.backup_management".to_string()),
            name: "Backup Management".to_string(),
            version: "0.1.0".to_string(),
            description:
                "List, preview, and import conversation history and memories from backups."
                    .to_string(),
            license: Some("MIT".to_string()),
            category: "System".to_string(),
            keywords: vec![
                "backup".to_string(),
                "restore".to_string(),
                "import".to_string(),
                "memory".to_string(),
            ],
            runtime: "Native".to_string(),
            min_abigail_version: "0.1.0".to_string(),
            platforms: vec!["All".to_string()],
            capabilities: vec![],
            permissions: vec![],
            secrets: vec![],
            config_defaults: HashMap::new(),
        };

        Self { manifest, ops }
    }
}

#[async_trait]
impl Skill for BackupManagementSkill {
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
        SkillHealth {
            status: HealthStatus::Healthy,
            message: None,
            last_check: chrono::Utc::now(),
            metrics: HashMap::new(),
        }
    }

    fn tools(&self) -> Vec<ToolDescriptor> {
        vec![
            ToolDescriptor {
                name: "list_backups".to_string(),
                description: "List available backup directories that contain importable memory databases. Returns paths, timestamps, and whether each backup has a usable SQLite DB.".to_string(),
                parameters: serde_json::json!({"type": "object", "properties": {}}),
                returns: serde_json::json!({
                    "type": "array",
                    "items": {
                        "type": "object",
                        "properties": {
                            "path": { "type": "string" },
                            "timestamp": { "type": "string" },
                            "has_memory_db": { "type": "boolean" }
                        }
                    }
                }),
                cost_estimate: CostEstimate::default(),
                required_permissions: vec![],
                autonomous: true,
                requires_confirmation: false,
            },
            ToolDescriptor {
                name: "preview_backup".to_string(),
                description: "Preview the contents of a backup database: count of conversation turns, memories, sessions, and date range. Does NOT modify any data.".to_string(),
                parameters: serde_json::json!({
                    "type": "object",
                    "properties": {
                        "backup_path": {
                            "type": "string",
                            "description": "Path to the backup directory (from list_backups)"
                        }
                    },
                    "required": ["backup_path"]
                }),
                returns: serde_json::json!({
                    "type": "object",
                    "properties": {
                        "db_path": { "type": "string" },
                        "turn_count": { "type": "integer" },
                        "memory_count": { "type": "integer" },
                        "session_count": { "type": "integer" },
                        "earliest": { "type": "string" },
                        "latest": { "type": "string" }
                    }
                }),
                cost_estimate: CostEstimate::default(),
                required_permissions: vec![],
                autonomous: true,
                requires_confirmation: false,
            },
            ToolDescriptor {
                name: "import_backup".to_string(),
                description: "Import conversation turns and memories from a backup into the current entity's memory store. Uses INSERT OR IGNORE for idempotent operation — safe to run multiple times.".to_string(),
                parameters: serde_json::json!({
                    "type": "object",
                    "properties": {
                        "backup_path": {
                            "type": "string",
                            "description": "Path to the backup directory (from list_backups)"
                        }
                    },
                    "required": ["backup_path"]
                }),
                returns: serde_json::json!({
                    "type": "object",
                    "properties": {
                        "turns_imported": { "type": "integer" },
                        "turns_skipped": { "type": "integer" },
                        "memories_imported": { "type": "integer" },
                        "memories_skipped": { "type": "integer" }
                    }
                }),
                cost_estimate: CostEstimate::default(),
                required_permissions: vec![],
                autonomous: true,
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
            "list_backups" => {
                let backups = self
                    .ops
                    .list_backups()
                    .await
                    .map_err(SkillError::ToolFailed)?;
                Ok(ToolOutput::success(
                    serde_json::to_value(backups).unwrap(),
                ))
            }
            "preview_backup" => {
                let path: String = params
                    .get("backup_path")
                    .ok_or_else(|| SkillError::ToolFailed("Missing 'backup_path'".to_string()))?;
                let preview = self
                    .ops
                    .preview_backup(&path)
                    .await
                    .map_err(SkillError::ToolFailed)?;
                Ok(ToolOutput::success(
                    serde_json::to_value(preview).unwrap(),
                ))
            }
            "import_backup" => {
                let path: String = params
                    .get("backup_path")
                    .ok_or_else(|| SkillError::ToolFailed("Missing 'backup_path'".to_string()))?;
                let result = self
                    .ops
                    .import_backup(&path)
                    .await
                    .map_err(SkillError::ToolFailed)?;
                Ok(ToolOutput::success(
                    serde_json::to_value(result).unwrap(),
                ))
            }
            _ => Err(SkillError::ToolFailed(format!(
                "Unknown tool: {}",
                tool_name
            ))),
        }
    }

    fn capabilities(&self) -> Vec<CapabilityDescriptor> {
        vec![]
    }

    fn get_capability(&self, _cap_type: &str) -> Option<&dyn std::any::Any> {
        None
    }

    fn triggers(&self) -> Vec<TriggerDescriptor> {
        vec![]
    }
}
