//! Entity daemon shared state.

use abigail_core::AppConfig;
use abigail_memory::{ArchiveExporter, MemoryStore};
use abigail_router::IdEgoRouter;
use abigail_skills::channel::EventBus;
use abigail_skills::{InstructionRegistry, SkillExecutor, SkillRegistry};
use entity_core::ChatMemoryHook;
use std::path::PathBuf;
use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::Arc;

/// Shared state for all entity-daemon route handlers.
#[derive(Clone)]
pub struct EntityDaemonState {
    pub entity_id: String,
    pub config: AppConfig,
    pub router: Arc<IdEgoRouter>,
    pub registry: Arc<SkillRegistry>,
    pub executor: Arc<SkillExecutor>,
    /// Event bus for skill-to-skill communication (used in Phase 2 streaming).
    #[allow(dead_code)]
    pub event_bus: Arc<EventBus>,
    /// Path to this entity's constitutional documents directory.
    pub docs_dir: PathBuf,
    /// SQLite memory store for persistent memory across conversations.
    pub memory: Arc<MemoryStore>,
    /// Optional hook called when a chat memory is persisted (for future Hive/Superego use).
    pub memory_hook: Option<Arc<dyn ChatMemoryHook>>,
    /// Skill instruction registry for injecting matched LLM instructions into prompts.
    pub instruction_registry: Arc<InstructionRegistry>,
    /// Encrypted archive exporter (None if public key not available).
    pub archive_exporter: Option<Arc<ArchiveExporter>>,
    /// Turns since last archive export; triggers export at threshold.
    pub turns_since_archive: Arc<AtomicU32>,
}

/// Default number of turns between automatic archive exports.
pub const ARCHIVE_INTERVAL_TURNS: u32 = 50;

impl EntityDaemonState {
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
