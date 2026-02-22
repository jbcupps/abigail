use crate::channel::TriggerDescriptor;
use crate::manifest::{FileSystemPermission, Permission, SkillId, SkillManifest};
use crate::skill::{
    CostEstimate, ExecutionContext, HealthStatus, Skill, SkillConfig, SkillError, SkillHealth,
    SkillResult, ToolDescriptor, ToolOutput, ToolParams,
};
use async_trait::async_trait;
use std::collections::HashMap;
use std::path::PathBuf;

pub struct SkillFactory {
    manifest: SkillManifest,
    skills_dir: PathBuf,
}

impl SkillFactory {
    pub fn new(skills_dir: PathBuf) -> Self {
        let manifest = SkillManifest {
            id: SkillId("builtin.skill_factory".to_string()),
            name: "Skill Factory".to_string(),
            version: "0.1.0".to_string(),
            description: "Allows the Entity to autonomously create, update, and manage its own skills.".to_string(),
            license: Some("MIT".to_string()),
            category: "System".to_string(),
            keywords: vec!["factory".to_string(), "meta".to_string(), "code".to_string()],
            runtime: "Native".to_string(),
            min_abigail_version: "0.1.0".to_string(),
            platforms: vec!["All".to_string()],
            capabilities: vec![],
            permissions: vec![Permission::FileSystem(FileSystemPermission::Full)],
            secrets: vec![],
            config_defaults: HashMap::new(),
        };

        Self {
            manifest,
            skills_dir,
        }
    }
}

#[async_trait]
impl Skill for SkillFactory {
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
                name: "author_skill".to_string(),
                description: "Create a new permanent skill by writing a skill.toml and script file. Use this for repeatable tasks.".to_string(),
                parameters: serde_json::json!({
                    "type": "object",
                    "properties": {
                        "id": { "type": "string", "description": "The skill ID (e.g., 'custom.my_tool')" },
                        "name": { "type": "string", "description": "Human-readable name" },
                        "description": { "type": "string" },
                        "script_content": { "type": "string", "description": "The code for the skill (Python or Node.js preferred)" },
                        "script_filename": { "type": "string", "description": "e.g., 'main.py' or 'index.js'" },
                        "how_to_use_md": { "type": "string", "description": "Instructional legacy for future Egos" }
                    },
                    "required": ["id", "name", "script_content", "script_filename", "how_to_use_md"]
                }),
                returns: serde_json::json!({ "type": "boolean" }),
                cost_estimate: CostEstimate::default(),
                required_permissions: vec![Permission::FileSystem(FileSystemPermission::Full)],
                autonomous: true,
                requires_confirmation: true,
            },
            ToolDescriptor {
                name: "delete_skill".to_string(),
                description: "Remove an obsolete or unused skill from the Hive.".to_string(),
                parameters: serde_json::json!({
                    "type": "object",
                    "properties": {
                        "id": { "type": "string", "description": "The ID of the skill to delete" }
                    },
                    "required": ["id"]
                }),
                returns: serde_json::json!({ "type": "boolean" }),
                cost_estimate: CostEstimate::default(),
                required_permissions: vec![Permission::FileSystem(FileSystemPermission::Full)],
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
            "author_skill" => {
                let id: String = params
                    .get("id")
                    .ok_or_else(|| SkillError::ToolFailed("Missing 'id'".to_string()))?;
                let name: String = params
                    .get("name")
                    .ok_or_else(|| SkillError::ToolFailed("Missing 'name'".to_string()))?;
                let desc: String = params.get("description").unwrap_or_default();
                let content: String = params.get("script_content").ok_or_else(|| {
                    SkillError::ToolFailed("Missing 'script_content'".to_string())
                })?;
                let filename: String = params.get("script_filename").ok_or_else(|| {
                    SkillError::ToolFailed("Missing 'script_filename'".to_string())
                })?;
                let how_to: String = params
                    .get("how_to_use_md")
                    .ok_or_else(|| SkillError::ToolFailed("Missing 'how_to_use_md'".to_string()))?;

                let skill_dir = self.skills_dir.join(&id);
                std::fs::create_dir_all(&skill_dir)
                    .map_err(|e| SkillError::ToolFailed(e.to_string()))?;

                // 1. Write skill.toml
                let manifest = format!(
                    r#"[skill]
id = "{}"
name = "{}"
version = "0.1.0"
description = "{}"
runtime = "Native"
category = "Custom"

[[tools]]
name = "execute"
description = "Execute the custom logic for {}"
parameters = {{ "type" = "object", "properties" = {{}}, "required" = [] }}
"#,
                    id, name, desc, name
                );

                std::fs::write(skill_dir.join("skill.toml"), manifest)
                    .map_err(|e| SkillError::ToolFailed(e.to_string()))?;

                // 2. Write script
                std::fs::write(skill_dir.join(filename), content)
                    .map_err(|e| SkillError::ToolFailed(e.to_string()))?;

                // 3. Write how-to-use.md
                std::fs::write(skill_dir.join("how-to-use.md"), how_to)
                    .map_err(|e| SkillError::ToolFailed(e.to_string()))?;

                Ok(ToolOutput::success(serde_json::json!(true)))
            }
            "delete_skill" => {
                let id: String = params
                    .get("id")
                    .ok_or_else(|| SkillError::ToolFailed("Missing 'id'".to_string()))?;
                let skill_dir = self.skills_dir.join(&id);
                if skill_dir.exists() && skill_dir.is_dir() {
                    std::fs::remove_dir_all(skill_dir)
                        .map_err(|e| SkillError::ToolFailed(e.to_string()))?;
                    Ok(ToolOutput::success(serde_json::json!(true)))
                } else {
                    Err(SkillError::ToolFailed(
                        "Skill not found or not a directory".to_string(),
                    ))
                }
            }
            _ => Err(SkillError::ToolFailed(format!(
                "Unknown tool: {}",
                tool_name
            ))),
        }
    }

    fn capabilities(&self) -> Vec<crate::manifest::CapabilityDescriptor> {
        vec![]
    }

    fn get_capability(&self, _cap_type: &str) -> Option<&dyn std::any::Any> {
        None
    }

    fn triggers(&self) -> Vec<TriggerDescriptor> {
        vec![]
    }
}
