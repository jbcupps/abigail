//! JobQueue — SQLite-backed job persistence with StreamBroker event publishing.
//!
//! SQLite is the source of truth for job state. Every state mutation also
//! publishes a `JobEvent` to the `StreamBroker` for real-time consumers.

use crate::types::*;
use abigail_streaming::StreamBroker;
use chrono::Utc;
use rusqlite::Connection;
use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use tokio::sync::broadcast;

/// Persistent job queue backed by SQLite with event streaming.
pub struct JobQueue {
    db: Arc<Mutex<Connection>>,
    broker: Arc<dyn StreamBroker>,
    local_bus: broadcast::Sender<JobEvent>,
}

impl JobQueue {
    /// Stream name used for job events.
    const STREAM: &'static str = "abigail";
    /// Topic name for job lifecycle events.
    const TOPIC: &'static str = "job-events";

    /// Create a new JobQueue using the given database connection and broker.
    pub fn new(db: Arc<Mutex<Connection>>, broker: Arc<dyn StreamBroker>) -> Self {
        let (local_bus, _) = broadcast::channel(256);
        Self {
            db,
            broker,
            local_bus,
        }
    }

    /// Get a receiver for local in-process job events.
    pub fn subscribe_local(&self) -> broadcast::Receiver<JobEvent> {
        self.local_bus.subscribe()
    }

    /// Submit a new job. Returns the assigned job ID.
    pub async fn submit_job(&self, spec: JobSpec) -> anyhow::Result<JobId> {
        let job_id = uuid::Uuid::new_v4().to_string();
        let now = Utc::now();
        let created_at = now.to_rfc3339();
        let expires_at = (now + chrono::Duration::seconds(spec.ttl_seconds as i64)).to_rfc3339();
        let allowed_skills_json = serde_json::to_string(&spec.allowed_skill_ids)?;
        let input_data_json = spec
            .input_data
            .as_ref()
            .map(|v| serde_json::to_string(v))
            .transpose()?;
        let capability_str = spec.capability.as_str().to_string();
        let priority_i32 = spec.priority.as_i32();

        {
            let conn = self.lock_db()?;
            conn.execute(
                "INSERT INTO job_queue \
                 (id, topic, goal, capability, priority, status, time_budget_ms, max_turns, \
                  system_context, allowed_skill_ids, input_data, parent_job_id, \
                  turns_consumed, ttl_seconds, created_at, expires_at) \
                 VALUES (?1, ?2, ?3, ?4, ?5, 'queued', ?6, ?7, ?8, ?9, ?10, ?11, 0, ?12, ?13, ?14)",
                rusqlite::params![
                    job_id,
                    spec.topic,
                    spec.goal,
                    capability_str,
                    priority_i32,
                    spec.time_budget_ms as i64,
                    spec.max_turns as i64,
                    spec.system_context,
                    allowed_skills_json,
                    input_data_json,
                    spec.parent_job_id,
                    spec.ttl_seconds as i64,
                    created_at,
                    expires_at,
                ],
            )?;
        }

        let event = JobEvent::JobQueued {
            job_id: job_id.clone(),
            topic: spec.topic.clone(),
            capability: spec.capability,
            priority: spec.priority,
        };
        self.publish_event(&event).await;

        tracing::info!("Job {} queued in topic '{}'", job_id, spec.topic);
        Ok(job_id)
    }

    /// Mark a job as running with the given agent and model info.
    pub async fn mark_running(
        &self,
        job_id: &str,
        agent_id: &str,
        model_used: &str,
        provider_used: &str,
    ) -> anyhow::Result<()> {
        let started_at = Utc::now().to_rfc3339();
        let topic: String;

        {
            let conn = self.lock_db()?;
            let updated = conn.execute(
                "UPDATE job_queue SET status = 'running', agent_id = ?2, model_used = ?3, \
                 provider_used = ?4, started_at = ?5 \
                 WHERE id = ?1 AND status = 'queued'",
                rusqlite::params![job_id, agent_id, model_used, provider_used, started_at],
            )?;
            if updated == 0 {
                anyhow::bail!("Job '{}' not found or not in queued state", job_id);
            }
            topic = conn.query_row(
                "SELECT topic FROM job_queue WHERE id = ?1",
                [job_id],
                |row| row.get(0),
            )?;
        }

        let event = JobEvent::JobStarted {
            job_id: job_id.to_string(),
            topic,
            agent_id: agent_id.to_string(),
            model_used: model_used.to_string(),
        };
        self.publish_event(&event).await;
        Ok(())
    }

    /// Mark a job as completed with its result.
    pub async fn mark_completed(
        &self,
        job_id: &str,
        result: &str,
        turns_consumed: u32,
    ) -> anyhow::Result<()> {
        let completed_at = Utc::now().to_rfc3339();
        let topic: String;

        {
            let conn = self.lock_db()?;
            let updated = conn.execute(
                "UPDATE job_queue SET status = 'completed', result = ?2, \
                 turns_consumed = ?3, completed_at = ?4 \
                 WHERE id = ?1 AND status = 'running'",
                rusqlite::params![job_id, result, turns_consumed as i64, completed_at],
            )?;
            if updated == 0 {
                anyhow::bail!("Job '{}' not found or not in running state", job_id);
            }
            topic = conn.query_row(
                "SELECT topic FROM job_queue WHERE id = ?1",
                [job_id],
                |row| row.get(0),
            )?;
        }

        let event = JobEvent::JobCompleted {
            job_id: job_id.to_string(),
            topic,
            result: result.to_string(),
            turns_consumed,
        };
        self.publish_event(&event).await;
        Ok(())
    }

    /// Mark a job as failed with an error message.
    pub async fn mark_failed(
        &self,
        job_id: &str,
        error: &str,
        turns_consumed: u32,
    ) -> anyhow::Result<()> {
        let completed_at = Utc::now().to_rfc3339();
        let topic: String;

        {
            let conn = self.lock_db()?;
            let updated = conn.execute(
                "UPDATE job_queue SET status = 'failed', error = ?2, \
                 turns_consumed = ?3, completed_at = ?4 \
                 WHERE id = ?1 AND status = 'running'",
                rusqlite::params![job_id, error, turns_consumed as i64, completed_at],
            )?;
            if updated == 0 {
                anyhow::bail!("Job '{}' not found or not in running state", job_id);
            }
            topic = conn.query_row(
                "SELECT topic FROM job_queue WHERE id = ?1",
                [job_id],
                |row| row.get(0),
            )?;
        }

        let event = JobEvent::JobFailed {
            job_id: job_id.to_string(),
            topic,
            error: error.to_string(),
            turns_consumed,
        };
        self.publish_event(&event).await;
        Ok(())
    }

    /// Cancel a job (from queued or running state).
    pub async fn cancel_job(&self, job_id: &str) -> anyhow::Result<()> {
        let completed_at = Utc::now().to_rfc3339();
        let topic: String;

        {
            let conn = self.lock_db()?;
            let updated = conn.execute(
                "UPDATE job_queue SET status = 'cancelled', completed_at = ?2 \
                 WHERE id = ?1 AND status IN ('queued', 'running')",
                rusqlite::params![job_id, completed_at],
            )?;
            if updated == 0 {
                anyhow::bail!("Job '{}' not found or already in terminal state", job_id);
            }
            topic = conn.query_row(
                "SELECT topic FROM job_queue WHERE id = ?1",
                [job_id],
                |row| row.get(0),
            )?;
        }

        let event = JobEvent::JobCancelled {
            job_id: job_id.to_string(),
            topic,
        };
        self.publish_event(&event).await;
        Ok(())
    }

    /// Expire queued jobs that have exceeded their TTL.
    /// Returns the number of jobs expired.
    pub async fn expire_stale_jobs(&self) -> anyhow::Result<usize> {
        let now = Utc::now().to_rfc3339();
        let expired_jobs: Vec<(String, String)>;

        {
            let conn = self.lock_db()?;
            let mut stmt = conn.prepare(
                "SELECT id, topic FROM job_queue \
                 WHERE status = 'queued' AND expires_at < ?1",
            )?;
            expired_jobs = stmt
                .query_map([&now], |row| Ok((row.get(0)?, row.get(1)?)))?
                .filter_map(|r| r.ok())
                .collect();

            conn.execute(
                "UPDATE job_queue SET status = 'expired' \
                 WHERE status = 'queued' AND expires_at < ?1",
                [&now],
            )?;
        }

        for (job_id, topic) in &expired_jobs {
            let event = JobEvent::JobExpired {
                job_id: job_id.clone(),
                topic: topic.clone(),
            };
            self.publish_event(&event).await;
        }

        if !expired_jobs.is_empty() {
            tracing::info!("Expired {} stale jobs", expired_jobs.len());
        }
        Ok(expired_jobs.len())
    }

    /// Get a job record by ID.
    pub fn get_job(&self, job_id: &str) -> anyhow::Result<Option<JobRecord>> {
        let conn = self.lock_db()?;
        let mut stmt = conn.prepare(
            "SELECT id, topic, goal, capability, priority, status, time_budget_ms, max_turns, \
             system_context, allowed_skill_ids, input_data, parent_job_id, agent_id, \
             model_used, provider_used, result, error, turns_consumed, ttl_seconds, \
             created_at, started_at, completed_at, expires_at \
             FROM job_queue WHERE id = ?1",
        )?;
        let result = stmt
            .query_map([job_id], Self::map_job_record)?
            .filter_map(|r| r.ok())
            .next();
        Ok(result)
    }

    /// List jobs filtered by status, ordered by priority (desc) then created_at (asc).
    pub fn list_jobs(&self, status: Option<&str>, limit: usize) -> anyhow::Result<Vec<JobRecord>> {
        let conn = self.lock_db()?;
        let (sql, params): (&str, Vec<Box<dyn rusqlite::types::ToSql>>) = match status {
            Some(s) => (
                "SELECT id, topic, goal, capability, priority, status, time_budget_ms, max_turns, \
                 system_context, allowed_skill_ids, input_data, parent_job_id, agent_id, \
                 model_used, provider_used, result, error, turns_consumed, ttl_seconds, \
                 created_at, started_at, completed_at, expires_at \
                 FROM job_queue WHERE status = ?1 \
                 ORDER BY priority DESC, created_at ASC LIMIT ?2",
                vec![
                    Box::new(s.to_string()) as Box<dyn rusqlite::types::ToSql>,
                    Box::new(limit as i64),
                ],
            ),
            None => (
                "SELECT id, topic, goal, capability, priority, status, time_budget_ms, max_turns, \
                 system_context, allowed_skill_ids, input_data, parent_job_id, agent_id, \
                 model_used, provider_used, result, error, turns_consumed, ttl_seconds, \
                 created_at, started_at, completed_at, expires_at \
                 FROM job_queue \
                 ORDER BY priority DESC, created_at ASC LIMIT ?1",
                vec![Box::new(limit as i64) as Box<dyn rusqlite::types::ToSql>],
            ),
        };

        let mut stmt = conn.prepare(sql)?;
        let params_ref: Vec<&dyn rusqlite::types::ToSql> =
            params.iter().map(|p| p.as_ref()).collect();
        let rows = stmt
            .query_map(params_ref.as_slice(), Self::map_job_record)?
            .filter_map(|r| r.ok())
            .collect();
        Ok(rows)
    }

    /// List all jobs in a topic, ordered by creation time (newest first).
    pub fn list_topic_jobs(&self, topic: &str, limit: usize) -> anyhow::Result<Vec<JobRecord>> {
        let conn = self.lock_db()?;
        let mut stmt = conn.prepare(
            "SELECT id, topic, goal, capability, priority, status, time_budget_ms, max_turns, \
             system_context, allowed_skill_ids, input_data, parent_job_id, agent_id, \
             model_used, provider_used, result, error, turns_consumed, ttl_seconds, \
             created_at, started_at, completed_at, expires_at \
             FROM job_queue WHERE topic = ?1 \
             ORDER BY created_at DESC LIMIT ?2",
        )?;
        let rows = stmt
            .query_map(rusqlite::params![topic, limit as i64], Self::map_job_record)?
            .filter_map(|r| r.ok())
            .collect();
        Ok(rows)
    }

    /// Get completed results for a topic.
    pub fn topic_results(&self, topic: &str, limit: usize) -> anyhow::Result<Vec<JobRecord>> {
        let conn = self.lock_db()?;
        let mut stmt = conn.prepare(
            "SELECT id, topic, goal, capability, priority, status, time_budget_ms, max_turns, \
             system_context, allowed_skill_ids, input_data, parent_job_id, agent_id, \
             model_used, provider_used, result, error, turns_consumed, ttl_seconds, \
             created_at, started_at, completed_at, expires_at \
             FROM job_queue WHERE topic = ?1 AND status = 'completed' \
             ORDER BY completed_at DESC LIMIT ?2",
        )?;
        let rows = stmt
            .query_map(rusqlite::params![topic, limit as i64], Self::map_job_record)?
            .filter_map(|r| r.ok())
            .collect();
        Ok(rows)
    }

    /// Get the next queued job to run (highest priority, oldest first).
    /// Also respects parent_job_id dependency: only picks jobs whose parent is completed.
    pub fn next_queued_job(&self) -> anyhow::Result<Option<JobRecord>> {
        let conn = self.lock_db()?;
        let mut stmt = conn.prepare(
            "SELECT id, topic, goal, capability, priority, status, time_budget_ms, max_turns, \
             system_context, allowed_skill_ids, input_data, parent_job_id, agent_id, \
             model_used, provider_used, result, error, turns_consumed, ttl_seconds, \
             created_at, started_at, completed_at, expires_at \
             FROM job_queue \
             WHERE status = 'queued' \
               AND (parent_job_id IS NULL \
                    OR parent_job_id IN (SELECT id FROM job_queue WHERE status = 'completed')) \
             ORDER BY priority DESC, created_at ASC \
             LIMIT 1",
        )?;
        let result = stmt
            .query_map([], Self::map_job_record)?
            .filter_map(|r| r.ok())
            .next();
        Ok(result)
    }

    /// Count jobs by status.
    pub fn queue_stats(&self) -> anyhow::Result<HashMap<String, u64>> {
        let conn = self.lock_db()?;
        let mut stmt = conn.prepare("SELECT status, COUNT(*) FROM job_queue GROUP BY status")?;
        let rows = stmt.query_map([], |row| {
            Ok((row.get::<_, String>(0)?, row.get::<_, i64>(1)? as u64))
        })?;
        let mut stats = HashMap::new();
        for row in rows {
            let (status, count) = row?;
            stats.insert(status, count);
        }
        Ok(stats)
    }

    /// Check if all jobs in a topic have reached a terminal state.
    pub fn topic_all_terminal(&self, topic: &str) -> anyhow::Result<bool> {
        let conn = self.lock_db()?;
        let non_terminal: i64 = conn.query_row(
            "SELECT COUNT(*) FROM job_queue \
             WHERE topic = ?1 AND status NOT IN ('completed', 'failed', 'cancelled', 'expired')",
            [topic],
            |row| row.get(0),
        )?;
        Ok(non_terminal == 0)
    }

    /// Mark all running jobs as failed (crash recovery on restart).
    pub fn recover_running_jobs(&self, reason: &str) -> anyhow::Result<usize> {
        let conn = self.lock_db()?;
        let completed_at = Utc::now().to_rfc3339();
        let updated = conn.execute(
            "UPDATE job_queue SET status = 'failed', error = ?1, completed_at = ?2 \
             WHERE status = 'running'",
            rusqlite::params![reason, completed_at],
        )?;
        if updated > 0 {
            tracing::warn!("Crash recovery: marked {} running jobs as failed", updated);
        }
        Ok(updated)
    }

    // ── Internal helpers ──

    fn lock_db(&self) -> anyhow::Result<std::sync::MutexGuard<'_, Connection>> {
        self.db
            .lock()
            .map_err(|e| anyhow::anyhow!("Failed to lock database: {}", e))
    }

    async fn publish_event(&self, event: &JobEvent) {
        // Local broadcast (in-process)
        let _ = self.local_bus.send(event.clone());

        // StreamBroker (for external consumers / future Iggy)
        let payload = match serde_json::to_vec(event) {
            Ok(p) => p,
            Err(e) => {
                tracing::error!("Failed to serialize JobEvent: {}", e);
                return;
            }
        };

        let mut headers = HashMap::new();
        headers.insert("event_type".to_string(), event.event_type_str().to_string());
        headers.insert("topic".to_string(), event.topic().to_string());
        headers.insert("source".to_string(), "job_queue".to_string());

        let msg = abigail_streaming::StreamMessage::with_headers(payload, headers);
        if let Err(e) = self.broker.publish(Self::STREAM, Self::TOPIC, msg).await {
            tracing::error!("Failed to publish JobEvent to broker: {}", e);
        }
    }

    fn map_job_record(row: &rusqlite::Row<'_>) -> rusqlite::Result<JobRecord> {
        let capability_str: String = row.get(3)?;
        let priority_i32: i32 = row.get(4)?;
        let status_str: String = row.get(5)?;
        let allowed_skills_json: Option<String> = row.get(9)?;
        let input_data_json: Option<String> = row.get(10)?;

        let allowed_skill_ids: Vec<String> = allowed_skills_json
            .and_then(|s| serde_json::from_str(&s).ok())
            .unwrap_or_default();
        let input_data: Option<serde_json::Value> =
            input_data_json.and_then(|s| serde_json::from_str(&s).ok());

        Ok(JobRecord {
            id: row.get(0)?,
            topic: row.get(1)?,
            goal: row.get(2)?,
            capability: RequiredCapability::from_str_lossy(&capability_str),
            priority: JobPriority::from_i32(priority_i32),
            status: JobStatus::from_str_lossy(&status_str),
            time_budget_ms: row.get::<_, i64>(6)? as u64,
            max_turns: row.get::<_, i64>(7)? as u32,
            system_context: row.get(8)?,
            allowed_skill_ids,
            input_data,
            parent_job_id: row.get(11)?,
            agent_id: row.get(12)?,
            model_used: row.get(13)?,
            provider_used: row.get(14)?,
            result: row.get(15)?,
            error: row.get(16)?,
            turns_consumed: row.get::<_, i64>(17)? as u32,
            ttl_seconds: row.get::<_, i64>(18)? as u64,
            created_at: row.get(19)?,
            started_at: row.get(20)?,
            completed_at: row.get(21)?,
            expires_at: row.get(22)?,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use abigail_streaming::MemoryBroker;

    fn setup_test_queue() -> JobQueue {
        let conn = Connection::open_in_memory().unwrap();
        conn.execute_batch("PRAGMA journal_mode=WAL;").unwrap();
        conn.execute_batch(crate::schema::MIGRATION_V3_JOB_QUEUE)
            .unwrap();
        let db = Arc::new(Mutex::new(conn));
        let broker = Arc::new(MemoryBroker::new(64));
        JobQueue::new(db, broker)
    }

    fn test_spec(topic: &str) -> JobSpec {
        JobSpec {
            goal: "Test job".into(),
            topic: topic.into(),
            capability: RequiredCapability::General,
            priority: JobPriority::Normal,
            time_budget_ms: 60000,
            max_turns: 5,
            system_context: None,
            allowed_skill_ids: vec![],
            ttl_seconds: 3600,
            input_data: None,
            parent_job_id: None,
        }
    }

    #[tokio::test]
    async fn test_submit_and_get() {
        let queue = setup_test_queue();
        let job_id = queue.submit_job(test_spec("test-topic")).await.unwrap();

        let record = queue.get_job(&job_id).unwrap().unwrap();
        assert_eq!(record.status, JobStatus::Queued);
        assert_eq!(record.topic, "test-topic");
        assert_eq!(record.goal, "Test job");
    }

    #[tokio::test]
    async fn test_job_lifecycle() {
        let queue = setup_test_queue();
        let job_id = queue.submit_job(test_spec("lifecycle")).await.unwrap();

        // Queued -> Running
        queue
            .mark_running(&job_id, "agent-1", "gpt-4.1", "openai")
            .await
            .unwrap();
        let record = queue.get_job(&job_id).unwrap().unwrap();
        assert_eq!(record.status, JobStatus::Running);
        assert_eq!(record.agent_id.as_deref(), Some("agent-1"));

        // Running -> Completed
        queue
            .mark_completed(&job_id, "The answer is 42", 3)
            .await
            .unwrap();
        let record = queue.get_job(&job_id).unwrap().unwrap();
        assert_eq!(record.status, JobStatus::Completed);
        assert_eq!(record.result.as_deref(), Some("The answer is 42"));
        assert_eq!(record.turns_consumed, 3);
    }

    #[tokio::test]
    async fn test_job_failure() {
        let queue = setup_test_queue();
        let job_id = queue.submit_job(test_spec("fail")).await.unwrap();
        queue
            .mark_running(&job_id, "agent-1", "gpt-4.1", "openai")
            .await
            .unwrap();
        queue
            .mark_failed(&job_id, "Provider timeout", 2)
            .await
            .unwrap();

        let record = queue.get_job(&job_id).unwrap().unwrap();
        assert_eq!(record.status, JobStatus::Failed);
        assert_eq!(record.error.as_deref(), Some("Provider timeout"));
    }

    #[tokio::test]
    async fn test_cancel_job() {
        let queue = setup_test_queue();
        let job_id = queue.submit_job(test_spec("cancel")).await.unwrap();
        queue.cancel_job(&job_id).await.unwrap();

        let record = queue.get_job(&job_id).unwrap().unwrap();
        assert_eq!(record.status, JobStatus::Cancelled);
    }

    #[tokio::test]
    async fn test_cancel_running_job() {
        let queue = setup_test_queue();
        let job_id = queue.submit_job(test_spec("cancel-run")).await.unwrap();
        queue
            .mark_running(&job_id, "agent-1", "gpt-4.1", "openai")
            .await
            .unwrap();
        queue.cancel_job(&job_id).await.unwrap();

        let record = queue.get_job(&job_id).unwrap().unwrap();
        assert_eq!(record.status, JobStatus::Cancelled);
    }

    #[tokio::test]
    async fn test_cannot_cancel_completed() {
        let queue = setup_test_queue();
        let job_id = queue.submit_job(test_spec("done")).await.unwrap();
        queue
            .mark_running(&job_id, "agent-1", "gpt-4.1", "openai")
            .await
            .unwrap();
        queue.mark_completed(&job_id, "done", 1).await.unwrap();

        let result = queue.cancel_job(&job_id).await;
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn test_list_jobs_by_status() {
        let queue = setup_test_queue();
        queue.submit_job(test_spec("a")).await.unwrap();
        let id2 = queue.submit_job(test_spec("b")).await.unwrap();
        queue
            .mark_running(&id2, "agent-1", "gpt-4.1", "openai")
            .await
            .unwrap();

        let queued = queue.list_jobs(Some("queued"), 10).unwrap();
        assert_eq!(queued.len(), 1);

        let running = queue.list_jobs(Some("running"), 10).unwrap();
        assert_eq!(running.len(), 1);

        let all = queue.list_jobs(None, 10).unwrap();
        assert_eq!(all.len(), 2);
    }

    #[tokio::test]
    async fn test_topic_results() {
        let queue = setup_test_queue();
        let id1 = queue.submit_job(test_spec("research")).await.unwrap();
        let id2 = queue.submit_job(test_spec("research")).await.unwrap();

        queue.mark_running(&id1, "a1", "m1", "p1").await.unwrap();
        queue.mark_completed(&id1, "result-1", 2).await.unwrap();

        queue.mark_running(&id2, "a2", "m2", "p2").await.unwrap();
        queue.mark_completed(&id2, "result-2", 3).await.unwrap();

        let results = queue.topic_results("research", 10).unwrap();
        assert_eq!(results.len(), 2);
    }

    #[tokio::test]
    async fn test_priority_ordering() {
        let queue = setup_test_queue();

        let mut low = test_spec("prio");
        low.priority = JobPriority::Low;
        queue.submit_job(low).await.unwrap();

        let mut critical = test_spec("prio");
        critical.priority = JobPriority::Critical;
        queue.submit_job(critical).await.unwrap();

        let mut normal = test_spec("prio");
        normal.priority = JobPriority::Normal;
        queue.submit_job(normal).await.unwrap();

        // next_queued_job should return Critical first
        let next = queue.next_queued_job().unwrap().unwrap();
        assert_eq!(next.priority, JobPriority::Critical);
    }

    #[tokio::test]
    async fn test_queue_stats() {
        let queue = setup_test_queue();
        let id1 = queue.submit_job(test_spec("stats")).await.unwrap();
        queue.submit_job(test_spec("stats")).await.unwrap();
        queue.mark_running(&id1, "a", "m", "p").await.unwrap();

        let stats = queue.queue_stats().unwrap();
        assert_eq!(*stats.get("queued").unwrap_or(&0), 1);
        assert_eq!(*stats.get("running").unwrap_or(&0), 1);
    }

    #[tokio::test]
    async fn test_topic_all_terminal() {
        let queue = setup_test_queue();
        let id1 = queue.submit_job(test_spec("done-check")).await.unwrap();
        let id2 = queue.submit_job(test_spec("done-check")).await.unwrap();

        assert!(!queue.topic_all_terminal("done-check").unwrap());

        queue.mark_running(&id1, "a", "m", "p").await.unwrap();
        queue.mark_completed(&id1, "ok", 1).await.unwrap();

        assert!(!queue.topic_all_terminal("done-check").unwrap());

        queue.cancel_job(&id2).await.unwrap();

        assert!(queue.topic_all_terminal("done-check").unwrap());
    }

    #[tokio::test]
    async fn test_recover_running_jobs() {
        let queue = setup_test_queue();
        let id1 = queue.submit_job(test_spec("crash")).await.unwrap();
        let id2 = queue.submit_job(test_spec("crash")).await.unwrap();
        queue.mark_running(&id1, "a", "m", "p").await.unwrap();
        queue.mark_running(&id2, "a", "m", "p").await.unwrap();

        let recovered = queue.recover_running_jobs("daemon restarted").unwrap();
        assert_eq!(recovered, 2);

        let r1 = queue.get_job(&id1).unwrap().unwrap();
        assert_eq!(r1.status, JobStatus::Failed);
        assert_eq!(r1.error.as_deref(), Some("daemon restarted"));
    }

    #[tokio::test]
    async fn test_expire_stale_jobs() {
        let queue = setup_test_queue();

        // Submit a job with 0 TTL so it's already expired
        let mut spec = test_spec("expire");
        spec.ttl_seconds = 0;
        let job_id = queue.submit_job(spec).await.unwrap();

        // Small delay to ensure expiry
        tokio::time::sleep(std::time::Duration::from_millis(10)).await;

        let expired = queue.expire_stale_jobs().await.unwrap();
        assert_eq!(expired, 1);

        let record = queue.get_job(&job_id).unwrap().unwrap();
        assert_eq!(record.status, JobStatus::Expired);
    }

    #[tokio::test]
    async fn test_local_bus_events() {
        let queue = setup_test_queue();
        let mut rx = queue.subscribe_local();

        let _job_id = queue.submit_job(test_spec("events")).await.unwrap();

        // Should receive JobQueued event
        let event = rx.try_recv().unwrap();
        assert_eq!(event.event_type_str(), "job_queued");
        assert_eq!(event.topic(), "events");
    }

    #[tokio::test]
    async fn test_parent_job_dependency() {
        let queue = setup_test_queue();

        // Parent job
        let parent_id = queue.submit_job(test_spec("dep")).await.unwrap();

        // Child job depends on parent
        let mut child_spec = test_spec("dep");
        child_spec.parent_job_id = Some(parent_id.clone());
        let _child_id = queue.submit_job(child_spec).await.unwrap();

        // Child should NOT be picked up while parent is queued
        let next = queue.next_queued_job().unwrap().unwrap();
        assert_eq!(next.id, parent_id);

        // Complete the parent
        queue.mark_running(&parent_id, "a", "m", "p").await.unwrap();
        queue.mark_completed(&parent_id, "done", 1).await.unwrap();

        // Now child should be eligible
        let next = queue.next_queued_job().unwrap().unwrap();
        assert_ne!(next.id, parent_id);
        assert_eq!(next.parent_job_id.as_deref(), Some(parent_id.as_str()));
    }

    #[tokio::test]
    async fn test_get_nonexistent_job() {
        let queue = setup_test_queue();
        let result = queue.get_job("nonexistent").unwrap();
        assert!(result.is_none());
    }
}
