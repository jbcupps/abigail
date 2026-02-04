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
    async fn discover_agents(&self) -> anyhow::Result<Vec<AgentInfo>> {
        Ok(vec![])
    }
    async fn send_message(&self, _agent_id: &str, _message: AgentMessage) -> anyhow::Result<()> {
        Err(anyhow::anyhow!("stub: not implemented"))
    }
    async fn delegate_task(&self, _agent_id: &str, _task: TaskRequest) -> anyhow::Result<TaskHandle> {
        Err(anyhow::anyhow!("stub: not implemented"))
    }
}
