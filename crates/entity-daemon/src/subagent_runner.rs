//! Executes queued jobs as delegated sub-agent runs.

use crate::capability_matcher::{CapabilityMatcher, CapabilitySelection};
use abigail_capabilities::cognitive::Message;
use abigail_queue::{JobQueue, JobRecord};
use abigail_router::IdEgoRouter;
use abigail_skills::{InstructionRegistry, SkillExecutor, SkillRegistry};
use std::collections::HashSet;
use std::sync::Arc;
use tokio::time::{timeout, Duration};

/// Executes individual jobs from the queue.
#[derive(Clone)]
pub struct SubagentRunner {
    queue: Arc<JobQueue>,
    router: Arc<IdEgoRouter>,
    registry: Arc<SkillRegistry>,
    executor: Arc<SkillExecutor>,
    matcher: CapabilityMatcher,
    entity_name: Option<String>,
    docs_dir: std::path::PathBuf,
    instruction_registry: Arc<InstructionRegistry>,
}

impl SubagentRunner {
    pub fn new(
        queue: Arc<JobQueue>,
        router: Arc<IdEgoRouter>,
        registry: Arc<SkillRegistry>,
        executor: Arc<SkillExecutor>,
        matcher: CapabilityMatcher,
        entity_name: Option<String>,
    ) -> Self {
        Self {
            queue,
            router,
            registry,
            executor,
            matcher,
            entity_name,
            docs_dir: std::path::PathBuf::new(),
            instruction_registry: Arc::new(InstructionRegistry::empty()),
        }
    }

    /// Set the docs directory for building sub-agent constitutional context.
    pub fn with_docs_dir(mut self, docs_dir: std::path::PathBuf) -> Self {
        self.docs_dir = docs_dir;
        self
    }

    /// Set the instruction registry for topic-affinity instruction delegation.
    pub fn with_instruction_registry(mut self, ir: Arc<InstructionRegistry>) -> Self {
        self.instruction_registry = ir;
        self
    }

    /// Claim and execute a job. Returns `Ok(())` when finished (including claim races).
    pub async fn run_job(&self, job: JobRecord) -> anyhow::Result<()> {
        let selection = self.matcher.select(&job.capability);
        let agent_id = format!("subagent-{}", uuid::Uuid::new_v4());
        let model_for_state = selection
            .model_hint
            .clone()
            .unwrap_or_else(|| "auto".to_string());

        if let Err(err) = self
            .queue
            .mark_running(&job.id, &agent_id, &model_for_state, &selection.provider)
            .await
        {
            if is_claim_race(&err) {
                tracing::debug!("Job {} was already claimed by another worker", job.id);
                return Ok(());
            }
            return Err(err);
        }

        let messages = build_job_messages(
            &job,
            &selection,
            self.entity_name.as_deref(),
            &self.docs_dir,
            &self.instruction_registry,
            &self.registry,
        );
        let tools = filter_tools_for_job(entity_chat::build_tool_definitions(&self.registry), &job);

        let timeout_ms = job.time_budget_ms.max(1_000);
        let task = entity_chat::run_tool_use_loop_with_model_override(
            &self.router,
            &self.executor,
            messages,
            tools,
            selection.model_hint.clone(),
        );
        match timeout(Duration::from_millis(timeout_ms), task).await {
            Ok(Ok(result)) => {
                let turns = result
                    .execution_trace
                    .as_ref()
                    .map(|t| t.steps.len() as u32)
                    .unwrap_or(1);
                self.queue
                    .mark_completed(&job.id, &result.content, turns.max(1))
                    .await?;
                tracing::info!(
                    "Completed queued job {} (topic={}, capability={})",
                    job.id,
                    job.topic,
                    job.capability.as_str()
                );
            }
            Ok(Err(err)) => {
                let msg = format!("Sub-agent execution failed: {}", err);
                self.queue.mark_failed(&job.id, &msg, 0).await?;
                tracing::warn!("Job {} failed (topic={}): {}", job.id, job.topic, err);
            }
            Err(_) => {
                let msg = format!("Job exceeded time budget ({} ms)", timeout_ms);
                self.queue.mark_failed(&job.id, &msg, 0).await?;
                tracing::warn!("Job {} timed out after {} ms", job.id, timeout_ms);
            }
        }

        Ok(())
    }
}

fn is_claim_race(err: &anyhow::Error) -> bool {
    let text = err.to_string().to_lowercase();
    text.contains("not in queued state")
}

fn build_job_messages(
    job: &JobRecord,
    selection: &CapabilitySelection,
    entity_name: Option<&str>,
    docs_dir: &std::path::Path,
    instruction_registry: &InstructionRegistry,
    skill_registry: &SkillRegistry,
) -> Vec<Message> {
    let mut system = String::new();
    system.push_str("You are a delegated sub-agent task runner for Abigail.\n");
    if let Some(name) = entity_name {
        system.push_str(&format!("Entity: {}.\n", name));
    }
    system.push_str(&format!(
        "Capability requirement: {}.\nPreferred provider: {}.\nPreferred tier: {:?}.\n",
        job.capability.as_str(),
        selection.provider,
        selection.tier
    ));
    if let Some(ref model) = selection.model_hint {
        system.push_str(&format!("Preferred model: {}.\n", model));
    }

    // Inject constitutional context: use job-supplied context if set,
    // otherwise build the full constitutional docs from disk.
    if let Some(ref ctx) = job.system_context {
        system.push('\n');
        system.push_str(ctx);
        system.push('\n');
    } else if docs_dir.exists() {
        let constitutional = abigail_core::system_prompt::build_subagent_system_context(docs_dir);
        if !constitutional.is_empty() {
            system.push('\n');
            system.push_str(&constitutional);
            system.push('\n');
        }
    }

    if !job.allowed_skill_ids.is_empty() {
        system.push_str(&format!(
            "Allowed skills: {}.\n",
            job.allowed_skill_ids.join(", ")
        ));
    }

    // Topic-affinity instruction delegation: inject skill-specific instructions
    // matched against the job goal so sub-agents know how to use relevant tools.
    let registered_ids: HashSet<String> = skill_registry
        .list()
        .unwrap_or_default()
        .iter()
        .map(|m| m.id.0.clone())
        .collect();
    let delegation_instructions =
        instruction_registry.format_for_delegation(&job.goal, &registered_ids);
    if !delegation_instructions.is_empty() {
        system.push_str(&delegation_instructions);
    }

    let mut user = format!("Task goal:\n{}\n", job.goal);
    if let Some(ref input) = job.input_data {
        user.push_str("\nInput data (JSON):\n");
        user.push_str(&serde_json::to_string_pretty(input).unwrap_or_else(|_| input.to_string()));
        user.push('\n');
    }

    vec![Message::new("system", system), Message::new("user", user)]
}

fn filter_tools_for_job(
    all_tools: Vec<abigail_capabilities::cognitive::ToolDefinition>,
    job: &JobRecord,
) -> Vec<abigail_capabilities::cognitive::ToolDefinition> {
    if job.allowed_skill_ids.is_empty() {
        return all_tools;
    }

    all_tools
        .into_iter()
        .filter(|tool| {
            tool.name
                .split_once("::")
                .map(|(skill_id, _)| job.allowed_skill_ids.iter().any(|id| id == skill_id))
                .unwrap_or(false)
        })
        .collect()
}
