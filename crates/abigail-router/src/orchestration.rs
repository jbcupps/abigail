//! Orchestration scheduler — cron-based job scheduling for agentic runs.

use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Arc;
use tokio::sync::RwLock;

/// An orchestration job that can be scheduled to run periodically.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OrchestrationJob {
    /// Unique job identifier.
    pub job_id: String,
    /// Human-readable name.
    pub name: String,
    /// Cron expression (UTC). E.g. "0 */6 * * *" for every 6 hours.
    pub cron_expression: String,
    /// Execution mode.
    pub mode: JobMode,
    /// Goal template (used when mode is AgenticRun).
    pub goal_template: Option<String>,
    /// Whether the job is enabled.
    pub enabled: bool,
    /// Significance policy for deciding how to handle results.
    #[serde(default)]
    pub significance_policy: SignificancePolicy,
    /// ISO 8601 timestamp of creation.
    pub created_at: String,
    /// ISO 8601 timestamp of last modification.
    pub updated_at: String,
}

/// How the job executes.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum JobMode {
    /// Quick Id check — uses local LLM for a simple assessment.
    IdCheck,
    /// Full agentic run with governor.
    AgenticRun,
}

/// How to assess and react to job results.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct SignificancePolicy {
    /// Keywords that indicate significance (e.g. "urgent", "error", "alert").
    #[serde(default)]
    pub keywords: Vec<String>,
    /// Minimum significance score (0.0–1.0) to trigger notification.
    #[serde(default = "default_threshold")]
    pub threshold: f32,
}

fn default_threshold() -> f32 {
    0.5
}

/// What to do with a job's result based on significance scoring.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SignificanceDecision {
    /// Low significance — just log it silently.
    SilentLog,
    /// Medium significance — spawn an agentic run to handle it.
    SpawnAgentic,
    /// High significance — flag the mentor for attention.
    FlagMentor,
}

/// Log entry for a completed orchestration job run.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OrchestrationJobLog {
    /// Job ID that generated this log.
    pub job_id: String,
    /// Unique run ID.
    pub run_id: String,
    /// ISO 8601 timestamp of when the job ran.
    pub ran_at: String,
    /// Result summary.
    pub result: String,
    /// Significance decision made.
    pub decision: SignificanceDecision,
    /// Duration in milliseconds.
    pub duration_ms: u64,
}

/// Manages orchestration jobs and their execution.
pub struct OrchestrationScheduler {
    jobs: Arc<RwLock<Vec<OrchestrationJob>>>,
    logs: Arc<RwLock<Vec<OrchestrationJobLog>>>,
    jobs_path: PathBuf,
    logs_path: PathBuf,
}

impl OrchestrationScheduler {
    /// Create a new scheduler, loading persisted state from the data directory.
    pub fn new(data_dir: PathBuf) -> Self {
        let jobs_path = data_dir.join("orchestration_jobs.json");
        let logs_path = data_dir.join("orchestration_job_logs.json");

        let jobs = if jobs_path.exists() {
            match std::fs::read_to_string(&jobs_path) {
                Ok(content) => serde_json::from_str(&content).unwrap_or_default(),
                Err(_) => Vec::new(),
            }
        } else {
            Vec::new()
        };

        let logs = if logs_path.exists() {
            match std::fs::read_to_string(&logs_path) {
                Ok(content) => serde_json::from_str(&content).unwrap_or_default(),
                Err(_) => Vec::new(),
            }
        } else {
            Vec::new()
        };

        Self {
            jobs: Arc::new(RwLock::new(jobs)),
            logs: Arc::new(RwLock::new(logs)),
            jobs_path,
            logs_path,
        }
    }

    /// List all jobs.
    pub async fn list_jobs(&self) -> Vec<OrchestrationJob> {
        self.jobs.read().await.clone()
    }

    /// Create a new job.
    pub async fn create_job(&self, job: OrchestrationJob) -> anyhow::Result<String> {
        let job_id = job.job_id.clone();
        {
            let mut jobs = self.jobs.write().await;
            jobs.push(job);
        }
        self.save_jobs().await?;
        Ok(job_id)
    }

    /// Update an existing job.
    pub async fn update_job(&self, job_id: &str, update: OrchestrationJob) -> anyhow::Result<()> {
        let mut jobs = self.jobs.write().await;
        if let Some(existing) = jobs.iter_mut().find(|j| j.job_id == job_id) {
            *existing = update;
        } else {
            anyhow::bail!("Job not found: {}", job_id);
        }
        drop(jobs);
        self.save_jobs().await
    }

    /// Delete a job.
    pub async fn delete_job(&self, job_id: &str) -> anyhow::Result<()> {
        let mut jobs = self.jobs.write().await;
        let before = jobs.len();
        jobs.retain(|j| j.job_id != job_id);
        if jobs.len() == before {
            anyhow::bail!("Job not found: {}", job_id);
        }
        drop(jobs);
        self.save_jobs().await
    }

    /// Enable or disable a job.
    pub async fn set_enabled(&self, job_id: &str, enabled: bool) -> anyhow::Result<()> {
        let mut jobs = self.jobs.write().await;
        if let Some(job) = jobs.iter_mut().find(|j| j.job_id == job_id) {
            job.enabled = enabled;
            job.updated_at = chrono::Utc::now().to_rfc3339();
        } else {
            anyhow::bail!("Job not found: {}", job_id);
        }
        drop(jobs);
        self.save_jobs().await
    }

    /// Get logs for all jobs or a specific job.
    pub async fn get_logs(&self, job_id: Option<&str>) -> Vec<OrchestrationJobLog> {
        let logs = self.logs.read().await;
        match job_id {
            Some(id) => logs.iter().filter(|l| l.job_id == id).cloned().collect(),
            None => logs.clone(),
        }
    }

    /// Record a job execution log.
    pub async fn record_log(&self, log: OrchestrationJobLog) -> anyhow::Result<()> {
        {
            let mut logs = self.logs.write().await;
            logs.push(log);
            // Keep last 1000 logs
            if logs.len() > 1000 {
                let drain_count = logs.len() - 1000;
                logs.drain(..drain_count);
            }
        }
        self.save_logs().await
    }

    /// Score the significance of a result based on the job's policy.
    pub fn score_significance(
        result: &str,
        policy: &SignificancePolicy,
    ) -> (f32, SignificanceDecision) {
        let lower = result.to_lowercase();
        let mut score: f32 = 0.0;

        // Keyword matching
        for keyword in &policy.keywords {
            if lower.contains(&keyword.to_lowercase()) {
                score += 0.3;
            }
        }

        // Built-in significance indicators
        let urgent_keywords = ["urgent", "error", "failure", "critical", "alert", "warning"];
        for kw in &urgent_keywords {
            if lower.contains(kw) {
                score += 0.2;
            }
        }

        score = score.min(1.0);

        let decision = if score >= 0.8 {
            SignificanceDecision::FlagMentor
        } else if score >= policy.threshold {
            SignificanceDecision::SpawnAgentic
        } else {
            SignificanceDecision::SilentLog
        };

        (score, decision)
    }

    /// Check which jobs are due to run now.
    /// Returns job IDs that should be triggered.
    pub async fn check_due_jobs(&self) -> Vec<String> {
        // Simple implementation: check if any enabled jobs match the current minute
        // A full cron parser would be used in production
        let jobs = self.jobs.read().await;
        jobs.iter()
            .filter(|j| j.enabled)
            .map(|j| j.job_id.clone())
            .collect()
    }

    async fn save_jobs(&self) -> anyhow::Result<()> {
        let jobs = self.jobs.read().await;
        let content = serde_json::to_string_pretty(&*jobs)?;
        if let Some(parent) = self.jobs_path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        std::fs::write(&self.jobs_path, content)?;
        Ok(())
    }

    async fn save_logs(&self) -> anyhow::Result<()> {
        let logs = self.logs.read().await;
        let content = serde_json::to_string_pretty(&*logs)?;
        std::fs::write(&self.logs_path, content)?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_significance_scoring_low() {
        let policy = SignificancePolicy {
            keywords: vec!["important".into()],
            threshold: 0.5,
        };
        let (score, decision) = OrchestrationScheduler::score_significance(
            "Nothing interesting happened today",
            &policy,
        );
        assert!(score < 0.5);
        assert_eq!(decision, SignificanceDecision::SilentLog);
    }

    #[test]
    fn test_significance_scoring_high() {
        let policy = SignificancePolicy {
            keywords: vec!["important".into()],
            threshold: 0.5,
        };
        let (score, decision) = OrchestrationScheduler::score_significance(
            "URGENT ALERT: critical error detected, this is important",
            &policy,
        );
        assert!(score >= 0.8);
        assert_eq!(decision, SignificanceDecision::FlagMentor);
    }

    #[test]
    fn test_significance_scoring_medium() {
        let policy = SignificancePolicy {
            keywords: vec!["deploy".into()],
            threshold: 0.3,
        };
        let (_, decision) = OrchestrationScheduler::score_significance(
            "New deploy available with a warning",
            &policy,
        );
        assert_eq!(decision, SignificanceDecision::SpawnAgentic);
    }

    #[tokio::test]
    async fn test_scheduler_crud() {
        let tmp = std::env::temp_dir().join("abigail_orch_test");
        let _ = std::fs::remove_dir_all(&tmp);
        std::fs::create_dir_all(&tmp).unwrap();

        let scheduler = OrchestrationScheduler::new(tmp.clone());

        let now = chrono::Utc::now().to_rfc3339();
        let job = OrchestrationJob {
            job_id: "job-1".into(),
            name: "Test Job".into(),
            cron_expression: "0 * * * *".into(),
            mode: JobMode::IdCheck,
            goal_template: None,
            enabled: true,
            significance_policy: SignificancePolicy::default(),
            created_at: now.clone(),
            updated_at: now,
        };

        scheduler.create_job(job).await.unwrap();
        let jobs = scheduler.list_jobs().await;
        assert_eq!(jobs.len(), 1);
        assert_eq!(jobs[0].name, "Test Job");

        scheduler.set_enabled("job-1", false).await.unwrap();
        let jobs = scheduler.list_jobs().await;
        assert!(!jobs[0].enabled);

        scheduler.delete_job("job-1").await.unwrap();
        let jobs = scheduler.list_jobs().await;
        assert!(jobs.is_empty());

        let _ = std::fs::remove_dir_all(&tmp);
    }
}
