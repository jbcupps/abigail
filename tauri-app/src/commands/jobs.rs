//! Tauri commands for the async job queue.

use crate::state::AppState;
use abigail_queue::{JobPriority, JobSpec, RequiredCapability};
use serde::{Deserialize, Serialize};
use tauri::State;

#[derive(Debug, Deserialize)]
pub struct SubmitJobArgs {
    pub goal: String,
    pub topic: String,
    pub capability: Option<String>,
    pub priority: Option<String>,
    pub time_budget_ms: Option<u64>,
    pub max_turns: Option<u32>,
    pub ttl_seconds: Option<u64>,
    pub system_context: Option<String>,
    pub input_data: Option<serde_json::Value>,
    pub parent_job_id: Option<String>,
    /// Cron expression for recurring jobs (e.g. "0 */6 * * *").
    pub cron_expression: Option<String>,
    /// If true, creates a recurring job template instead of a one-shot job.
    pub is_recurring: Option<bool>,
    /// Goal template for recurring jobs (interpolated at scheduling time).
    pub goal_template: Option<String>,
}

#[derive(Debug, Serialize)]
pub struct SubmitJobResult {
    pub job_id: String,
    pub topic: String,
    pub status: String,
}

#[derive(Debug, Serialize)]
pub struct JobRecord {
    pub id: String,
    pub topic: String,
    pub goal: String,
    pub status: String,
    pub priority: String,
    pub capability: String,
    pub is_recurring: bool,
    pub cron_expression: Option<String>,
    pub result: Option<String>,
    pub error: Option<String>,
    pub created_at: String,
    pub started_at: Option<String>,
    pub completed_at: Option<String>,
}

fn to_record(j: abigail_queue::JobRecord) -> JobRecord {
    JobRecord {
        id: j.id,
        topic: j.topic,
        goal: j.goal,
        status: j.status.as_str().to_string(),
        priority: match j.priority {
            JobPriority::Low => "low",
            JobPriority::Normal => "normal",
            JobPriority::High => "high",
            JobPriority::Critical => "critical",
        }
        .to_string(),
        capability: j.capability.as_str().to_string(),
        is_recurring: j.is_recurring,
        cron_expression: j.cron_expression,
        result: j.result,
        error: j.error,
        created_at: j.created_at,
        started_at: j.started_at,
        completed_at: j.completed_at,
    }
}

fn parse_priority(s: Option<&str>) -> JobPriority {
    match s.unwrap_or("normal").to_ascii_lowercase().as_str() {
        "low" => JobPriority::Low,
        "high" => JobPriority::High,
        "critical" => JobPriority::Critical,
        _ => JobPriority::Normal,
    }
}

fn parse_capability(s: Option<&str>) -> RequiredCapability {
    s.map(RequiredCapability::from_str_lossy)
        .unwrap_or(RequiredCapability::General)
}

/// Submit a new job (one-shot or recurring template).
#[tauri::command]
pub async fn submit_job(
    state: State<'_, AppState>,
    args: SubmitJobArgs,
) -> Result<SubmitJobResult, String> {
    let spec = JobSpec {
        goal: args.goal,
        topic: args.topic.clone(),
        capability: parse_capability(args.capability.as_deref()),
        priority: parse_priority(args.priority.as_deref()),
        time_budget_ms: args.time_budget_ms.unwrap_or(120_000),
        max_turns: args.max_turns.unwrap_or(10),
        system_context: args.system_context,
        allowed_skill_ids: vec![],
        ttl_seconds: args.ttl_seconds.unwrap_or(3600),
        input_data: args.input_data,
        parent_job_id: args.parent_job_id,
        cron_expression: args.cron_expression,
        is_recurring: args.is_recurring.unwrap_or(false),
        significance_keywords: vec![],
        significance_threshold: 0.5,
        job_mode: "agentic_run".into(),
        goal_template: args.goal_template,
        depends_on: vec![],
    };

    let job_id = state
        .job_queue
        .submit_job(spec)
        .await
        .map_err(|e| e.to_string())?;

    Ok(SubmitJobResult {
        job_id,
        topic: args.topic,
        status: "queued".to_string(),
    })
}

/// List jobs, optionally filtered by status.
#[tauri::command]
pub async fn list_jobs(
    state: State<'_, AppState>,
    status: Option<String>,
    limit: Option<usize>,
) -> Result<Vec<JobRecord>, String> {
    let jobs = state
        .job_queue
        .list_jobs(status.as_deref(), limit.unwrap_or(50))
        .map_err(|e| e.to_string())?;
    Ok(jobs.into_iter().map(to_record).collect())
}

/// Get details for a specific job.
#[tauri::command]
pub async fn get_job_status(
    state: State<'_, AppState>,
    job_id: String,
) -> Result<JobRecord, String> {
    state
        .job_queue
        .get_job(&job_id)
        .map_err(|e| e.to_string())?
        .map(to_record)
        .ok_or_else(|| format!("Job '{}' not found", job_id))
}

/// Cancel a queued or running job.
#[tauri::command]
pub async fn cancel_job(
    state: State<'_, AppState>,
    job_id: String,
) -> Result<serde_json::Value, String> {
    state
        .job_queue
        .cancel_job(&job_id)
        .await
        .map_err(|e| e.to_string())?;
    Ok(serde_json::json!({ "job_id": job_id, "status": "cancelled" }))
}

/// List recurring job templates.
#[tauri::command]
pub async fn list_recurring_templates(
    state: State<'_, AppState>,
) -> Result<Vec<JobRecord>, String> {
    let templates = state
        .job_queue
        .get_recurring_templates()
        .map_err(|e| e.to_string())?;
    Ok(templates.into_iter().map(to_record).collect())
}
