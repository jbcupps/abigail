//! MCP bridge capability (stub).

use async_trait::async_trait;

use crate::SkillResult;

#[async_trait]
pub trait McpClientCapability: Send + Sync {
    async fn list_tools(&self) -> SkillResult<Vec<McpTool>> {
        Ok(vec![])
    }
    async fn call_tool(
        &self,
        _tool: &str,
        _args: serde_json::Value,
    ) -> SkillResult<serde_json::Value> {
        Err(crate::SkillError::ToolFailed("stub".into()))
    }
}

#[derive(Debug, Clone)]
pub struct McpTool {
    pub name: String,
    pub description: Option<String>,
}
