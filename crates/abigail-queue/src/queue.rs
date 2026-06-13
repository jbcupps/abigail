//! Job queue persistence backed by the SurrealDB Hive store.

use crate::types::*;
use abigail_persistence::PersistenceHandle;
use abigail_streaming::StreamBroker;
use chrono::Utc;
use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::broadcast;

/// Maximum sub-agent nesting depth: a mentor-initiated job (depth 0) may
/// spawn sub-jobs (depth 1) which may spawn one more level (depth 2).
pub const MAX_SUBAGENT_DEPTH: u32 = 2;

pub struct JobQueue {
    store: PersistenceHandle,
    broker: Arc<dyn StreamBroker>,
    local_bus: broadcast::Sender<JobEvent>,
}

impl JobQueue {
    const STREAM: &'static str = abigail_streaming::BUS_STREAM;
    const TOPIC: &'static str = abigail_streaming::Topic::JobEvents.as_str();

    pub fn new(store: PersistenceHandle, broker: Arc<dyn StreamBroker>) -> Self {
        let (local_bus, _) = broadcast::channel(256);
        Self {
            store,
            broker,
            local_bus,
        }
    }

    pub fn subscribe_local(&self) -> broadcast::Receiver<JobEvent> {
        self.local_bus.subscribe()
    }

    pub async fn submit_job(&self, spec: JobSpec) -> anyhow::Result<JobId> {
        if spec.depth > MAX_SUBAGENT_DEPTH {
            anyhow::bail!(
                "Job depth {} exceeds the sub-agent nesting limit ({})",
                spec.depth,
                MAX_SUBAGENT_DEPTH
            );
        }
        let job_id = uuid::Uuid::new_v4().to_string();
        let now = Utc::now();
        let record = JobRecord {
            id: job_id.clone(),
            topic: spec.topic.clone(),
            goal: spec.goal,
            capability: spec.capability.clone(),
            priority: spec.priority,
            status: JobStatus::Queued,
            time_budget_ms: spec.time_budget_ms,
            max_turns: spec.max_turns,
            system_context: spec.system_context,
            allowed_skill_ids: spec.allowed_skill_ids,
            input_data: spec.input_data,
            parent_job_id: spec.parent_job_id,
            parent_correlation_id: spec.parent_correlation_id,
            depth: spec.depth,
            provider_profile: spec.provider_profile,
            agent_id: None,
            model_used: None,
            provider_used: None,
            result: None,
            error: None,
            turns_consumed: 0,
            ttl_seconds: spec.ttl_seconds,
            created_at: now.to_rfc3339(),
            started_at: None,
            completed_at: None,
            expires_at: (now + chrono::Duration::seconds(spec.ttl_seconds as i64)).to_rfc3339(),
            cron_expression: spec.cron_expression,
            is_recurring: spec.is_recurring,
            significance_keywords: spec.significance_keywords,
            significance_threshold: spec.significance_threshold,
            job_mode: spec.job_mode,
            goal_template: spec.goal_template,
            last_scheduled_at: None,
            depends_on: spec.depends_on,
            execution_mode: spec.execution_mode,
            direct_tool_call: spec.direct_tool_call,
        };

        self.store.create("job_record", &job_id, &record)?;

        let event = JobEvent::JobQueued {
            job_id: job_id.clone(),
            topic: record.topic.clone(),
            capability: record.capability.clone(),
            priority: record.priority,
        };
        self.publish_event(&event).await;

        tracing::info!("Job {} queued in topic '{}'", job_id, record.topic);
        Ok(job_id)
    }

    pub async fn mark_running(
        &self,
        job_id: &str,
        agent_id: &str,
        model_used: &str,
        provider_used: &str,
    ) -> anyhow::Result<()> {
        let mut record = self.require_job(job_id)?;
        if record.status != JobStatus::Queued {
            anyhow::bail!("Job '{}' not found or not in queued state", job_id);
        }

        record.status = JobStatus::Running;
        record.agent_id = Some(agent_id.to_string());
        record.model_used = Some(model_used.to_string());
        record.provider_used = Some(provider_used.to_string());
        record.started_at = Some(Utc::now().to_rfc3339());
        self.store.upsert("job_record", &record.id, &record)?;

        let event = JobEvent::JobStarted {
            job_id: record.id.clone(),
            topic: record.topic.clone(),
            agent_id: agent_id.to_string(),
            model_used: model_used.to_string(),
        };
        self.publish_event(&event).await;
        Ok(())
    }

    pub async fn mark_completed(
        &self,
        job_id: &str,
        result: &str,
        turns_consumed: u32,
    ) -> anyhow::Result<()> {
        let mut record = self.require_job(job_id)?;
        if record.status != JobStatus::Running {
            anyhow::bail!("Job '{}' not found or not in running state", job_id);
        }

        record.status = JobStatus::Completed;
        record.result = Some(result.to_string());
        record.turns_consumed = turns_consumed;
        record.completed_at = Some(Utc::now().to_rfc3339());
        self.store.upsert("job_record", &record.id, &record)?;

        let event = JobEvent::JobCompleted {
            job_id: record.id.clone(),
            topic: record.topic.clone(),
            result: result.to_string(),
            turns_consumed,
        };
        self.publish_event(&event).await;
        Ok(())
    }

    pub async fn mark_failed(
        &self,
        job_id: &str,
        error: &str,
        turns_consumed: u32,
    ) -> anyhow::Result<()> {
        let mut record = self.require_job(job_id)?;
        if record.status != JobStatus::Running {
            anyhow::bail!("Job '{}' not found or not in running state", job_id);
        }

        record.status = JobStatus::Failed;
        record.error = Some(error.to_string());
        record.turns_consumed = turns_consumed;
        record.completed_at = Some(Utc::now().to_rfc3339());
        self.store.upsert("job_record", &record.id, &record)?;

        let event = JobEvent::JobFailed {
            job_id: record.id.clone(),
            topic: record.topic.clone(),
            error: error.to_string(),
            turns_consumed,
        };
        self.publish_event(&event).await;
        Ok(())
    }

    pub async fn cancel_job(&self, job_id: &str) -> anyhow::Result<()> {
        let mut record = self.require_job(job_id)?;
        if record.status.is_terminal() {
            anyhow::bail!("Job '{}' not found or already in terminal state", job_id);
        }

        record.status = JobStatus::Cancelled;
        record.completed_at = Some(Utc::now().to_rfc3339());
        self.store.upsert("job_record", &record.id, &record)?;

        let event = JobEvent::JobCancelled {
            job_id: record.id.clone(),
            topic: record.topic.clone(),
        };
        self.publish_event(&event).await;
        Ok(())
    }

    pub async fn expire_stale_jobs(&self) -> anyhow::Result<usize> {
        let now = Utc::now().to_rfc3339();
        let mut expired = Vec::new();

        for mut job in self.load_all_jobs()? {
            if job.status == JobStatus::Queued && job.expires_at < now {
                job.status = JobStatus::Expired;
                self.store.upsert("job_record", &job.id, &job)?;
                expired.push((job.id.clone(), job.topic.clone()));
            }
        }

        for (job_id, topic) in &expired {
            self.publish_event(&JobEvent::JobExpired {
                job_id: job_id.clone(),
                topic: topic.clone(),
            })
            .await;
        }

        if !expired.is_empty() {
            tracing::info!("Expired {} stale jobs", expired.len());
        }
        Ok(expired.len())
    }

    pub fn get_job(&self, job_id: &str) -> anyhow::Result<Option<JobRecord>> {
        self.store
            .select_record("job_record", job_id)
            .map_err(Into::into)
    }

    pub fn list_jobs(&self, status: Option<&str>, limit: usize) -> anyhow::Result<Vec<JobRecord>> {
        let mut jobs = self.load_all_jobs()?;
        if let Some(status) = status {
            jobs.retain(|job| job.status.as_str() == status);
        }
        jobs.sort_by(|left, right| {
            right
                .priority
                .cmp(&left.priority)
                .then_with(|| left.created_at.cmp(&right.created_at))
        });
        jobs.truncate(limit);
        Ok(jobs)
    }

    pub fn list_topic_jobs(&self, topic: &str, limit: usize) -> anyhow::Result<Vec<JobRecord>> {
        let mut jobs: Vec<JobRecord> = self
            .load_all_jobs()?
            .into_iter()
            .filter(|job| job.topic == topic)
            .collect();
        jobs.sort_by(|left, right| right.created_at.cmp(&left.created_at));
        jobs.truncate(limit);
        Ok(jobs)
    }

    pub fn topic_results(&self, topic: &str, limit: usize) -> anyhow::Result<Vec<JobRecord>> {
        let mut jobs: Vec<JobRecord> = self
            .load_all_jobs()?
            .into_iter()
            .filter(|job| job.topic == topic && job.status == JobStatus::Completed)
            .collect();
        jobs.sort_by(|left, right| right.completed_at.cmp(&left.completed_at));
        jobs.truncate(limit);
        Ok(jobs)
    }

    pub fn next_queued_job(&self) -> anyhow::Result<Option<JobRecord>> {
        let jobs = self.load_all_jobs()?;
        let completed_ids: std::collections::HashSet<String> = jobs
            .iter()
            .filter(|job| job.status == JobStatus::Completed)
            .map(|job| job.id.clone())
            .collect();

        let mut candidates: Vec<JobRecord> = jobs
            .into_iter()
            .filter(|job| {
                job.status == JobStatus::Queued
                    && !job.is_recurring
                    && job
                        .parent_job_id
                        .as_ref()
                        .map(|parent| completed_ids.contains(parent))
                        .unwrap_or(true)
                    && job.depends_on.iter().all(|dep| completed_ids.contains(dep))
            })
            .collect();

        candidates.sort_by(|left, right| {
            right
                .priority
                .cmp(&left.priority)
                .then_with(|| left.created_at.cmp(&right.created_at))
        });
        Ok(candidates.into_iter().next())
    }

    pub fn queue_stats(&self) -> anyhow::Result<HashMap<String, u64>> {
        let mut stats = HashMap::new();
        for job in self.load_all_jobs()? {
            *stats.entry(job.status.as_str().to_string()).or_insert(0) += 1;
        }
        Ok(stats)
    }

    pub fn topic_all_terminal(&self, topic: &str) -> anyhow::Result<bool> {
        Ok(self
            .load_all_jobs()?
            .into_iter()
            .filter(|job| job.topic == topic)
            .all(|job| job.status.is_terminal()))
    }

    pub fn recover_running_jobs(&self, reason: &str) -> anyhow::Result<usize> {
        let mut recovered = 0usize;
        for mut job in self.load_all_jobs()? {
            if job.status == JobStatus::Running {
                job.status = JobStatus::Failed;
                job.error = Some(reason.to_string());
                job.completed_at = Some(Utc::now().to_rfc3339());
                self.store.upsert("job_record", &job.id, &job)?;
                recovered += 1;
            }
        }
        if recovered > 0 {
            tracing::warn!(
                "Crash recovery: marked {} running jobs as failed",
                recovered
            );
        }
        Ok(recovered)
    }

    pub fn get_recurring_templates(&self) -> anyhow::Result<Vec<JobRecord>> {
        Ok(self
            .load_all_jobs()?
            .into_iter()
            .filter(|job| {
                job.is_recurring
                    && job
                        .cron_expression
                        .as_ref()
                        .is_some_and(|cron| !cron.is_empty())
                    && job.status == JobStatus::Queued
            })
            .collect())
    }

    pub async fn spawn_recurring_instance(
        &self,
        template: &JobRecord,
        goal_override: Option<String>,
    ) -> anyhow::Result<JobId> {
        let spec = JobSpec {
            goal: goal_override.unwrap_or_else(|| template.goal.clone()),
            topic: template.topic.clone(),
            capability: template.capability.clone(),
            priority: template.priority,
            time_budget_ms: template.time_budget_ms,
            max_turns: template.max_turns,
            system_context: template.system_context.clone(),
            allowed_skill_ids: template.allowed_skill_ids.clone(),
            ttl_seconds: template.ttl_seconds,
            input_data: template.input_data.clone(),
            parent_job_id: Some(template.id.clone()),
            parent_correlation_id: template.parent_correlation_id.clone(),
            depth: template.depth,
            provider_profile: template.provider_profile.clone(),
            cron_expression: None,
            is_recurring: false,
            significance_keywords: template.significance_keywords.clone(),
            significance_threshold: template.significance_threshold,
            job_mode: template.job_mode.clone(),
            goal_template: None,
            depends_on: Vec::new(),
            execution_mode: template.execution_mode.clone(),
            direct_tool_call: template.direct_tool_call.clone(),
        };

        let job_id = self.submit_job(spec).await?;
        let mut updated_template = template.clone();
        updated_template.last_scheduled_at = Some(Utc::now().to_rfc3339());
        self.store
            .upsert("job_record", &updated_template.id, &updated_template)?;

        tracing::info!(
            "Spawned recurring instance {} from template {}",
            job_id,
            template.id
        );
        Ok(job_id)
    }

    fn load_all_jobs(&self) -> anyhow::Result<Vec<JobRecord>> {
        self.store
            .query_vec("SELECT * FROM job_record", &[])
            .map_err(Into::into)
    }

    fn require_job(&self, job_id: &str) -> anyhow::Result<JobRecord> {
        self.get_job(job_id)?
            .ok_or_else(|| anyhow::anyhow!("Job '{}' not found", job_id))
    }

    async fn publish_event(&self, event: &JobEvent) {
        let _ = self.local_bus.send(event.clone());

        let payload = match serde_json::to_vec(event) {
            Ok(payload) => payload,
            Err(error) => {
                tracing::error!("Failed to serialize JobEvent: {}", error);
                return;
            }
        };

        let mut headers = HashMap::new();
        headers.insert("event_type".to_string(), event.event_type_str().to_string());
        headers.insert("topic".to_string(), event.topic().to_string());
        headers.insert("source".to_string(), "job_queue".to_string());

        let msg = abigail_streaming::StreamMessage::with_headers(payload, headers);
        if let Err(error) = self.broker.publish(Self::STREAM, Self::TOPIC, msg).await {
            tracing::error!("Failed to publish JobEvent to broker: {}", error);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use abigail_persistence::{ci_mode_enabled, EntityScope, PersistenceHandle};
    use abigail_streaming::MemoryBroker;

    fn setup_test_queue() -> JobQueue {
        let store = if cfg!(target_os = "windows") || ci_mode_enabled() {
            PersistenceHandle::open_ephemeral(EntityScope::Hive).unwrap()
        } else {
            let root =
                std::env::temp_dir().join(format!("abigail-job-queue-{}", uuid::Uuid::new_v4()));
            PersistenceHandle::open(root.join("memory.db"), EntityScope::Hive).unwrap()
        };
        let broker = Arc::new(MemoryBroker::new(64));
        JobQueue::new(store, broker)
    }

    fn test_spec(topic: &str) -> JobSpec {
        JobSpec {
            goal: "Test job".into(),
            topic: topic.into(),
            capability: RequiredCapability::General,
            priority: JobPriority::Normal,
            time_budget_ms: 60_000,
            max_turns: 5,
            system_context: None,
            allowed_skill_ids: vec![],
            ttl_seconds: 3600,
            input_data: None,
            parent_job_id: None,
            parent_correlation_id: None,
            depth: 0,
            provider_profile: None,
            cron_expression: None,
            is_recurring: false,
            significance_keywords: vec![],
            significance_threshold: 0.5,
            job_mode: "agentic_run".into(),
            goal_template: None,
            depends_on: vec![],
            execution_mode: ExecutionMode::Mediated,
            direct_tool_call: None,
        }
    }

    #[tokio::test]
    async fn test_submit_and_get() {
        let queue = setup_test_queue();
        let job_id = queue.submit_job(test_spec("test-topic")).await.unwrap();

        let record = queue.get_job(&job_id).unwrap().unwrap();
        assert_eq!(record.status, JobStatus::Queued);
        assert_eq!(record.topic, "test-topic");
    }

    #[tokio::test]
    async fn test_submit_rejects_excess_depth() {
        let queue = setup_test_queue();

        let mut at_limit = test_spec("depth-ok");
        at_limit.depth = MAX_SUBAGENT_DEPTH;
        assert!(queue.submit_job(at_limit).await.is_ok());

        let mut too_deep = test_spec("depth-exceeded");
        too_deep.depth = MAX_SUBAGENT_DEPTH + 1;
        let err = queue.submit_job(too_deep).await.unwrap_err();
        assert!(err.to_string().contains("nesting limit"));
    }

    #[tokio::test]
    async fn test_submit_carries_provider_profile_and_correlation() {
        let queue = setup_test_queue();
        let mut spec = test_spec("profile-topic");
        spec.provider_profile = Some("perplexity".to_string());
        spec.parent_correlation_id = Some("turn-123".to_string());
        spec.depth = 1;

        let job_id = queue.submit_job(spec).await.unwrap();
        let record = queue.get_job(&job_id).unwrap().unwrap();
        assert_eq!(record.provider_profile.as_deref(), Some("perplexity"));
        assert_eq!(record.parent_correlation_id.as_deref(), Some("turn-123"));
        assert_eq!(record.depth, 1);
    }

    #[tokio::test]
    async fn test_job_lifecycle() {
        let queue = setup_test_queue();
        let job_id = queue.submit_job(test_spec("lifecycle")).await.unwrap();

        queue
            .mark_running(&job_id, "agent-1", "gpt-4.1", "openai")
            .await
            .unwrap();
        queue.mark_completed(&job_id, "done", 2).await.unwrap();

        let record = queue.get_job(&job_id).unwrap().unwrap();
        assert_eq!(record.status, JobStatus::Completed);
        assert_eq!(record.turns_consumed, 2);
    }

    #[tokio::test]
    async fn test_depends_on_blocks_scheduling() {
        let queue = setup_test_queue();
        let dep1 = queue.submit_job(test_spec("deps")).await.unwrap();
        let dep2 = queue.submit_job(test_spec("deps")).await.unwrap();

        let mut dependent = test_spec("deps");
        dependent.depends_on = vec![dep1.clone(), dep2.clone()];
        dependent.priority = JobPriority::Critical;
        let dependent_id = queue.submit_job(dependent).await.unwrap();

        let next = queue.next_queued_job().unwrap().unwrap();
        assert_ne!(next.id, dependent_id);

        queue.mark_running(&dep1, "a", "m", "p").await.unwrap();
        queue.mark_completed(&dep1, "ok", 1).await.unwrap();
        queue.mark_running(&dep2, "a", "m", "p").await.unwrap();
        queue.mark_completed(&dep2, "ok", 1).await.unwrap();

        let next = queue.next_queued_job().unwrap().unwrap();
        assert_eq!(next.id, dependent_id);
    }
}
