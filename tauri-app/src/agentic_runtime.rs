use abigail_capabilities::cognitive::provider::{LlmProvider, ToolCall, ToolDefinition};
use abigail_router::{
    AgenticEngine, AgenticEvent, AgenticRun, RunConfig, RunStatus, ToolExecutor,
};
use abigail_skills::{manifest::SkillId, SkillExecutor, ToolParams};
use anyhow::Context;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use tauri::{AppHandle, Emitter};
use tokio::sync::RwLock;
use tokio_util::sync::CancellationToken;

const AGENTIC_EVENT_NAME: &str = "agentic-event";
const RUN_STORE_FILE: &str = "agentic_runs.json";
const STORE_SCHEMA_VERSION: u32 = 2;
const RECOVERY_FAILURE_REASON: &str = "Run marked failed during startup recovery after process restart";

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RunAttribution {
    pub origin: String,
    pub entity_id: Option<String>,
    pub session_id: Option<String>,
    pub correlation_id: Option<String>,
}

impl RunAttribution {
    pub fn gui() -> Self {
        Self {
            origin: "gui".to_string(),
            entity_id: None,
            session_id: None,
            correlation_id: None,
        }
    }

    pub fn entity(
        entity_id: Option<String>,
        session_id: Option<String>,
        correlation_id: Option<String>,
    ) -> Self {
        Self {
            origin: "entity_pipeline".to_string(),
            entity_id,
            session_id,
            correlation_id,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RunSnapshot {
    pub run: AgenticRun,
    pub attribution: RunAttribution,
}

#[derive(Debug, Clone, Serialize)]
pub struct AgenticRuntimeStatus {
    pub healthy: bool,
    pub storage_path: String,
    pub loaded_runs: usize,
    pub active_runs: usize,
    pub recovered_runs: usize,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct PersistedRunRecord {
    run: AgenticRun,
    attribution: RunAttribution,
    updated_at: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct PersistedRunsEnvelopeV2 {
    schema_version: u32,
    runs: Vec<PersistedRunRecord>,
}

struct RunControl {
    mentor_tx: abigail_router::agentic::MentorResponseTx,
    confirm_tx: abigail_router::agentic::ConfirmationResponseTx,
    cancel: CancellationToken,
}

struct RuntimeToolExecutor {
    executor: Arc<SkillExecutor>,
}

#[async_trait::async_trait]
impl ToolExecutor for RuntimeToolExecutor {
    async fn execute(&self, tool_call: &ToolCall) -> anyhow::Result<String> {
        let (skill_id, tool_name) = split_qualified_tool_name(&tool_call.name)
            .ok_or_else(|| anyhow::anyhow!("Invalid tool name '{}'. Expected skill_id::tool_name", tool_call.name))?;

        let values = if tool_call.arguments.trim().is_empty() {
            HashMap::new()
        } else {
            serde_json::from_str::<HashMap<String, serde_json::Value>>(&tool_call.arguments)
                .with_context(|| format!("Invalid tool arguments JSON for '{}': {}", tool_call.name, tool_call.arguments))?
        };

        let output = self
            .executor
            .execute(&SkillId(skill_id), &tool_name, ToolParams { values })
            .await
            .map_err(|e| anyhow::anyhow!(e.to_string()))?;

        serde_json::to_string(&output).map_err(|e| anyhow::anyhow!(e.to_string()))
    }
}

#[derive(Clone)]
pub struct AgenticRuntime {
    store_path: PathBuf,
    runs: Arc<RwLock<HashMap<String, Arc<RwLock<AgenticRun>>>>>,
    attribution: Arc<RwLock<HashMap<String, RunAttribution>>>,
    controls: Arc<RwLock<HashMap<String, RunControl>>>,
    recovered_runs: Arc<RwLock<usize>>,
}

impl AgenticRuntime {
    pub fn new(data_dir: &Path) -> Self {
        Self {
            store_path: data_dir.join(RUN_STORE_FILE),
            runs: Arc::new(RwLock::new(HashMap::new())),
            attribution: Arc::new(RwLock::new(HashMap::new())),
            controls: Arc::new(RwLock::new(HashMap::new())),
            recovered_runs: Arc::new(RwLock::new(0)),
        }
    }

    pub async fn initialize_recovery(&self) -> anyhow::Result<()> {
        if !self.store_path.exists() {
            return Ok(());
        }

        let content = tokio::fs::read_to_string(&self.store_path)
            .await
            .with_context(|| format!("Failed reading {}", self.store_path.display()))?;

        let parsed = parse_persisted_runs(&content)?;
        let mut recovered = 0usize;

        {
            let mut runs = self.runs.write().await;
            let mut attribution = self.attribution.write().await;

            for mut record in parsed {
                if !is_terminal_status(&record.run.status) {
                    record.run.status = RunStatus::Failed;
                    record.run.completed_at = Some(chrono::Utc::now().to_rfc3339());
                    record.run.events.push(AgenticEvent::RunFailed {
                        task_id: record.run.task_id.clone(),
                        error: RECOVERY_FAILURE_REASON.to_string(),
                    });
                    recovered += 1;
                }

                attribution.insert(record.run.task_id.clone(), record.attribution);
                runs.insert(
                    record.run.task_id.clone(),
                    Arc::new(RwLock::new(record.run)),
                );
            }
        }

        *self.recovered_runs.write().await = recovered;
        self.persist_all().await
    }

    pub async fn status(&self) -> AgenticRuntimeStatus {
        let runs = self.runs.read().await;
        let active_runs = runs
            .values()
            .filter(|r| {
                if let Ok(guard) = r.try_read() {
                    !is_terminal_status(&guard.status)
                } else {
                    false
                }
            })
            .count();

        AgenticRuntimeStatus {
            healthy: true,
            storage_path: self.store_path.display().to_string(),
            loaded_runs: runs.len(),
            active_runs,
            recovered_runs: *self.recovered_runs.read().await,
        }
    }

    pub async fn start_run(
        &self,
        provider: Arc<dyn LlmProvider>,
        tools: Vec<ToolDefinition>,
        executor: Arc<SkillExecutor>,
        config: RunConfig,
        attribution: RunAttribution,
        app_handle: Option<AppHandle>,
    ) -> anyhow::Result<String> {
        let task_id = uuid::Uuid::new_v4().to_string();
        let run = Arc::new(RwLock::new(AgenticRun::new(task_id.clone(), config)));
        let (mentor_tx, mentor_rx) = tokio::sync::mpsc::channel(1);
        let (confirm_tx, confirm_rx) = tokio::sync::mpsc::channel(1);
        let (event_tx, mut event_rx) = tokio::sync::mpsc::channel(256);
        let cancel = CancellationToken::new();

        {
            let mut runs = self.runs.write().await;
            runs.insert(task_id.clone(), run.clone());
        }
        {
            let mut attrs = self.attribution.write().await;
            attrs.insert(task_id.clone(), attribution);
        }
        {
            let mut controls = self.controls.write().await;
            controls.insert(
                task_id.clone(),
                RunControl {
                    mentor_tx,
                    confirm_tx,
                    cancel: cancel.clone(),
                },
            );
        }

        self.persist_all().await?;

        let engine = AgenticEngine::new(provider, tools, Arc::new(RuntimeToolExecutor { executor }));
        let runtime = self.clone();
        let task_id_for_forward = task_id.clone();
        let app_handle_for_events = app_handle.clone();
        tokio::spawn(async move {
            while let Some(event) = event_rx.recv().await {
                runtime
                    .handle_engine_event(&task_id_for_forward, event, app_handle_for_events.as_ref())
                    .await;
            }
        });

        let runtime = self.clone();
        let task_id_for_completion = task_id.clone();
        let app_handle_for_completion = app_handle;
        tokio::spawn(async move {
            let result = engine
                .run(run.clone(), event_tx, mentor_rx, confirm_rx, cancel)
                .await;

            if let Err(err) = result {
                runtime
                    .mark_failed_if_needed(
                        &task_id_for_completion,
                        &err.to_string(),
                        app_handle_for_completion.as_ref(),
                    )
                    .await;
            }

            runtime.controls.write().await.remove(&task_id_for_completion);
            let _ = runtime.persist_all().await;
        });

        Ok(task_id)
    }

    pub async fn get_run_status(&self, task_id: &str) -> anyhow::Result<RunSnapshot> {
        let run = self
            .runs
            .read()
            .await
            .get(task_id)
            .cloned()
            .ok_or_else(|| anyhow::anyhow!("Run '{}' not found", task_id))?;

        let attribution = self
            .attribution
            .read()
            .await
            .get(task_id)
            .cloned()
            .unwrap_or_else(RunAttribution::gui);

        let run_snapshot = run.read().await.clone();
        Ok(RunSnapshot {
            run: run_snapshot,
            attribution,
        })
    }

    pub async fn list_runs(&self) -> Vec<RunSnapshot> {
        let run_entries = self
            .runs
            .read()
            .await
            .iter()
            .map(|(task_id, run)| (task_id.clone(), run.clone()))
            .collect::<Vec<_>>();

        let attrs = self.attribution.read().await.clone();
        let mut output = Vec::with_capacity(run_entries.len());
        for (task_id, run) in run_entries {
            let attribution = attrs
                .get(&task_id)
                .cloned()
                .unwrap_or_else(RunAttribution::gui);
            output.push(RunSnapshot {
                run: run.read().await.clone(),
                attribution,
            });
        }

        output.sort_by(|a, b| b.run.created_at.cmp(&a.run.created_at));
        output
    }

    pub async fn respond_to_mentor(
        &self,
        task_id: &str,
        response: String,
        app_handle: Option<&AppHandle>,
    ) -> anyhow::Result<()> {
        let run = self
            .runs
            .read()
            .await
            .get(task_id)
            .cloned()
            .ok_or_else(|| anyhow::anyhow!("Run '{}' not found", task_id))?;

        {
            let guard = run.read().await;
            if guard.status != RunStatus::WaitingForInput {
                anyhow::bail!(
                    "Run '{}' is not waiting for mentor input (status: {:?})",
                    task_id,
                    guard.status
                );
            }
        }

        let sender = self
            .controls
            .read()
            .await
            .get(task_id)
            .map(|c| c.mentor_tx.clone())
            .ok_or_else(|| anyhow::anyhow!("Run '{}' is no longer active", task_id))?;

        sender
            .send(response)
            .await
            .map_err(|_| anyhow::anyhow!("Mentor response channel closed for run '{}'", task_id))?;

        emit_bridge_event(
            app_handle,
            serde_json::json!({
                "type": "mentor_response_received",
                "task_id": task_id,
            }),
        );

        Ok(())
    }

    pub async fn confirm_action(
        &self,
        task_id: &str,
        approved: bool,
        app_handle: Option<&AppHandle>,
    ) -> anyhow::Result<()> {
        let run = self
            .runs
            .read()
            .await
            .get(task_id)
            .cloned()
            .ok_or_else(|| anyhow::anyhow!("Run '{}' not found", task_id))?;

        {
            let guard = run.read().await;
            if guard.status != RunStatus::WaitingForConfirmation {
                anyhow::bail!(
                    "Run '{}' is not waiting for confirmation (status: {:?})",
                    task_id,
                    guard.status
                );
            }
        }

        let sender = self
            .controls
            .read()
            .await
            .get(task_id)
            .map(|c| c.confirm_tx.clone())
            .ok_or_else(|| anyhow::anyhow!("Run '{}' is no longer active", task_id))?;

        sender
            .send(approved)
            .await
            .map_err(|_| anyhow::anyhow!("Confirmation channel closed for run '{}'", task_id))?;

        emit_bridge_event(
            app_handle,
            serde_json::json!({
                "type": "mentor_confirmation_received",
                "task_id": task_id,
                "approved": approved,
            }),
        );

        Ok(())
    }

    pub async fn cancel_run(&self, task_id: &str) -> anyhow::Result<()> {
        let run = self
            .runs
            .read()
            .await
            .get(task_id)
            .cloned()
            .ok_or_else(|| anyhow::anyhow!("Run '{}' not found", task_id))?;

        if is_terminal_status(&run.read().await.status) {
            return Ok(());
        }

        if let Some(control) = self.controls.read().await.get(task_id) {
            control.cancel.cancel();
            Ok(())
        } else {
            anyhow::bail!("Run '{}' is no longer active", task_id)
        }
    }

    async fn handle_engine_event(&self, task_id: &str, event: AgenticEvent, app_handle: Option<&AppHandle>) {
        if let Some(run_arc) = self.runs.read().await.get(task_id).cloned() {
            let mut run = run_arc.write().await;
            enforce_status_transition(&mut run, &event);
        }

        if let Ok(payload) = serde_json::to_value(&event) {
            emit_bridge_event(app_handle, payload);
        }

        let _ = self.persist_all().await;
    }

    async fn mark_failed_if_needed(&self, task_id: &str, error: &str, app_handle: Option<&AppHandle>) {
        if let Some(run_arc) = self.runs.read().await.get(task_id).cloned() {
            let mut run = run_arc.write().await;
            if !is_terminal_status(&run.status) {
                run.status = RunStatus::Failed;
                run.completed_at = Some(chrono::Utc::now().to_rfc3339());
                let fail_event = AgenticEvent::RunFailed {
                    task_id: task_id.to_string(),
                    error: error.to_string(),
                };
                run.events.push(fail_event.clone());
                if let Ok(payload) = serde_json::to_value(fail_event) {
                    emit_bridge_event(app_handle, payload);
                }
            }
        }
    }

    async fn persist_all(&self) -> anyhow::Result<()> {
        let runs = self.runs.read().await;
        let attrs = self.attribution.read().await;
        let mut persisted_runs = Vec::with_capacity(runs.len());

        for (task_id, run) in runs.iter() {
            let run_guard = run.read().await;
            persisted_runs.push(PersistedRunRecord {
                run: run_guard.clone(),
                attribution: attrs
                    .get(task_id)
                    .cloned()
                    .unwrap_or_else(RunAttribution::gui),
                updated_at: chrono::Utc::now().to_rfc3339(),
            });
        }

        let envelope = PersistedRunsEnvelopeV2 {
            schema_version: STORE_SCHEMA_VERSION,
            runs: persisted_runs,
        };

        if let Some(parent) = self.store_path.parent() {
            tokio::fs::create_dir_all(parent).await?;
        }
        let content = serde_json::to_string_pretty(&envelope)?;
        tokio::fs::write(&self.store_path, content).await?;
        Ok(())
    }
}

fn parse_persisted_runs(content: &str) -> anyhow::Result<Vec<PersistedRunRecord>> {
    if let Ok(v2) = serde_json::from_str::<PersistedRunsEnvelopeV2>(content) {
        return Ok(v2.runs);
    }

    // Migration/backfill: older snapshots may have stored only `Vec<AgenticRun>`.
    if let Ok(legacy_runs) = serde_json::from_str::<Vec<AgenticRun>>(content) {
        return Ok(legacy_runs
            .into_iter()
            .map(|run| PersistedRunRecord {
                run,
                attribution: RunAttribution::gui(),
                updated_at: chrono::Utc::now().to_rfc3339(),
            })
            .collect());
    }

    anyhow::bail!("Unable to parse persisted agentic run store")
}

fn split_qualified_tool_name(qualified: &str) -> Option<(String, String)> {
    let idx = qualified.find("::")?;
    let skill_id = qualified[..idx].to_string();
    let tool_name = qualified[idx + 2..].to_string();
    if skill_id.is_empty() || tool_name.is_empty() {
        return None;
    }
    Some((skill_id, tool_name))
}

fn emit_bridge_event(app_handle: Option<&AppHandle>, payload: serde_json::Value) {
    if let Some(app) = app_handle {
        let _ = app.emit(AGENTIC_EVENT_NAME, payload);
    }
}

fn is_terminal_status(status: &RunStatus) -> bool {
    matches!(status, RunStatus::Completed | RunStatus::Failed | RunStatus::Cancelled)
}

fn enforce_status_transition(run: &mut AgenticRun, event: &AgenticEvent) {
    let next = match event {
        AgenticEvent::RunStarted { .. } => Some(RunStatus::Running),
        AgenticEvent::MentorAsk { .. } => Some(RunStatus::WaitingForInput),
        AgenticEvent::ToolConfirmation { .. } => Some(RunStatus::WaitingForConfirmation),
        AgenticEvent::RunCompleted { .. } => Some(RunStatus::Completed),
        AgenticEvent::RunFailed { .. } => Some(RunStatus::Failed),
        AgenticEvent::RunCancelled { .. } => Some(RunStatus::Cancelled),
        AgenticEvent::TurnStarted { .. } | AgenticEvent::TurnCompleted { .. } => {
            Some(RunStatus::Running)
        }
        _ => None,
    };

    let Some(next_status) = next else {
        return;
    };

    if is_terminal_status(&run.status) {
        // Terminal states are sticky and idempotent.
        return;
    }

    run.status = next_status;

    if is_terminal_status(&run.status) && run.completed_at.is_none() {
        run.completed_at = Some(chrono::Utc::now().to_rfc3339());
    }
}
