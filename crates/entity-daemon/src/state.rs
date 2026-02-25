//! Entity daemon shared state.

use abigail_core::AppConfig;
use abigail_memory::MemoryStore;
use abigail_router::IdEgoRouter;
use abigail_skills::channel::EventBus;
use abigail_skills::{InstructionRegistry, SkillExecutor, SkillRegistry};
use entity_core::ChatMemoryHook;
use std::path::PathBuf;
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
}
