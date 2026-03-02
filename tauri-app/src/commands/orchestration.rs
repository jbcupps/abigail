use crate::agentic_runtime::RunAttribution;
use crate::state::AppState;
use abigail_capabilities::cognitive::provider::Message;
use abigail_router::{JobMode, OrchestrationJobLog};
use serde::Serialize;
use tauri::State;

#[derive(Debug, Serialize)]
pub struct OrchestrationBackendStatus {
    pub healthy: bool,
    pub jobs_loaded: usize,
    pub runtime_loaded_runs: usize,
    pub runtime_active_runs: usize,
}

#[tauri::command]
pub async fn get_orchestration_backend_status(
    state: State<'_, AppState>,
) -> Result<OrchestrationBackendStatus, String> {
    let jobs_loaded = state.orchestration_scheduler.list_jobs().await.len();
    let runtime = state.agentic_runtime.status().await;

    Ok(OrchestrationBackendStatus {
        healthy: runtime.healthy,
        jobs_loaded,
        runtime_loaded_runs: runtime.loaded_runs,
        runtime_active_runs: runtime.active_runs,
    })
}

#[tauri::command]
pub async fn list_orchestration_jobs(
    state: State<'_, AppState>,
) -> Result<Vec<abigail_router::OrchestrationJob>, String> {
    Ok(state.orchestration_scheduler.list_jobs().await)
}

#[tauri::command]
pub async fn set_orchestration_job_enabled(
    state: State<'_, AppState>,
    job_id: String,
    enabled: bool,
) -> Result<(), String> {
    state
        .orchestration_scheduler
        .set_enabled(&job_id, enabled)
        .await
        .map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn delete_orchestration_job(
    state: State<'_, AppState>,
    job_id: String,
) -> Result<(), String> {
    state
        .orchestration_scheduler
        .delete_job(&job_id)
        .await
        .map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn list_orchestration_job_logs(
    state: State<'_, AppState>,
    job_id: Option<String>,
) -> Result<Vec<OrchestrationJobLog>, String> {
    Ok(state
        .orchestration_scheduler
        .get_logs(job_id.as_deref())
        .await)
}

#[derive(Debug, Serialize)]
pub struct RunNowResult {
    pub run_id: String,
    pub mode: String,
    pub result_summary: String,
}

#[tauri::command]
pub async fn run_orchestration_job_now(
    app: tauri::AppHandle,
    state: State<'_, AppState>,
    job_id: String,
) -> Result<RunNowResult, String> {
    let jobs = state.orchestration_scheduler.list_jobs().await;
    let job = jobs
        .into_iter()
        .find(|j| j.job_id == job_id)
        .ok_or_else(|| format!("Job not found: {}", job_id))?;

    let started = std::time::Instant::now();
    let (result_summary, run_id) = match job.mode {
        JobMode::AgenticRun => {
            let provider = {
                let router = state.router.read().map_err(|e| e.to_string())?.clone();
                router.best_available_provider().ok_or_else(|| {
                    "No available provider for orchestration agentic run".to_string()
                })?
            };

            let task_id = state
                .agentic_runtime
                .start_run(
                    provider,
                    entity_chat::build_tool_definitions(&state.registry),
                    state.executor.clone(),
                    abigail_router::RunConfig {
                        goal: job.goal_template.clone().unwrap_or_else(|| {
                            format!("Execute orchestration job '{}'.", job.name)
                        }),
                        max_turns: 8,
                        require_confirmation: false,
                        system_context: Some(format!(
                            "Orchestration job {} ({}) initiated this run.",
                            job.name, job.job_id
                        )),
                    },
                    RunAttribution::entity(
                        Some("orchestration".to_string()),
                        Some(job.job_id.clone()),
                        None,
                    ),
                    Some(app),
                )
                .await
                .map_err(|e| e.to_string())?;

            (
                format!(
                    "Spawned agentic run {} from orchestration job {}",
                    task_id, job.job_id
                ),
                task_id,
            )
        }
        JobMode::IdCheck => {
            let router = state.router.read().map_err(|e| e.to_string())?.clone();
            let prompt = job.goal_template.clone().unwrap_or_else(|| {
                format!(
                    "Run the scheduled Id health check for job '{}' and summarize findings.",
                    job.name
                )
            });
            let response = router
                .id_only(vec![Message::new("user", &prompt)])
                .await
                .map_err(|e| e.to_string())?;
            let run_id = uuid::Uuid::new_v4().to_string();
            (response.content, run_id)
        }
    };

    let (_, decision) = abigail_router::OrchestrationScheduler::score_significance(
        &result_summary,
        &job.significance_policy,
    );

    state
        .orchestration_scheduler
        .record_log(OrchestrationJobLog {
            job_id: job.job_id.clone(),
            run_id: run_id.clone(),
            ran_at: chrono::Utc::now().to_rfc3339(),
            result: result_summary.clone(),
            decision,
            duration_ms: started.elapsed().as_millis() as u64,
        })
        .await
        .map_err(|e| e.to_string())?;

    Ok(RunNowResult {
        run_id,
        mode: match job.mode {
            JobMode::AgenticRun => "agentic_run".to_string(),
            JobMode::IdCheck => "id_check".to_string(),
        },
        result_summary,
    })
}
