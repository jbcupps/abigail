//! Core types for the job queue system.

use serde::{Deserialize, Serialize};

/// Unique job identifier (UUID string).
pub type JobId = String;

/// Topic identifier — groups related jobs for batch retrieval.
pub type TopicId = String;

/// Required capability for a job, used by CapabilityMatcher to select the
/// optimal LLM model.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum RequiredCapability {
    /// General-purpose tasks.
    General,
    /// Code generation, analysis, debugging.
    Code,
    /// Image analysis, visual understanding.
    Vision,
    /// Complex reasoning, chain-of-thought.
    Reasoning,
    /// Web search, information retrieval.
    Search,
    /// Custom capability identified by name.
    Custom(String),
}

impl RequiredCapability {
    pub fn as_str(&self) -> &str {
        match self {
            RequiredCapability::General => "general",
            RequiredCapability::Code => "code",
            RequiredCapability::Vision => "vision",
            RequiredCapability::Reasoning => "reasoning",
            RequiredCapability::Search => "search",
            RequiredCapability::Custom(s) => s.as_str(),
        }
    }

    pub fn from_str_lossy(s: &str) -> Self {
        match s {
            "general" => RequiredCapability::General,
            "code" => RequiredCapability::Code,
            "vision" => RequiredCapability::Vision,
            "reasoning" => RequiredCapability::Reasoning,
            "search" => RequiredCapability::Search,
            other => RequiredCapability::Custom(other.to_string()),
        }
    }
}

/// Job priority level.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, PartialOrd, Ord)]
#[serde(rename_all = "snake_case")]
pub enum JobPriority {
    Low = 0,
    Normal = 1,
    High = 2,
    Critical = 3,
}

impl JobPriority {
    pub fn as_i32(&self) -> i32 {
        *self as i32
    }

    pub fn from_i32(v: i32) -> Self {
        match v {
            0 => JobPriority::Low,
            2 => JobPriority::High,
            3 => JobPriority::Critical,
            _ => JobPriority::Normal,
        }
    }
}

/// Lifecycle status of a job.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum JobStatus {
    Queued,
    Running,
    Completed,
    Failed,
    Cancelled,
    Expired,
}

impl JobStatus {
    pub fn as_str(&self) -> &str {
        match self {
            JobStatus::Queued => "queued",
            JobStatus::Running => "running",
            JobStatus::Completed => "completed",
            JobStatus::Failed => "failed",
            JobStatus::Cancelled => "cancelled",
            JobStatus::Expired => "expired",
        }
    }

    pub fn from_str_lossy(s: &str) -> Self {
        match s {
            "queued" => JobStatus::Queued,
            "running" => JobStatus::Running,
            "completed" => JobStatus::Completed,
            "failed" => JobStatus::Failed,
            "cancelled" => JobStatus::Cancelled,
            "expired" => JobStatus::Expired,
            _ => JobStatus::Queued,
        }
    }

    /// Whether this is a terminal state (no further transitions).
    pub fn is_terminal(&self) -> bool {
        matches!(
            self,
            JobStatus::Completed | JobStatus::Failed | JobStatus::Cancelled | JobStatus::Expired
        )
    }
}

/// What the entity submits to create a new job.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct JobSpec {
    /// What the job should accomplish.
    pub goal: String,
    /// Groups related jobs for batch result retrieval.
    pub topic: TopicId,
    /// Required capability for model selection.
    #[serde(default = "default_capability")]
    pub capability: RequiredCapability,
    /// Priority level.
    #[serde(default = "default_priority")]
    pub priority: JobPriority,
    /// Max execution time in milliseconds (default 120s).
    #[serde(default = "default_time_budget")]
    pub time_budget_ms: u64,
    /// AgenticEngine turn limit (default 10).
    #[serde(default = "default_max_turns")]
    pub max_turns: u32,
    /// Custom system prompt for the sub-agent.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub system_context: Option<String>,
    /// Restrict which tools the agent can use.
    #[serde(default)]
    pub allowed_skill_ids: Vec<String>,
    /// TTL in seconds — expires if not picked up (default 1 hour).
    #[serde(default = "default_ttl")]
    pub ttl_seconds: u64,
    /// Context blob (file path, URL, structured data, etc.).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub input_data: Option<serde_json::Value>,
    /// Parent job for chaining/dependency.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub parent_job_id: Option<JobId>,
}

fn default_capability() -> RequiredCapability {
    RequiredCapability::General
}
fn default_priority() -> JobPriority {
    JobPriority::Normal
}
fn default_time_budget() -> u64 {
    120_000
}
fn default_max_turns() -> u32 {
    10
}
fn default_ttl() -> u64 {
    3600
}

/// Full job record as stored in SQLite.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct JobRecord {
    pub id: JobId,
    pub topic: TopicId,
    pub goal: String,
    pub capability: RequiredCapability,
    pub priority: JobPriority,
    pub status: JobStatus,
    pub time_budget_ms: u64,
    pub max_turns: u32,
    pub system_context: Option<String>,
    pub allowed_skill_ids: Vec<String>,
    pub input_data: Option<serde_json::Value>,
    pub parent_job_id: Option<JobId>,
    pub agent_id: Option<String>,
    pub model_used: Option<String>,
    pub provider_used: Option<String>,
    pub result: Option<String>,
    pub error: Option<String>,
    pub turns_consumed: u32,
    pub ttl_seconds: u64,
    pub created_at: String,
    pub started_at: Option<String>,
    pub completed_at: Option<String>,
    pub expires_at: String,
}

/// Real-time event published when job state changes.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "event_type", rename_all = "snake_case")]
pub enum JobEvent {
    JobQueued {
        job_id: JobId,
        topic: TopicId,
        capability: RequiredCapability,
        priority: JobPriority,
    },
    JobStarted {
        job_id: JobId,
        topic: TopicId,
        agent_id: String,
        model_used: String,
    },
    JobProgress {
        job_id: JobId,
        topic: TopicId,
        turns_consumed: u32,
        message: String,
    },
    JobCompleted {
        job_id: JobId,
        topic: TopicId,
        result: String,
        turns_consumed: u32,
    },
    JobFailed {
        job_id: JobId,
        topic: TopicId,
        error: String,
        turns_consumed: u32,
    },
    JobCancelled {
        job_id: JobId,
        topic: TopicId,
    },
    JobExpired {
        job_id: JobId,
        topic: TopicId,
    },
}

impl JobEvent {
    pub fn job_id(&self) -> &str {
        match self {
            JobEvent::JobQueued { job_id, .. }
            | JobEvent::JobStarted { job_id, .. }
            | JobEvent::JobProgress { job_id, .. }
            | JobEvent::JobCompleted { job_id, .. }
            | JobEvent::JobFailed { job_id, .. }
            | JobEvent::JobCancelled { job_id, .. }
            | JobEvent::JobExpired { job_id, .. } => job_id,
        }
    }

    pub fn topic(&self) -> &str {
        match self {
            JobEvent::JobQueued { topic, .. }
            | JobEvent::JobStarted { topic, .. }
            | JobEvent::JobProgress { topic, .. }
            | JobEvent::JobCompleted { topic, .. }
            | JobEvent::JobFailed { topic, .. }
            | JobEvent::JobCancelled { topic, .. }
            | JobEvent::JobExpired { topic, .. } => topic,
        }
    }

    pub fn event_type_str(&self) -> &str {
        match self {
            JobEvent::JobQueued { .. } => "job_queued",
            JobEvent::JobStarted { .. } => "job_started",
            JobEvent::JobProgress { .. } => "job_progress",
            JobEvent::JobCompleted { .. } => "job_completed",
            JobEvent::JobFailed { .. } => "job_failed",
            JobEvent::JobCancelled { .. } => "job_cancelled",
            JobEvent::JobExpired { .. } => "job_expired",
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_job_status_terminal() {
        assert!(!JobStatus::Queued.is_terminal());
        assert!(!JobStatus::Running.is_terminal());
        assert!(JobStatus::Completed.is_terminal());
        assert!(JobStatus::Failed.is_terminal());
        assert!(JobStatus::Cancelled.is_terminal());
        assert!(JobStatus::Expired.is_terminal());
    }

    #[test]
    fn test_job_priority_ordering() {
        assert!(JobPriority::Low < JobPriority::Normal);
        assert!(JobPriority::Normal < JobPriority::High);
        assert!(JobPriority::High < JobPriority::Critical);
    }

    #[test]
    fn test_required_capability_roundtrip() {
        let caps = vec![
            RequiredCapability::General,
            RequiredCapability::Code,
            RequiredCapability::Vision,
            RequiredCapability::Reasoning,
            RequiredCapability::Search,
            RequiredCapability::Custom("my_cap".into()),
        ];
        for cap in caps {
            let s = cap.as_str();
            let back = RequiredCapability::from_str_lossy(s);
            assert_eq!(cap, back);
        }
    }

    #[test]
    fn test_job_event_accessors() {
        let event = JobEvent::JobCompleted {
            job_id: "job-1".into(),
            topic: "research".into(),
            result: "found it".into(),
            turns_consumed: 3,
        };
        assert_eq!(event.job_id(), "job-1");
        assert_eq!(event.topic(), "research");
        assert_eq!(event.event_type_str(), "job_completed");
    }

    #[test]
    fn test_job_spec_serde() {
        let spec = JobSpec {
            goal: "Research quantum computing".into(),
            topic: "research-quantum".into(),
            capability: RequiredCapability::Reasoning,
            priority: JobPriority::High,
            time_budget_ms: 60000,
            max_turns: 5,
            system_context: Some("You are a research assistant.".into()),
            allowed_skill_ids: vec!["web-search".into()],
            ttl_seconds: 1800,
            input_data: None,
            parent_job_id: None,
        };

        let json = serde_json::to_string(&spec).unwrap();
        let back: JobSpec = serde_json::from_str(&json).unwrap();
        assert_eq!(back.goal, "Research quantum computing");
        assert_eq!(back.capability, RequiredCapability::Reasoning);
        assert_eq!(back.priority, JobPriority::High);
    }
}
