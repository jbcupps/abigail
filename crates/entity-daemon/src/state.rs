//! Entity daemon shared state.

use crate::outbox::RuntimeOutbox;
use abigail_core::AppConfig;
use abigail_memory::{ArchiveExporter, MemoryStore};
use abigail_queue::JobQueue;
use abigail_router::{ConstraintStore, IdEgoRouter};
use abigail_skills::{InstructionRegistry, SkillExecutor, SkillRegistry};
use abigail_streaming::StreamBroker;
use entity_core::{ChatMemoryHook, SkillApplyAcknowledgement};
use hive_core::{ForgeApprovalJob, RuntimeSessionLease, SkillAssignment};
use std::path::PathBuf;
use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::Arc;
use tokio::sync::RwLock;
use tokio_util::sync::CancellationToken;

/// Shared state for all entity-daemon route handlers.
#[derive(Clone)]
pub struct EntityDaemonState {
    pub entity_id: String,
    pub config: AppConfig,
    #[allow(dead_code)]
    pub hive_url: String,
    pub runtime_id: String,
    pub session_lease: RuntimeSessionLease,
    pub router: Arc<IdEgoRouter>,
    pub registry: Arc<SkillRegistry>,
    pub executor: Arc<SkillExecutor>,
    /// Path to this entity's constitutional documents directory.
    pub docs_dir: PathBuf,
    /// Surreal-backed memory store for persistent memory across conversations.
    pub memory: Arc<MemoryStore>,
    /// Persistent async job queue for delegated sub-agent tasks.
    pub job_queue: Arc<JobQueue>,
    /// Backing stream broker for topic-based event publishing.
    pub stream_broker: Arc<dyn StreamBroker>,
    /// Optional hook called when a chat memory is persisted (for future Hive/Superego use).
    pub memory_hook: Option<Arc<dyn ChatMemoryHook>>,
    /// Skill instruction registry for injecting matched LLM instructions into prompts.
    pub instruction_registry: Arc<InstructionRegistry>,
    /// Encrypted archive exporter (None if public key not available).
    pub archive_exporter: Option<Arc<ArchiveExporter>>,
    /// Turns since last archive export; triggers export at threshold.
    pub turns_since_archive: Arc<AtomicU32>,
    /// Cancellation token for the active streaming chat (if any).
    pub active_stream_cancel: Arc<tokio::sync::Mutex<Option<CancellationToken>>>,
    /// Persistent constraint store for learned execution constraints.
    pub constraints: Arc<RwLock<ConstraintStore>>,
    /// Bounded local outbox for entity-scoped writes while Hive is unavailable.
    pub outbox: Arc<RuntimeOutbox>,
    /// Last known successful or failed Hive sync state.
    pub last_hive_sync_at_utc: Arc<RwLock<Option<String>>>,
    pub last_hive_error: Arc<RwLock<Option<String>>>,
    /// Current runtime URL after the local HTTP listener is bound.
    pub runtime_url: Arc<RwLock<Option<String>>>,
    /// Hive-owned skill assignments currently applied to this runtime.
    pub skill_assignments: Arc<RwLock<Vec<SkillAssignment>>>,
    /// Hive-approved forge jobs available to this runtime.
    #[allow(dead_code)]
    pub forge_jobs: Arc<RwLock<Vec<ForgeApprovalJob>>>,
    /// Recent runtime acknowledgements for skill apply/hot-reload activity.
    pub recent_skill_acks: Arc<RwLock<Vec<SkillApplyAcknowledgement>>>,
}

/// Default number of turns between automatic archive exports.
pub const ARCHIVE_INTERVAL_TURNS: u32 = 50;

impl EntityDaemonState {
    pub fn queue_outbox_record(
        &self,
        kind: &str,
        payload: serde_json::Value,
    ) -> Result<(), String> {
        self.outbox.enqueue(&self.entity_id, kind, payload).map(|_| ())
    }

    pub async fn record_hive_sync_success(&self) {
        let mut last_sync = self.last_hive_sync_at_utc.write().await;
        *last_sync = self
            .outbox
            .status()
            .ok()
            .and_then(|status| status.last_sync_at_utc)
            .or_else(|| Some(chrono::Utc::now().to_rfc3339()));
        let mut last_error = self.last_hive_error.write().await;
        *last_error = None;
    }

    pub async fn record_hive_sync_error(&self, error: impl Into<String>) {
        let mut last_error = self.last_hive_error.write().await;
        *last_error = Some(error.into());
    }

    pub async fn push_skill_ack(&self, acknowledgement: SkillApplyAcknowledgement) {
        let mut acks = self.recent_skill_acks.write().await;
        acks.push(acknowledgement);
        if acks.len() > 32 {
            let overflow = acks.len() - 32;
            acks.drain(0..overflow);
        }
    }

    /// Increment the turn counter and trigger an archive export if threshold reached.
    pub fn maybe_auto_archive(&self) {
        let count = self.turns_since_archive.fetch_add(1, Ordering::Relaxed) + 1;
        if count >= ARCHIVE_INTERVAL_TURNS {
            self.turns_since_archive.store(0, Ordering::Relaxed);
            if let Some(ref exporter) = self.archive_exporter {
                let mem = self.memory.clone();
                let exp = exporter.clone();
                tokio::spawn(async move {
                    if let Err(e) = exp.export(&mem) {
                        tracing::warn!("Auto-archive failed: {}", e);
                    }
                });
            }
        }
    }
}
