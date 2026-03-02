use crate::channel::TriggerDescriptor;
use crate::manifest::CapabilityDescriptor;
use crate::manifest::{SkillId, SkillManifest};
use crate::skill::{
    CostEstimate, ExecutionContext, HealthStatus, Skill, SkillConfig, SkillError, SkillHealth,
    SkillResult, ToolDescriptor, ToolOutput, ToolParams,
};
use abigail_queue::{JobPriority, JobRecord, JobSpec, RequiredCapability};
use async_trait::async_trait;
use std::collections::HashMap;
use std::sync::Arc;

#[async_trait]
pub trait QueueOperations: Send + Sync {
    async fn submit_job(&self, spec: JobSpec) -> Result<String, String>;
    async fn get_job(&self, job_id: &str) -> Result<Option<JobRecord>, String>;
    async fn list_jobs(&self, status: Option<&str>, limit: usize)
        -> Result<Vec<JobRecord>, String>;
    async fn cancel_job(&self, job_id: &str) -> Result<(), String>;
    async fn topic_results(&self, topic: &str, limit: usize) -> Result<Vec<JobRecord>, String>;
    async fn topic_all_terminal(&self, topic: &str) -> Result<bool, String>;
}

pub struct QueueManagementSkill {
    manifest: SkillManifest,
    ops: Arc<dyn QueueOperations>,
}

impl QueueManagementSkill {
    pub fn new(ops: Arc<dyn QueueOperations>) -> Self {
        let manifest = SkillManifest {
            id: SkillId("builtin.queue_management".to_string()),
            name: "Queue Management".to_string(),
            version: "0.1.0".to_string(),
            description: "Submit and manage async delegated jobs for sub-agents.".to_string(),
            license: Some("MIT".to_string()),
            category: "System".to_string(),
            keywords: vec![
                "queue".to_string(),
                "job".to_string(),
                "agent".to_string(),
                "topic".to_string(),
            ],
            runtime: "Native".to_string(),
            min_abigail_version: "0.1.0".to_string(),
            platforms: vec!["All".to_string()],
            capabilities: vec![],
            permissions: vec![],
            secrets: vec![],
            config_defaults: HashMap::new(),
        };

        Self { manifest, ops }
    }
}

#[async_trait]
impl Skill for QueueManagementSkill {
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
                name: "submit_job".to_string(),
                description: "Submit a new async job to the queue.".to_string(),
                parameters: serde_json::json!({
                    "type": "object",
                    "properties": {
                        "goal": { "type": "string" },
                        "topic": { "type": "string" },
                        "capability": { "type": "string" },
                        "priority": { "type": "string" },
                        "time_budget_ms": { "type": "integer" },
                        "max_turns": { "type": "integer" },
                        "system_context": { "type": "string" },
                        "allowed_skill_ids": { "type": "array", "items": { "type": "string" } },
                        "ttl_seconds": { "type": "integer" },
                        "input_data": {},
                        "parent_job_id": { "type": "string" }
                    },
                    "required": ["goal", "topic"]
                }),
                returns: serde_json::json!({ "type": "object" }),
                cost_estimate: CostEstimate::default(),
                required_permissions: vec![],
                autonomous: true,
                requires_confirmation: false,
            },
            ToolDescriptor {
                name: "check_job_status".to_string(),
                description: "Get the status/details for a queued job.".to_string(),
                parameters: serde_json::json!({
                    "type": "object",
                    "properties": { "job_id": { "type": "string" } },
                    "required": ["job_id"]
                }),
                returns: serde_json::json!({ "type": "object" }),
                cost_estimate: CostEstimate::default(),
                required_permissions: vec![],
                autonomous: true,
                requires_confirmation: false,
            },
            ToolDescriptor {
                name: "list_jobs".to_string(),
                description: "List queued/running/completed jobs with optional status filter."
                    .to_string(),
                parameters: serde_json::json!({
                    "type": "object",
                    "properties": {
                        "status": { "type": "string" },
                        "limit": { "type": "integer" }
                    }
                }),
                returns: serde_json::json!({ "type": "array" }),
                cost_estimate: CostEstimate::default(),
                required_permissions: vec![],
                autonomous: true,
                requires_confirmation: false,
            },
            ToolDescriptor {
                name: "cancel_job".to_string(),
                description: "Cancel a queued or running job.".to_string(),
                parameters: serde_json::json!({
                    "type": "object",
                    "properties": { "job_id": { "type": "string" } },
                    "required": ["job_id"]
                }),
                returns: serde_json::json!({ "type": "object" }),
                cost_estimate: CostEstimate::default(),
                required_permissions: vec![],
                autonomous: true,
                requires_confirmation: true,
            },
            ToolDescriptor {
                name: "list_topic_results".to_string(),
                description: "Get completed results for all jobs in a topic.".to_string(),
                parameters: serde_json::json!({
                    "type": "object",
                    "properties": {
                        "topic": { "type": "string" },
                        "limit": { "type": "integer" }
                    },
                    "required": ["topic"]
                }),
                returns: serde_json::json!({ "type": "object" }),
                cost_estimate: CostEstimate::default(),
                required_permissions: vec![],
                autonomous: true,
                requires_confirmation: false,
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
            "submit_job" => {
                let goal: String = params
                    .get_string("goal")
                    .ok_or_else(|| SkillError::ToolFailed("Missing 'goal'".to_string()))?;
                let topic: String = params
                    .get_string("topic")
                    .ok_or_else(|| SkillError::ToolFailed("Missing 'topic'".to_string()))?;
                let capability = parse_capability(params.get_string("capability").as_deref());
                let priority = parse_priority(params.get_string("priority").as_deref());
                let time_budget_ms = params.get::<u64>("time_budget_ms").unwrap_or(120_000);
                let max_turns = params.get::<u32>("max_turns").unwrap_or(10);
                let system_context = params.get_string("system_context");
                let allowed_skill_ids = params
                    .get::<Vec<String>>("allowed_skill_ids")
                    .unwrap_or_default();
                let ttl_seconds = params.get::<u64>("ttl_seconds").unwrap_or(3600);
                let input_data = params.values.get("input_data").cloned();
                let parent_job_id = params.get_string("parent_job_id");

                let spec = JobSpec {
                    goal,
                    topic: topic.clone(),
                    capability,
                    priority,
                    time_budget_ms,
                    max_turns,
                    system_context,
                    allowed_skill_ids,
                    ttl_seconds,
                    input_data,
                    parent_job_id,
                };
                let job_id = self
                    .ops
                    .submit_job(spec)
                    .await
                    .map_err(SkillError::ToolFailed)?;

                Ok(ToolOutput::success(serde_json::json!({
                    "job_id": job_id,
                    "topic": topic,
                    "status": "queued"
                })))
            }
            "check_job_status" => {
                let job_id: String = params
                    .get_string("job_id")
                    .ok_or_else(|| SkillError::ToolFailed("Missing 'job_id'".to_string()))?;
                let job = self
                    .ops
                    .get_job(&job_id)
                    .await
                    .map_err(SkillError::ToolFailed)?;
                match job {
                    Some(job) => Ok(ToolOutput::success(job_to_json(job))),
                    None => Err(SkillError::ToolFailed(format!(
                        "Job '{}' not found",
                        job_id
                    ))),
                }
            }
            "list_jobs" => {
                let status = params.get_string("status");
                let limit = params.get::<usize>("limit").unwrap_or(50);
                let jobs = self
                    .ops
                    .list_jobs(status.as_deref(), limit)
                    .await
                    .map_err(SkillError::ToolFailed)?;
                let jobs_json: Vec<serde_json::Value> = jobs.into_iter().map(job_to_json).collect();
                Ok(ToolOutput::success(jobs_json))
            }
            "cancel_job" => {
                let job_id: String = params
                    .get_string("job_id")
                    .ok_or_else(|| SkillError::ToolFailed("Missing 'job_id'".to_string()))?;
                self.ops
                    .cancel_job(&job_id)
                    .await
                    .map_err(SkillError::ToolFailed)?;
                Ok(ToolOutput::success(
                    serde_json::json!({"job_id": job_id, "status": "cancelled"}),
                ))
            }
            "list_topic_results" => {
                let topic: String = params
                    .get_string("topic")
                    .ok_or_else(|| SkillError::ToolFailed("Missing 'topic'".to_string()))?;
                let limit = params.get::<usize>("limit").unwrap_or(50);
                let jobs = self
                    .ops
                    .topic_results(&topic, limit)
                    .await
                    .map_err(SkillError::ToolFailed)?;
                let all_terminal = self
                    .ops
                    .topic_all_terminal(&topic)
                    .await
                    .map_err(SkillError::ToolFailed)?;
                let jobs_json: Vec<serde_json::Value> = jobs.into_iter().map(job_to_json).collect();
                Ok(ToolOutput::success(serde_json::json!({
                    "topic": topic,
                    "all_terminal": all_terminal,
                    "jobs": jobs_json
                })))
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

    fn get_capability(&self, _cap_type: &str) -> Option<&dyn std::any::Any> {
        None
    }

    fn triggers(&self) -> Vec<TriggerDescriptor> {
        vec![]
    }
}

fn parse_capability(value: Option<&str>) -> RequiredCapability {
    value
        .map(RequiredCapability::from_str_lossy)
        .unwrap_or(RequiredCapability::General)
}

fn parse_priority(value: Option<&str>) -> JobPriority {
    match value.unwrap_or("normal").to_ascii_lowercase().as_str() {
        "low" => JobPriority::Low,
        "high" => JobPriority::High,
        "critical" => JobPriority::Critical,
        _ => JobPriority::Normal,
    }
}

fn job_to_json(job: JobRecord) -> serde_json::Value {
    serde_json::json!({
        "id": job.id,
        "topic": job.topic,
        "goal": job.goal,
        "capability": job.capability.as_str(),
        "priority": match job.priority {
            JobPriority::Low => "low",
            JobPriority::Normal => "normal",
            JobPriority::High => "high",
            JobPriority::Critical => "critical",
        },
        "status": job.status.as_str(),
        "time_budget_ms": job.time_budget_ms,
        "max_turns": job.max_turns,
        "system_context": job.system_context,
        "allowed_skill_ids": job.allowed_skill_ids,
        "input_data": job.input_data,
        "parent_job_id": job.parent_job_id,
        "agent_id": job.agent_id,
        "model_used": job.model_used,
        "provider_used": job.provider_used,
        "result": job.result,
        "error": job.error,
        "turns_consumed": job.turns_consumed,
        "ttl_seconds": job.ttl_seconds,
        "created_at": job.created_at,
        "started_at": job.started_at,
        "completed_at": job.completed_at,
        "expires_at": job.expires_at
    })
}
