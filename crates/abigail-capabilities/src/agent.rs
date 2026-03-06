//! Agent cooperation capability trait (stub).

use async_trait::async_trait;

#[derive(Debug, Clone)]
pub struct AgentInfo;

#[derive(Debug, Clone)]
pub struct AgentMessage;

#[derive(Debug, Clone)]
pub struct TaskRequest;

#[derive(Debug, Clone)]
pub struct TaskHandle;

#[derive(Debug, Clone)]
pub struct TaskStatus;

#[async_trait]
pub trait AgentCooperationCapability: Send + Sync {
    async fn discover_agents(&self) -> anyhow::Result<Vec<AgentInfo>>;
    async fn send_message(&self, agent_id: &str, message: AgentMessage) -> anyhow::Result<()>;
    async fn delegate_task(
        &self,
        agent_id: &str,
        task: TaskRequest,
    ) -> anyhow::Result<TaskHandle>;
}
