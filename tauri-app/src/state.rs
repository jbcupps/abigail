use crate::identity_manager::IdentityManager;
use crate::ollama_manager::OllamaManager;
use crate::rate_limit::CooldownGuard;
use abigail_auth::AuthManager;
use abigail_birth::BirthOrchestrator;
use abigail_core::{AppConfig, SecretsVault};
use abigail_router::{IdEgoRouter, SubagentManager};
use abigail_skills::channel::EventBus;
use abigail_skills::{InstructionRegistry, SkillExecutor, SkillRegistry};
use std::sync::{Arc, Mutex, RwLock};

/// Shared application state holding all subsystem handles.
///
/// ## Lock ordering convention
///
/// When acquiring multiple locks, always follow this order to prevent deadlocks:
///
///   1. `config`            (RwLock — most frequently accessed, acquire first)
///   2. `birth`             (RwLock)
///   3. `secrets`           (Mutex)
///   3b. `skills_secrets`    (Mutex — operational keys for Ego/Skills)
///   4. `hive_secrets`      (Mutex)
///   4b. `auth_manager`     (Arc — internal async locks, acquire after hive_secrets)
///   5. `router`            (RwLock)
///   6. `active_agent_id`   (RwLock)
///   7. `subagent_manager`  (RwLock)
///   8. `browser`           (tokio RwLock — async, acquire after all sync locks)
///   9. `http_client`       (tokio RwLock — async, acquire after all sync locks)
///  10. `ollama`            (tokio Mutex — async, acquire last)
///
/// Rules:
/// - Never hold a sync lock (1-7) across an `.await` boundary.
/// - Drop earlier locks before acquiring later ones when possible.
/// - Scoped blocks `{ let guard = lock.write(); ... }` are preferred to limit hold duration.
pub struct AppState {
    pub config: RwLock<AppConfig>,
    pub birth: RwLock<Option<BirthOrchestrator>>,
    pub router: RwLock<IdEgoRouter>,
    pub registry: Arc<SkillRegistry>,
    pub executor: Arc<SkillExecutor>,
    pub event_bus: Arc<EventBus>,
    pub secrets: Arc<Mutex<SecretsVault>>,
    /// Operational secrets for Ego/Skills (IMAP, Jira, etc.)
    pub skills_secrets: Arc<Mutex<SecretsVault>>,
    /// Hive-level secrets vault (shared API keys across all agents)
    pub hive_secrets: Arc<Mutex<SecretsVault>>,
    /// Auth manager for integration credential lifecycle
    pub auth_manager: Arc<AuthManager>,
    /// Identity manager for the Hive multi-agent system
    pub identity_manager: Arc<IdentityManager>,
    /// Currently active agent UUID (None if no agent loaded)
    pub active_agent_id: RwLock<Option<String>>,
    /// Subagent manager for delegating tasks to specialized subagents
    pub subagent_manager: RwLock<SubagentManager>,
    /// Browser automation capability (lazy-init, async-safe)
    pub browser:
        Arc<tokio::sync::RwLock<abigail_capabilities::sensory::browser::BrowserCapability>>,
    /// Enhanced HTTP client capability with sessions and cookies
    pub http_client:
        Arc<tokio::sync::RwLock<abigail_capabilities::sensory::http_client::HttpClientCapability>>,
    /// Managed Ollama instance (bundled or system)
    pub ollama: Arc<tokio::sync::Mutex<Option<OllamaManager>>>,
    /// Skill instruction registry for injecting skill-specific LLM instructions
    pub instruction_registry: Arc<InstructionRegistry>,
    /// Rate limiter for chat_stream command
    pub chat_cooldown: CooldownGuard,
    /// Rate limiter for birth_chat command
    pub birth_cooldown: CooldownGuard,
    /// Handle to the CLI REST server (if running)
    pub cli_server: Arc<tokio::sync::Mutex<Option<CliServerHandle>>>,
}

pub struct CliServerHandle {
    pub task: tokio::task::JoinHandle<()>,
    pub token: String,
    pub port: u16,
}
