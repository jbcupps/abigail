use abigail_queue::{JobQueue, JobRecord, JobSpec};
use abigail_skills::QueueOperations;
use async_trait::async_trait;
use std::sync::Arc;

#[derive(Clone)]
pub struct LocalQueueOperations {
    queue: Arc<JobQueue>,
}

impl LocalQueueOperations {
    pub fn new(queue: Arc<JobQueue>) -> Self {
        Self { queue }
    }
}

#[async_trait]
impl QueueOperations for LocalQueueOperations {
    async fn submit_job(&self, spec: JobSpec) -> Result<String, String> {
        self.queue.submit_job(spec).await.map_err(|e| e.to_string())
    }

    async fn get_job(&self, job_id: &str) -> Result<Option<JobRecord>, String> {
        self.queue.get_job(job_id).map_err(|e| e.to_string())
    }

    async fn list_jobs(
        &self,
        status: Option<&str>,
        limit: usize,
    ) -> Result<Vec<JobRecord>, String> {
        self.queue
            .list_jobs(status, limit)
            .map_err(|e| e.to_string())
    }

    async fn cancel_job(&self, job_id: &str) -> Result<(), String> {
        self.queue
            .cancel_job(job_id)
            .await
            .map_err(|e| e.to_string())
    }

    async fn topic_results(&self, topic: &str, limit: usize) -> Result<Vec<JobRecord>, String> {
        self.queue
            .topic_results(topic, limit)
            .map_err(|e| e.to_string())
    }

    async fn topic_all_terminal(&self, topic: &str) -> Result<bool, String> {
        self.queue
            .topic_all_terminal(topic)
            .map_err(|e| e.to_string())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use abigail_persistence::{EntityScope, PersistenceHandle};
    use abigail_skills::{ExecutionContext, QueueManagementSkill, Skill, ToolParams};
    use abigail_streaming::MemoryBroker;

    fn test_queue() -> Arc<JobQueue> {
        let store = PersistenceHandle::open_ephemeral(EntityScope::Hive).unwrap();
        Arc::new(JobQueue::new(store, Arc::new(MemoryBroker::new(64))))
    }

    fn job_context(depth: u32) -> ExecutionContext {
        ExecutionContext {
            request_id: "test".to_string(),
            job_depth: Some(depth),
            correlation_id: Some("trace-root".to_string()),
            ..Default::default()
        }
    }

    fn submit_params() -> ToolParams {
        ToolParams::new()
            .with("goal", "delegated child task")
            .with("topic", "depth-test")
    }

    #[tokio::test]
    async fn depth_one_job_submits_depth_two_child_with_correlation() {
        let queue = test_queue();
        let skill = QueueManagementSkill::new(Arc::new(LocalQueueOperations::new(queue.clone())));

        let out = skill
            .execute_tool("submit_job", submit_params(), &job_context(1))
            .await
            .unwrap();
        let job_id = out.data.unwrap()["job_id"].as_str().unwrap().to_string();

        let record = queue.get_job(&job_id).unwrap().unwrap();
        assert_eq!(record.depth, 2);
        assert_eq!(record.parent_correlation_id.as_deref(), Some("trace-root"));
    }

    #[tokio::test]
    async fn depth_two_job_submission_attempt_is_rejected() {
        let queue = test_queue();
        let skill = QueueManagementSkill::new(Arc::new(LocalQueueOperations::new(queue.clone())));

        let err = skill
            .execute_tool("submit_job", submit_params(), &job_context(2))
            .await
            .unwrap_err();
        assert!(
            err.to_string().contains("nesting limit"),
            "expected nesting-limit rejection, got: {}",
            err
        );
        assert!(
            queue.list_jobs(None, 10).unwrap().is_empty(),
            "rejected submission must not enqueue a job"
        );
    }
}
