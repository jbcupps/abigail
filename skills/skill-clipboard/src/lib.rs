//! Clipboard skill: read from and write to the system clipboard.
//!
//! Provides two tools:
//! - `clipboard_read`: reads the current text content from the clipboard
//! - `clipboard_write`: writes text content to the clipboard (requires confirmation)

use abigail_skills::{
    CapabilityDescriptor, CostEstimate, ExecutionContext, HealthStatus, Permission, Skill,
    SkillConfig, SkillError, SkillHealth, SkillManifest, SkillResult, ToolDescriptor, ToolOutput,
    ToolParams, TriggerDescriptor,
};
use async_trait::async_trait;
use std::any::Any;
use std::collections::HashMap;

/// Clipboard skill with read and write tools.
pub struct ClipboardSkill {
    manifest: SkillManifest,
}

impl ClipboardSkill {
    /// Parse the embedded skill.toml manifest.
    pub fn default_manifest() -> SkillManifest {
        let toml_str = include_str!("../skill.toml");
        SkillManifest::parse(toml_str).expect("Failed to parse clipboard skill.toml")
    }

    /// Create a new clipboard skill.
    pub fn new(manifest: SkillManifest) -> Self {
        Self { manifest }
    }

    /// Read text from the system clipboard.
    fn read_clipboard(&self) -> SkillResult<ToolOutput> {
        let mut clipboard = arboard::Clipboard::new()
            .map_err(|e| SkillError::ToolFailed(format!("Failed to access clipboard: {}", e)))?;

        match clipboard.get_text() {
            Ok(text) => Ok(ToolOutput::success(serde_json::json!({
                "text": text,
            }))),
            Err(e) => Ok(ToolOutput::error(format!(
                "Failed to read clipboard text: {}. The clipboard may be empty or contain non-text data.",
                e
            ))),
        }
    }

    /// Write text to the system clipboard.
    fn write_clipboard(&self, text: &str) -> SkillResult<ToolOutput> {
        let mut clipboard = arboard::Clipboard::new()
            .map_err(|e| SkillError::ToolFailed(format!("Failed to access clipboard: {}", e)))?;

        match clipboard.set_text(text) {
            Ok(()) => Ok(ToolOutput::success(serde_json::json!({
                "written": true,
                "length": text.len(),
            }))),
            Err(e) => Ok(ToolOutput::error(format!(
                "Failed to write to clipboard: {}",
                e
            ))),
        }
    }
}

#[async_trait]
impl Skill for ClipboardSkill {
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
                name: "clipboard_read".to_string(),
                description: "Read the current text content from the system clipboard.".to_string(),
                parameters: serde_json::json!({
                    "type": "object",
                    "properties": {},
                    "required": []
                }),
                returns: serde_json::json!({
                    "type": "object",
                    "properties": {
                        "text": { "type": "string" }
                    }
                }),
                cost_estimate: CostEstimate {
                    latency_ms: 50,
                    network_bound: false,
                    token_cost: None,
                },
                required_permissions: vec![Permission::Clipboard],
                autonomous: true,
                requires_confirmation: false,
            },
            ToolDescriptor {
                name: "clipboard_write".to_string(),
                description: "Write text content to the system clipboard.".to_string(),
                parameters: serde_json::json!({
                    "type": "object",
                    "properties": {
                        "text": {
                            "type": "string",
                            "description": "The text to write to the clipboard"
                        }
                    },
                    "required": ["text"]
                }),
                returns: serde_json::json!({
                    "type": "object",
                    "properties": {
                        "written": { "type": "boolean" },
                        "length": { "type": "integer" }
                    }
                }),
                cost_estimate: CostEstimate {
                    latency_ms: 50,
                    network_bound: false,
                    token_cost: None,
                },
                required_permissions: vec![Permission::Clipboard],
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
            "clipboard_read" => self.read_clipboard(),
            "clipboard_write" => {
                let text: String = params.get("text").ok_or_else(|| {
                    SkillError::ToolFailed("Missing required parameter: text".to_string())
                })?;
                self.write_clipboard(&text)
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

    fn test_skill() -> ClipboardSkill {
        ClipboardSkill::new(ClipboardSkill::default_manifest())
    }

    #[test]
    fn test_manifest_parses() {
        let manifest = ClipboardSkill::default_manifest();
        assert_eq!(manifest.name, "Clipboard");
    }

    #[test]
    fn test_tools_list() {
        let skill = test_skill();
        let tools = skill.tools();
        assert_eq!(tools.len(), 2);
        assert_eq!(tools[0].name, "clipboard_read");
        assert_eq!(tools[1].name, "clipboard_write");
    }

    #[test]
    fn test_write_requires_confirmation() {
        let skill = test_skill();
        let tools = skill.tools();
        let write_tool = tools.iter().find(|t| t.name == "clipboard_write").unwrap();
        assert!(write_tool.requires_confirmation);
        assert!(!write_tool.autonomous);

        let read_tool = tools.iter().find(|t| t.name == "clipboard_read").unwrap();
        assert!(!read_tool.requires_confirmation);
        assert!(read_tool.autonomous);
    }
}
