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
