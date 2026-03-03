//! Built-in job delegation tools for the entity ego.
//!
//! These tools are injected into the LLM tool-use loop so the ego can
//! submit background work, check on results, and list active jobs without
//! leaving the conversation.

use abigail_capabilities::cognitive::ToolDefinition;
use abigail_queue::{DirectToolCall, ExecutionMode, JobQueue, JobSpec, RequiredCapability};
use serde_json::json;
use std::str::FromStr;
use std::sync::Arc;

pub const TOOL_SUBMIT_JOB: &str = "builtin.jobs::submit_background_job";
pub const TOOL_GET_RESULT: &str = "builtin.jobs::get_job_result";
pub const TOOL_LIST_JOBS: &str = "builtin.jobs::list_my_jobs";
pub const TOOL_CREATE_RECURRING: &str = "builtin.jobs::create_recurring_job";
pub const TOOL_LIST_RECURRING: &str = "builtin.jobs::list_recurring_jobs";
pub const TOOL_CANCEL_RECURRING: &str = "builtin.jobs::cancel_recurring_job";

pub fn job_tool_definitions() -> Vec<ToolDefinition> {
    vec![
        ToolDefinition {
            name: TOOL_SUBMIT_JOB.into(),
            description: "Submit a background job for async execution. Use this to delegate \
                tasks like image generation, research, or code analysis to a background agent. \
                Returns a job_id you can check later with get_job_result."
                .into(),
            parameters: json!({
                "type": "object",
                "properties": {
                    "goal": {
                        "type": "string",
                        "description": "What the background job should accomplish"
                    },
                    "capability": {
                        "type": "string",
                        "enum": ["general", "code", "vision", "reasoning", "search",
                                 "image_generation", "audio_generation", "video_generation",
                                 "transcription"],
                        "description": "Required capability for model selection"
                    },
                    "topic": {
                        "type": "string",
                        "description": "Topic grouping for this job (e.g. 'image-tasks', 'research')"
                    },
                    "priority": {
                        "type": "string",
                        "enum": ["low", "normal", "high", "critical"],
                        "description": "Job priority level"
                    },
                    "execution_mode": {
                        "type": "string",
                        "enum": ["mediated", "direct"],
                        "description": "mediated = LLM agent loop, direct = execute skill tool directly"
                    },
                    "direct_skill_id": {
                        "type": "string",
                        "description": "Skill ID for direct execution (required when execution_mode=direct)"
                    },
                    "direct_tool_name": {
                        "type": "string",
                        "description": "Tool name for direct execution (required when execution_mode=direct)"
                    },
                    "direct_params": {
                        "type": "object",
                        "description": "Parameters for the direct tool call"
                    }
                },
                "required": ["goal", "capability", "topic"]
            }),
        },
        ToolDefinition {
            name: TOOL_GET_RESULT.into(),
            description: "Check the status and result of a previously submitted background job."
                .into(),
            parameters: json!({
                "type": "object",
                "properties": {
                    "job_id": {
                        "type": "string",
                        "description": "The job ID returned from submit_background_job"
                    }
                },
                "required": ["job_id"]
            }),
        },
        ToolDefinition {
            name: TOOL_LIST_JOBS.into(),
            description: "List background jobs, optionally filtered by status.".into(),
            parameters: json!({
                "type": "object",
                "properties": {
                    "status": {
                        "type": "string",
                        "enum": ["queued", "running", "completed", "failed"],
                        "description": "Filter by job status (omit for all)"
                    },
                    "limit": {
                        "type": "integer",
                        "description": "Maximum number of jobs to return (default 10)"
                    }
                }
            }),
        },
        ToolDefinition {
            name: TOOL_CREATE_RECURRING.into(),
            description: "Create a recurring job that runs on a cron schedule. \
                Use this for autonomous scheduled work (e.g. daily email check, weekly summaries)."
                .into(),
            parameters: json!({
                "type": "object",
                "properties": {
                    "goal": {
                        "type": "string",
                        "description": "What each recurring instance should accomplish"
                    },
                    "goal_template": {
                        "type": "string",
                        "description": "Goal template with {date} and {time} placeholders for interpolation"
                    },
                    "cron_expression": {
                        "type": "string",
                        "description": "Cron expression in UTC (e.g. '0 8 * * *' for daily at 8 AM)"
                    },
                    "capability": {
                        "type": "string",
                        "enum": ["general", "code", "vision", "reasoning", "search",
                                 "image_generation", "audio_generation", "video_generation",
                                 "transcription"],
                        "description": "Required capability for model selection"
                    },
                    "topic": {
                        "type": "string",
                        "description": "Topic grouping for recurring instances"
                    }
                },
                "required": ["goal", "cron_expression", "capability", "topic"]
            }),
        },
        ToolDefinition {
            name: TOOL_LIST_RECURRING.into(),
            description: "List all active recurring job schedules.".into(),
            parameters: json!({
                "type": "object",
                "properties": {}
            }),
        },
        ToolDefinition {
            name: TOOL_CANCEL_RECURRING.into(),
            description: "Cancel a recurring job schedule by its template ID.".into(),
            parameters: json!({
                "type": "object",
                "properties": {
                    "job_id": {
                        "type": "string",
                        "description": "The recurring template job ID to cancel"
                    }
                },
                "required": ["job_id"]
            }),
        },
    ]
}

/// Returns true if the tool name is a built-in job tool.
pub fn is_job_tool(name: &str) -> bool {
    matches!(
        name,
        TOOL_SUBMIT_JOB
            | TOOL_GET_RESULT
            | TOOL_LIST_JOBS
            | TOOL_CREATE_RECURRING
            | TOOL_LIST_RECURRING
            | TOOL_CANCEL_RECURRING
    )
}

/// Execute a built-in job tool call. Returns the JSON result string.
pub async fn execute_job_tool(
    queue: &Arc<JobQueue>,
    tool_name: &str,
    args: &serde_json::Value,
) -> String {
    match tool_name {
        TOOL_SUBMIT_JOB => execute_submit(queue, args).await,
        TOOL_GET_RESULT => execute_get_result(queue, args).await,
        TOOL_LIST_JOBS => execute_list(queue, args).await,
        TOOL_CREATE_RECURRING => execute_create_recurring(queue, args).await,
        TOOL_LIST_RECURRING => execute_list_recurring(queue).await,
        TOOL_CANCEL_RECURRING => execute_cancel_recurring(queue, args).await,
        _ => json!({"error": format!("Unknown job tool: {}", tool_name)}).to_string(),
    }
}

async fn execute_submit(queue: &Arc<JobQueue>, args: &serde_json::Value) -> String {
    let goal = args["goal"].as_str().unwrap_or("").to_string();
    let capability_str = args["capability"].as_str().unwrap_or("general");
    let topic = args["topic"].as_str().unwrap_or("delegation").to_string();
    let priority_str = args["priority"].as_str().unwrap_or("normal");
    let exec_mode_str = args["execution_mode"].as_str().unwrap_or("mediated");

    let capability = RequiredCapability::from_str_lossy(capability_str);
    let priority = match priority_str {
        "low" => abigail_queue::JobPriority::Low,
        "high" => abigail_queue::JobPriority::High,
        "critical" => abigail_queue::JobPriority::Critical,
        _ => abigail_queue::JobPriority::Normal,
    };

    let execution_mode = match exec_mode_str {
        "direct" => ExecutionMode::Direct,
        _ => ExecutionMode::Mediated,
    };

    let direct_tool_call = if execution_mode == ExecutionMode::Direct {
        let skill_id = args["direct_skill_id"].as_str().unwrap_or("").to_string();
        let tool_name = args["direct_tool_name"].as_str().unwrap_or("").to_string();
        let params = args["direct_params"].clone();
        if skill_id.is_empty() || tool_name.is_empty() {
            return json!({"error": "direct execution requires direct_skill_id and direct_tool_name"}).to_string();
        }
        Some(DirectToolCall {
            skill_id,
            tool_name,
            params,
        })
    } else {
        None
    };

    let spec = JobSpec {
        goal,
        topic: topic.clone(),
        capability,
        priority,
        time_budget_ms: 120_000,
        max_turns: 10,
        system_context: None,
        allowed_skill_ids: vec![],
        ttl_seconds: 3600,
        input_data: None,
        parent_job_id: None,
        cron_expression: None,
        is_recurring: false,
        significance_keywords: vec![],
        significance_threshold: 0.5,
        job_mode: "agentic_run".into(),
        goal_template: None,
        depends_on: vec![],
        execution_mode,
        direct_tool_call,
    };

    match queue.submit_job(spec).await {
        Ok(job_id) => json!({
            "success": true,
            "job_id": job_id,
            "topic": topic,
            "status": "queued"
        })
        .to_string(),
        Err(e) => json!({"error": format!("Failed to submit job: {}", e)}).to_string(),
    }
}

async fn execute_get_result(queue: &Arc<JobQueue>, args: &serde_json::Value) -> String {
    let job_id = args["job_id"].as_str().unwrap_or("");
    if job_id.is_empty() {
        return json!({"error": "job_id is required"}).to_string();
    }

    match queue.get_job(job_id) {
        Ok(Some(job)) => {
            let mut response = json!({
                "job_id": job.id,
                "status": job.status.as_str(),
                "topic": job.topic,
                "capability": job.capability.as_str(),
                "execution_mode": match job.execution_mode {
                    ExecutionMode::Direct => "direct",
                    ExecutionMode::Mediated => "mediated",
                },
                "created_at": job.created_at,
                "started_at": job.started_at,
                "completed_at": job.completed_at,
            });

            if let Some(ref result) = job.result {
                // Try to parse structured result for richer presentation
                if let Ok(parsed) = serde_json::from_str::<serde_json::Value>(result) {
                    response["result"] = parsed;
                    if let Some(data) = response["result"]["data"].clone().as_object() {
                        // Surface image URLs/paths directly for inline display
                        if let Some(url) = data.get("url").or(data.get("image_url")) {
                            response["inline_content"] = json!({
                                "type": "image",
                                "url": url,
                            });
                        }
                    }
                } else {
                    response["result"] = json!(result);
                }
            }
            if let Some(ref error) = job.error {
                response["error"] = json!(error);
            }

            response.to_string()
        }
        Ok(None) => json!({"error": format!("Job '{}' not found", job_id)}).to_string(),
        Err(e) => json!({"error": format!("Failed to get job: {}", e)}).to_string(),
    }
}

async fn execute_list(queue: &Arc<JobQueue>, args: &serde_json::Value) -> String {
    let status = args["status"].as_str();
    let limit = args["limit"].as_u64().unwrap_or(10) as usize;

    match queue.list_jobs(status, limit) {
        Ok(jobs) => {
            let summaries: Vec<serde_json::Value> = jobs
                .into_iter()
                .map(|j| {
                    json!({
                        "job_id": j.id,
                        "status": j.status.as_str(),
                        "topic": j.topic,
                        "goal": if j.goal.len() > 100 { format!("{}...", &j.goal[..100]) } else { j.goal },
                        "capability": j.capability.as_str(),
                        "created_at": j.created_at,
                    })
                })
                .collect();
            json!({"jobs": summaries, "count": summaries.len()}).to_string()
        }
        Err(e) => json!({"error": format!("Failed to list jobs: {}", e)}).to_string(),
    }
}

async fn execute_create_recurring(queue: &Arc<JobQueue>, args: &serde_json::Value) -> String {
    let goal = args["goal"].as_str().unwrap_or("").to_string();
    let cron_expression = args["cron_expression"].as_str().unwrap_or("").to_string();
    let capability_str = args["capability"].as_str().unwrap_or("general");
    let topic = args["topic"].as_str().unwrap_or("recurring").to_string();
    let goal_template = args["goal_template"].as_str().map(|s| s.to_string());

    if cron_expression.is_empty() {
        return json!({"error": "cron_expression is required"}).to_string();
    }

    // Validate cron expression
    if cron::Schedule::from_str(&cron_expression).is_err() {
        return json!({"error": format!("Invalid cron expression: {}", cron_expression)})
            .to_string();
    }

    let spec = JobSpec {
        goal,
        topic: topic.clone(),
        capability: RequiredCapability::from_str_lossy(capability_str),
        priority: abigail_queue::JobPriority::Normal,
        time_budget_ms: 120_000,
        max_turns: 10,
        system_context: None,
        allowed_skill_ids: vec![],
        ttl_seconds: 86_400,
        input_data: None,
        parent_job_id: None,
        cron_expression: Some(cron_expression),
        is_recurring: true,
        significance_keywords: vec![],
        significance_threshold: 0.5,
        job_mode: "agentic_run".into(),
        goal_template,
        depends_on: vec![],
        execution_mode: ExecutionMode::Mediated,
        direct_tool_call: None,
    };

    match queue.submit_job(spec).await {
        Ok(job_id) => json!({
            "success": true,
            "template_id": job_id,
            "topic": topic,
            "status": "recurring template created"
        })
        .to_string(),
        Err(e) => json!({"error": format!("Failed to create recurring job: {}", e)}).to_string(),
    }
}

async fn execute_list_recurring(queue: &Arc<JobQueue>) -> String {
    match queue.get_recurring_templates() {
        Ok(templates) => {
            let summaries: Vec<serde_json::Value> = templates
                .into_iter()
                .map(|t| {
                    json!({
                        "template_id": t.id,
                        "goal": t.goal,
                        "topic": t.topic,
                        "cron_expression": t.cron_expression,
                        "capability": t.capability.as_str(),
                        "last_scheduled_at": t.last_scheduled_at,
                    })
                })
                .collect();
            json!({"templates": summaries, "count": summaries.len()}).to_string()
        }
        Err(e) => {
            json!({"error": format!("Failed to list recurring templates: {}", e)}).to_string()
        }
    }
}

async fn execute_cancel_recurring(queue: &Arc<JobQueue>, args: &serde_json::Value) -> String {
    let job_id = args["job_id"].as_str().unwrap_or("");
    if job_id.is_empty() {
        return json!({"error": "job_id is required"}).to_string();
    }

    match queue.cancel_job(job_id).await {
        Ok(()) => json!({
            "success": true,
            "template_id": job_id,
            "status": "cancelled"
        })
        .to_string(),
        Err(e) => json!({"error": format!("Failed to cancel recurring job: {}", e)}).to_string(),
    }
}
