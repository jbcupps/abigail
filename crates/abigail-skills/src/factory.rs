use crate::channel::TriggerDescriptor;
use crate::dynamic::{DynamicApiSkill, DynamicSkillConfig, DynamicToolConfig};
use crate::manifest::{FileSystemPermission, Permission, SkillId, SkillManifest};
use crate::skill::{
    CostEstimate, ExecutionContext, HealthStatus, Skill, SkillConfig, SkillError, SkillHealth,
    SkillResult, ToolDescriptor, ToolOutput, ToolParams,
};
use crate::SkillRegistry;
use abigail_core::SecretsVault;
use async_trait::async_trait;
use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};

/// A secret required by an authored skill, parsed from the `required_secrets` JSON parameter.
#[derive(serde::Deserialize)]
struct SecretEntry {
    name: String,
    #[serde(default)]
    description: String,
}

pub struct SkillFactory {
    manifest: SkillManifest,
    skills_dir: PathBuf,
    registry: Option<Arc<SkillRegistry>>,
    secrets: Option<Arc<Mutex<SecretsVault>>>,
}

impl SkillFactory {
    pub fn new(skills_dir: PathBuf) -> Self {
        let manifest = SkillManifest {
            id: SkillId("builtin.skill_factory".to_string()),
            name: "Skill Factory".to_string(),
            version: "0.1.0".to_string(),
            description: "Allows the Sovereign Entity to autonomously create, update, and manage its own skills.".to_string(),
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
            registry: None,
            secrets: None,
        }
    }

    /// Attach a live registry so newly created skills are immediately registered.
    pub fn with_registry(mut self, registry: Arc<SkillRegistry>) -> Self {
        self.registry = Some(registry);
        self
    }

    /// Attach a secrets vault for newly created dynamic API skills.
    pub fn with_secrets(mut self, secrets: Arc<Mutex<SecretsVault>>) -> Self {
        self.secrets = Some(secrets);
        self
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
                description: "Create a new permanent skill. Use format='dynamic_api' for HTTP-based skills (immediately usable) or format='script' for code-based skills.".to_string(),
                parameters: serde_json::json!({
                    "type": "object",
                    "properties": {
                        "id": { "type": "string", "description": "The skill ID (e.g., 'custom.my_tool' or 'dynamic.my_api')" },
                        "name": { "type": "string", "description": "Human-readable name" },
                        "description": { "type": "string" },
                        "format": { "type": "string", "enum": ["dynamic_api", "script"], "description": "Skill type. 'dynamic_api' creates an HTTP-based skill (default, immediately available). 'script' creates a code-based skill." },
                        "tools_json": { "type": "string", "description": "For dynamic_api format: JSON array of tool configs, each with name, description, parameters, method, url_template, headers, body_template, response_extract" },
                        "script_content": { "type": "string", "description": "For script format: the code (Python or Node.js preferred)" },
                        "script_filename": { "type": "string", "description": "For script format: e.g., 'main.py' or 'index.js'" },
                        "how_to_use_md": { "type": "string", "description": "Instructional legacy for future Egos" },
                        "required_secrets": { "type": "string", "description": "JSON array of secrets this skill needs, each with 'name' and 'description'. Example: [{\"name\":\"api_key\",\"description\":\"API key for the service\"}]" }
                    },
                    "required": ["id", "name", "how_to_use_md"]
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
                let how_to: String = params
                    .get("how_to_use_md")
                    .ok_or_else(|| SkillError::ToolFailed("Missing 'how_to_use_md'".to_string()))?;

                let format: String = params.get("format").unwrap_or_else(|| "dynamic_api".into());

                // Parse optional required_secrets JSON array
                let secrets: Vec<SecretEntry> =
                    if let Some(raw) = params.get::<String>("required_secrets") {
                        serde_json::from_str(&raw).map_err(|e| {
                            SkillError::ToolFailed(format!("Invalid required_secrets JSON: {}", e))
                        })?
                    } else {
                        vec![]
                    };

                let skill_dir = self.skills_dir.join(&id);
                std::fs::create_dir_all(&skill_dir)
                    .map_err(|e| SkillError::ToolFailed(e.to_string()))?;

                // Write how-to-use.md (common to both formats)
                std::fs::write(skill_dir.join("how-to-use.md"), &how_to)
                    .map_err(|e| SkillError::ToolFailed(e.to_string()))?;

                if format == "dynamic_api" {
                    self.author_dynamic_api(&id, &name, &desc, &skill_dir, &params, &secrets)?;
                } else {
                    self.author_script(&id, &name, &desc, &skill_dir, &params, &secrets)?;
                }

                Ok(ToolOutput::success(serde_json::json!({
                    "created": true,
                    "id": id,
                    "format": format,
                    "registered": self.registry.is_some()
                })))
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

impl SkillFactory {
    /// Format `[[secrets]]` TOML blocks from parsed secret entries.
    fn format_secrets_toml(secrets: &[SecretEntry]) -> String {
        let mut out = String::new();
        for s in secrets {
            out.push_str(&format!(
                "\n[[secrets]]\nname = \"{}\"\ndescription = \"{}\"\nrequired = true\n",
                s.name,
                s.description.replace('"', "\\\""),
            ));
        }
        out
    }

    fn author_dynamic_api(
        &self,
        id: &str,
        name: &str,
        desc: &str,
        skill_dir: &std::path::Path,
        params: &ToolParams,
        secrets: &[SecretEntry],
    ) -> SkillResult<()> {
        let tools_json_str: String = params.get("tools_json").unwrap_or_else(|| {
            serde_json::json!([{
                "name": "execute",
                "description": format!("Execute {}", name),
                "parameters": { "type": "object", "properties": {}, "required": [] },
                "method": "GET",
                "url_template": "https://example.com",
                "headers": {},
                "response_extract": {}
            }])
            .to_string()
        });

        let tools: Vec<DynamicToolConfig> = serde_json::from_str(&tools_json_str)
            .map_err(|e| SkillError::ToolFailed(format!("Invalid tools_json: {}", e)))?;

        if tools.len() > 10 {
            return Err(SkillError::ToolFailed(
                "Maximum 10 tools per dynamic skill".into(),
            ));
        }

        let config = DynamicSkillConfig {
            id: id.to_string(),
            name: name.to_string(),
            description: desc.to_string(),
            version: "0.1.0".to_string(),
            category: "Custom".to_string(),
            created_at: chrono::Utc::now().to_rfc3339(),
            tools,
        };

        let json_path = skill_dir.join(format!("{}.json", id.replace('.', "_")));
        let json = serde_json::to_string_pretty(&config)
            .map_err(|e| SkillError::ToolFailed(e.to_string()))?;
        std::fs::write(&json_path, json).map_err(|e| SkillError::ToolFailed(e.to_string()))?;

        // Write skill.toml with secrets so namespace validation can find them
        if !secrets.is_empty() {
            let manifest_toml = format!(
                r#"[skill]
id = "{id}"
name = "{name}"
version = "0.1.0"
description = "{desc}"
runtime = "Native"
category = "Custom"
{secrets_block}"#,
                secrets_block = Self::format_secrets_toml(secrets)
            );
            std::fs::write(skill_dir.join("skill.toml"), manifest_toml)
                .map_err(|e| SkillError::ToolFailed(e.to_string()))?;

            // The new skill.toml is now on disk. validate_secret_namespace_with()
            // calls SkillRegistry::discover() which re-scans the filesystem, so
            // the declared secrets will be visible on the next store_secret call.
        }

        // Immediately register the skill if we have a registry.
        if let Some(ref registry) = self.registry {
            match DynamicApiSkill::load_from_path(&json_path, self.secrets.clone()) {
                Ok(skill) => {
                    let skill_id = SkillId(id.to_string());
                    let _ = registry.register(skill_id, Arc::new(skill));
                    tracing::info!("SkillFactory: auto-registered dynamic skill {}", id);
                }
                Err(e) => {
                    tracing::warn!("SkillFactory: created skill but failed to register: {}", e);
                }
            }
        }

        Ok(())
    }

    fn author_script(
        &self,
        id: &str,
        name: &str,
        desc: &str,
        skill_dir: &std::path::Path,
        params: &ToolParams,
        secrets: &[SecretEntry],
    ) -> SkillResult<()> {
        let content: String = params.get("script_content").ok_or_else(|| {
            SkillError::ToolFailed("Missing 'script_content' for script format".into())
        })?;
        let filename: String = params.get("script_filename").ok_or_else(|| {
            SkillError::ToolFailed("Missing 'script_filename' for script format".into())
        })?;

        let manifest = format!(
            r#"[skill]
id = "{id}"
name = "{name}"
version = "0.1.0"
description = "{desc}"
runtime = "Native"
category = "Custom"
{secrets_block}"#,
            secrets_block = Self::format_secrets_toml(secrets)
        );

        std::fs::write(skill_dir.join("skill.toml"), manifest)
            .map_err(|e| SkillError::ToolFailed(e.to_string()))?;
        std::fs::write(skill_dir.join(filename), content)
            .map_err(|e| SkillError::ToolFailed(e.to_string()))?;

        Ok(())
    }
}
