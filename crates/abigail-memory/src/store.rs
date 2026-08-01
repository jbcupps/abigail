use crate::protected_topics::{
    decrypt_secret_payload, encrypt_secret_payload, plan_secret_move, ProtectedTopicEntry,
    ProtectedTopicSummary, SecretKind, SecretMovePlan, TriangleEthicPreview,
};
use abigail_core::{AppConfig, HybridUnlockProvider, PassphraseUnlockProvider, UnlockProvider};
use abigail_persistence::{EntityScope, PersistenceError, PersistenceHandle, QueryBinding};
use chrono::Utc;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use thiserror::Error;
use uuid::Uuid;

const STORE_SCHEMA_VERSION: i64 = 6;

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub enum MemoryWeight {
    Ephemeral,
    Distilled,
    Crystallized,
}

impl MemoryWeight {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Ephemeral => "ephemeral",
            Self::Distilled => "distilled",
            Self::Crystallized => "crystallized",
        }
    }
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct Memory {
    pub id: String,
    pub content: String,
    pub weight: MemoryWeight,
    pub created_at: chrono::DateTime<Utc>,
}

impl Memory {
    pub fn ephemeral(content: String) -> Self {
        Self::new(content, MemoryWeight::Ephemeral)
    }

    pub fn distilled(content: String) -> Self {
        Self::new(content, MemoryWeight::Distilled)
    }

    pub fn crystallized(content: String) -> Self {
        Self::new(content, MemoryWeight::Crystallized)
    }

    fn new(content: String, weight: MemoryWeight) -> Self {
        Self {
            id: Uuid::new_v4().to_string(),
            content,
            weight,
            created_at: Utc::now(),
        }
    }
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct ConversationTurn {
    pub id: String,
    pub session_id: String,
    pub turn_number: u32,
    pub role: String,
    pub content: String,
    pub provider: Option<String>,
    pub model: Option<String>,
    pub tier: Option<String>,
    pub complexity_score: Option<u8>,
    pub token_estimate: Option<u32>,
    pub created_at: chrono::DateTime<Utc>,
}

impl ConversationTurn {
    pub fn new(session_id: &str, role: &str, content: &str) -> Self {
        Self {
            id: Uuid::new_v4().to_string(),
            session_id: session_id.to_string(),
            turn_number: 0,
            role: role.to_string(),
            content: content.to_string(),
            provider: None,
            model: None,
            tier: None,
            complexity_score: None,
            token_estimate: None,
            created_at: Utc::now(),
        }
    }

    pub fn with_metadata(
        mut self,
        provider: Option<String>,
        model: Option<String>,
        tier: Option<String>,
        complexity_score: Option<u8>,
    ) -> Self {
        self.provider = provider;
        self.model = model;
        self.tier = tier;
        self.complexity_score = complexity_score;
        self
    }
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct SessionSummary {
    pub session_id: String,
    pub turn_count: u64,
    pub first_at: String,
    pub last_at: String,
}

#[derive(Error, Debug)]
pub enum StoreError {
    #[error("Persistence error: {0}")]
    Persistence(#[from] PersistenceError),
    #[error("Birth already recorded")]
    BirthAlreadyRecorded,
    #[error("Invalid data: {0}")]
    InvalidData(String),
    #[error("Protected topic error: {0}")]
    ProtectedTopic(String),
    #[error("Migration failed: {0}")]
    Migration(String),
}

pub type Result<T> = std::result::Result<T, StoreError>;

#[derive(Clone)]
pub struct MemoryStore {
    persistence: PersistenceHandle,
    unlock: Arc<dyn UnlockProvider>,
    entity_id: Option<String>,
    temp_root: Option<PathBuf>,
}

impl MemoryStore {
    pub fn new_for_ci() -> Result<Self> {
        Self::open_in_memory()
    }

    pub fn open(path: impl AsRef<Path>) -> Result<Self> {
        Self::open_with_unlock(path, Arc::new(HybridUnlockProvider::new()))
    }

    pub fn open_with_unlock(
        path: impl AsRef<Path>,
        unlock: Arc<dyn UnlockProvider>,
    ) -> Result<Self> {
        let path = path.as_ref().to_path_buf();
        let entity_id = infer_entity_id(&path).or_else(|| synthetic_file_scope(&path));
        Self::open_internal(path, unlock, entity_id, None)
    }

    pub fn open_in_memory() -> Result<Self> {
        let unlock: Arc<dyn UnlockProvider> =
            Arc::new(PassphraseUnlockProvider::new("abigail-memory-in-memory"));
        Ok(Self {
            persistence: PersistenceHandle::open_ephemeral(EntityScope::Entity(
                "in_memory".to_string(),
            ))?,
            unlock,
            entity_id: Some("in_memory".to_string()),
            temp_root: None,
        })
    }

    /// Opens an ephemeral in-memory store with a caller-supplied entity id and
    /// unlock provider.  Useful for tests that need a specific entity scope
    /// (e.g. secret-detection tests that assert on `secrets-{entity_id}` topic
    /// names) without touching the filesystem.
    pub fn open_in_memory_with_entity_and_unlock(
        entity_id: &str,
        unlock: Arc<dyn UnlockProvider>,
    ) -> Result<Self> {
        Ok(Self {
            persistence: PersistenceHandle::open_ephemeral(EntityScope::Entity(
                entity_id.to_string(),
            ))?,
            unlock,
            entity_id: Some(entity_id.to_string()),
            temp_root: None,
        })
    }

    pub fn schema_version(&self) -> Result<i64> {
        Ok(STORE_SCHEMA_VERSION)
    }

    pub fn open_with_config(config: &AppConfig) -> Result<Self> {
        let db_path = config.db_path.clone();
        if let Some(parent) = db_path.parent() {
            std::fs::create_dir_all(parent)
                .map_err(|error| StoreError::Migration(error.to_string()))?;
        }

        let entity_id = if config.is_hive {
            None
        } else {
            config
                .data_dir
                .file_name()
                .and_then(|name| name.to_str())
                .map(str::to_string)
        };
        tracing::info!(
            "MemoryStore opening SurrealDB store: scope={} path={} data_dir={}",
            if config.is_hive { "hive" } else { "entity" },
            db_path.display(),
            config.data_dir.display()
        );
        if abigail_persistence::ci_mode_enabled()
            && std::env::var_os("ABIGAIL_DAEMON_INTEGRATION").is_some()
        {
            tracing::info!(
                "MemoryStore using ephemeral store for daemon integration test scope={}",
                entity_id.as_deref().unwrap_or("hive")
            );
            return Self::open_in_memory_with_entity_and_unlock(
                entity_id.as_deref().unwrap_or("hive"),
                Arc::new(HybridUnlockProvider::new()),
            );
        }
        Self::open_internal(
            db_path,
            Arc::new(HybridUnlockProvider::new()),
            entity_id,
            None,
        )
    }

    pub fn path(&self) -> &Path {
        self.persistence.path()
    }

    pub fn capture_secret_message(
        &self,
        entity_id: &str,
        session_id: &str,
        role: &str,
        content: &str,
        source: &str,
    ) -> Result<Option<SecretMovePlan>> {
        let Some(secret) =
            self.prepare_secret_capture(entity_id, session_id, role, content, source, Utc::now())?
        else {
            return Ok(None);
        };
        self.persist_protected_secret(entity_id, &secret)?;
        Ok(Some(secret.plan))
    }

    pub fn list_protected_topics(&self, limit: usize) -> Result<Vec<ProtectedTopicSummary>> {
        let sql = format!(
            "SELECT * FROM protected_topic ORDER BY updated_at DESC LIMIT {}",
            limit.max(1)
        );
        let docs: Vec<ProtectedTopicDoc> = self.persistence.query_vec(&sql, &[])?;
        docs.into_iter().map(TryFrom::try_from).collect()
    }

    pub fn protected_topic_entries(
        &self,
        topic_name: &str,
        limit: usize,
    ) -> Result<Vec<ProtectedTopicEntry>> {
        let sql = format!(
            "SELECT * FROM protected_topic_entry WHERE topic_name = $topic_name ORDER BY created_at DESC LIMIT {}",
            limit.max(1)
        );
        let bindings = [binding("topic_name", topic_name)?];
        let docs: Vec<ProtectedTopicEntryDoc> = self.persistence.query_vec(&sql, &bindings)?;
        docs.into_iter()
            .map(|doc| {
                let payload =
                    decrypt_secret_payload(&self.unlock, &doc.topic_name, &doc.ciphertext)
                        .map_err(StoreError::ProtectedTopic)?;
                Ok(ProtectedTopicEntry {
                    id: doc.id,
                    topic_name: doc.topic_name,
                    session_id: doc.session_id,
                    role: doc.role,
                    source: payload.source,
                    secret_kind: parse_secret_kind(&doc.secret_kind)?,
                    redacted_excerpt: doc.redacted_excerpt,
                    preview: doc.preview,
                    content: payload.content,
                    created_at: parse_timestamp(&doc.created_at)?,
                })
            })
            .collect()
    }

    pub fn has_birth(&self) -> Result<bool> {
        Ok(self.birth_doc()?.is_some())
    }

    pub fn record_birth(&self, memory: &Memory) -> Result<()> {
        if self.has_birth()? {
            return Err(StoreError::BirthAlreadyRecorded);
        }
        let doc = BirthDoc {
            id: "primary".to_string(),
            content: memory.content.clone(),
            created_at: memory.created_at.to_rfc3339(),
        };
        self.persistence.create("birth", &doc.id, &doc)?;
        Ok(())
    }

    pub fn insert_memory(&self, memory: &Memory) -> Result<()> {
        if self.memory_exists(&memory.id)? {
            return Err(StoreError::InvalidData(format!(
                "memory '{}' already exists",
                memory.id
            )));
        }
        self.persistence
            .create("memory_entry", &memory.id, &MemoryDoc::from(memory))?;
        Ok(())
    }

    pub fn count_memories(&self) -> Result<u64> {
        Ok(self.all_memories()?.len() as u64)
    }

    pub fn vacuum(&self) -> Result<()> {
        Ok(())
    }

    pub fn clear_memories(&self) -> Result<u64> {
        let memories = self.all_memories()?;
        for memory in &memories {
            self.persistence.delete_record("memory_entry", &memory.id)?;
        }
        Ok(memories.len() as u64)
    }

    pub fn search_memories(&self, query: &str, limit: usize) -> Result<Vec<Memory>> {
        let query = query.to_ascii_lowercase();
        let mut memories: Vec<Memory> = self
            .all_memories()?
            .into_iter()
            .filter(|memory| contains_case_insensitive(&memory.content, &query))
            .collect();
        memories.sort_by_key(|memory| std::cmp::Reverse(memory.created_at));
        memories.truncate(limit);
        Ok(memories)
    }

    pub fn insert_turn(&self, turn: &ConversationTurn) -> Result<()> {
        if self.turn_exists(&turn.id)? {
            return Err(StoreError::InvalidData(format!(
                "turn '{}' already exists",
                turn.id
            )));
        }
        self.persist_turn(turn)
    }

    pub fn session_turn_count(&self, session_id: &str) -> Result<u64> {
        Ok(self
            .all_turns()?
            .into_iter()
            .filter(|turn| turn.session_id == session_id)
            .count() as u64)
    }

    pub fn total_turn_count(&self) -> Result<u64> {
        Ok(self.all_turns()?.len() as u64)
    }

    pub fn recent_turns(&self, session_id: &str, limit: usize) -> Result<Vec<ConversationTurn>> {
        let sql = format!(
            "SELECT * FROM conversation_turn WHERE session_id = $session_id ORDER BY turn_number DESC, created_at DESC LIMIT {}",
            limit.max(1)
        );
        let bindings = [binding("session_id", session_id)?];
        let mut turns: Vec<ConversationTurn> = self
            .persistence
            .query_vec::<TurnDoc>(&sql, &bindings)?
            .into_iter()
            .map(TryFrom::try_from)
            .collect::<Result<Vec<_>>>()?;
        turns.sort_by_key(|turn| turn.turn_number);
        Ok(turns)
    }

    pub fn recent_turns_all_sessions(&self, limit: usize) -> Result<Vec<ConversationTurn>> {
        let sql = format!(
            "SELECT * FROM conversation_turn ORDER BY created_at DESC LIMIT {}",
            limit.max(1)
        );
        self.persistence
            .query_vec::<TurnDoc>(&sql, &[])?
            .into_iter()
            .map(TryFrom::try_from)
            .collect()
    }

    pub fn search_turns(&self, query: &str, limit: usize) -> Result<Vec<ConversationTurn>> {
        let query = query.to_ascii_lowercase();
        let mut turns: Vec<ConversationTurn> = self
            .all_turns()?
            .into_iter()
            .filter(|turn| contains_case_insensitive(&turn.content, &query))
            .collect();
        turns.sort_by_key(|turn| std::cmp::Reverse(turn.created_at));
        turns.truncate(limit);
        Ok(turns)
    }

    pub fn list_sessions(&self, limit: usize) -> Result<Vec<SessionSummary>> {
        let mut sessions = BTreeMap::<String, SessionAccumulator>::new();
        for turn in self.all_turns()? {
            let entry = sessions
                .entry(turn.session_id.clone())
                .or_insert_with(|| SessionAccumulator::new(turn.created_at.to_rfc3339()));
            entry.turn_count += 1;
            let timestamp = turn.created_at.to_rfc3339();
            if timestamp < entry.first_at {
                entry.first_at = timestamp.clone();
            }
            if timestamp > entry.last_at {
                entry.last_at = timestamp;
            }
        }

        let mut out: Vec<SessionSummary> = sessions
            .into_iter()
            .map(|(session_id, acc)| SessionSummary {
                session_id,
                turn_count: acc.turn_count,
                first_at: acc.first_at,
                last_at: acc.last_at,
            })
            .collect();
        out.sort_by(|left, right| right.last_at.cmp(&left.last_at));
        out.truncate(limit);
        Ok(out)
    }

    pub fn all_turns(&self) -> Result<Vec<ConversationTurn>> {
        self.persistence
            .query_vec::<TurnDoc>(
                "SELECT * FROM conversation_turn ORDER BY created_at ASC",
                &[],
            )?
            .into_iter()
            .map(TryFrom::try_from)
            .collect()
    }

    pub fn all_memories(&self) -> Result<Vec<Memory>> {
        self.persistence
            .query_vec::<MemoryDoc>("SELECT * FROM memory_entry ORDER BY created_at ASC", &[])?
            .into_iter()
            .map(TryFrom::try_from)
            .collect()
    }

    pub fn insert_turn_or_ignore(&self, turn: &ConversationTurn) -> Result<bool> {
        if self.turn_exists(&turn.id)? {
            return Ok(false);
        }
        self.persist_turn(turn)?;
        Ok(true)
    }

    pub fn insert_memory_or_ignore(&self, memory: &Memory) -> Result<bool> {
        if self.memory_exists(&memory.id)? {
            return Ok(false);
        }
        self.persistence
            .create("memory_entry", &memory.id, &MemoryDoc::from(memory))?;
        Ok(true)
    }

    pub fn recent_memories(&self, limit: usize) -> Result<Vec<Memory>> {
        let sql = format!(
            "SELECT * FROM memory_entry ORDER BY created_at DESC LIMIT {}",
            limit.max(1)
        );
        self.persistence
            .query_vec::<MemoryDoc>(&sql, &[])?
            .into_iter()
            .map(TryFrom::try_from)
            .collect()
    }

    fn open_internal(
        path: PathBuf,
        unlock: Arc<dyn UnlockProvider>,
        entity_id: Option<String>,
        temp_root: Option<PathBuf>,
    ) -> Result<Self> {
        let scope = match entity_id.as_ref() {
            Some(entity_id) => EntityScope::Entity(entity_id.clone()),
            None => EntityScope::Hive,
        };
        let persistence = PersistenceHandle::open(&path, scope)?;
        Ok(Self {
            persistence,
            unlock,
            entity_id,
            temp_root,
        })
    }

    fn persist_turn(&self, turn: &ConversationTurn) -> Result<()> {
        let (persisted_turn, protected_secret) =
            self.prepare_turn_for_storage(turn, "conversation_turn")?;
        if let Some(secret) = protected_secret.as_ref() {
            let entity_id = self
                .entity_id
                .as_deref()
                .ok_or_else(|| StoreError::ProtectedTopic("missing entity id".to_string()))?;
            self.persist_protected_secret(entity_id, secret)?;
        }
        self.persistence.create(
            "conversation_turn",
            &persisted_turn.id,
            &TurnDoc::from(&persisted_turn),
        )?;
        Ok(())
    }

    fn birth_doc(&self) -> Result<Option<BirthDoc>> {
        self.persistence
            .select_record("birth", "primary")
            .map_err(StoreError::from)
    }

    fn memory_exists(&self, id: &str) -> Result<bool> {
        self.persistence
            .record_exists("memory_entry", id)
            .map_err(StoreError::from)
    }

    fn turn_exists(&self, id: &str) -> Result<bool> {
        self.persistence
            .record_exists("conversation_turn", id)
            .map_err(StoreError::from)
    }

    fn persist_protected_secret(
        &self,
        entity_id: &str,
        secret: &PreparedProtectedSecret,
    ) -> Result<()> {
        let existing_topic: Option<ProtectedTopicDoc> = self.persistence.query_one(
            "SELECT * FROM protected_topic WHERE topic_name = $topic_name LIMIT 1",
            &[binding("topic_name", &secret.plan.topic_name)?],
        )?;

        let created_at = secret.created_at.to_rfc3339();
        let updated = ProtectedTopicDoc {
            id: secret.plan.topic_name.clone(),
            topic_name: secret.plan.topic_name.clone(),
            entity_id: entity_id.to_string(),
            protection_kind: "secret".to_string(),
            entry_count: existing_topic
                .as_ref()
                .map(|topic| topic.entry_count + 1)
                .unwrap_or(1),
            last_secret_kind: secret.plan.secret_kind.as_str().to_string(),
            last_redacted_excerpt: secret.plan.redacted_excerpt.clone(),
            last_preview: secret.plan.preview.clone(),
            created_at: existing_topic
                .as_ref()
                .map(|topic| topic.created_at.clone())
                .unwrap_or_else(|| created_at.clone()),
            updated_at: created_at.clone(),
        };
        self.persistence
            .upsert("protected_topic", &updated.id, &updated)?;

        if !self
            .persistence
            .record_exists("protected_topic_entry", &secret.id)?
        {
            let entry = ProtectedTopicEntryDoc {
                id: secret.id.clone(),
                topic_name: secret.plan.topic_name.clone(),
                session_id: secret.session_id.clone(),
                role: secret.role.clone(),
                source: secret.source.clone(),
                secret_kind: secret.plan.secret_kind.as_str().to_string(),
                redacted_excerpt: secret.plan.redacted_excerpt.clone(),
                preview: secret.plan.preview.clone(),
                ciphertext: secret.ciphertext.clone(),
                created_at,
            };
            self.persistence
                .create("protected_topic_entry", &entry.id, &entry)?;
        }

        Ok(())
    }

    fn prepare_turn_for_storage(
        &self,
        turn: &ConversationTurn,
        source: &str,
    ) -> Result<(ConversationTurn, Option<PreparedProtectedSecret>)> {
        let Some(entity_id) = self.entity_id.as_deref() else {
            return Ok((turn.clone(), None));
        };
        let Some(secret) = self.prepare_secret_capture(
            entity_id,
            &turn.session_id,
            &turn.role,
            &turn.content,
            source,
            turn.created_at,
        )?
        else {
            return Ok((turn.clone(), None));
        };

        let mut persisted_turn = turn.clone();
        persisted_turn.content = secret.plan.redacted_excerpt.clone();
        Ok((persisted_turn, Some(secret)))
    }

    fn prepare_secret_capture(
        &self,
        entity_id: &str,
        session_id: &str,
        role: &str,
        content: &str,
        source: &str,
        created_at: chrono::DateTime<Utc>,
    ) -> Result<Option<PreparedProtectedSecret>> {
        let Some(plan) = plan_secret_move(Some(entity_id), content) else {
            return Ok(None);
        };
        let ciphertext = encrypt_secret_payload(
            &self.unlock,
            &plan,
            session_id,
            role,
            source,
            content,
            created_at,
        )
        .map_err(StoreError::ProtectedTopic)?;

        Ok(Some(PreparedProtectedSecret {
            id: stable_secret_id(
                &plan.topic_name,
                session_id,
                role,
                source,
                &created_at.to_rfc3339(),
                content,
            ),
            plan,
            session_id: session_id.to_string(),
            role: role.to_string(),
            source: source.to_string(),
            ciphertext,
            created_at,
        }))
    }
}

impl Drop for MemoryStore {
    fn drop(&mut self) {
        if let Some(root) = self.temp_root.take() {
            let _ = std::fs::remove_dir_all(root);
        }
    }
}

#[derive(Default)]
struct SessionAccumulator {
    turn_count: u64,
    first_at: String,
    last_at: String,
}

impl SessionAccumulator {
    fn new(created_at: String) -> Self {
        Self {
            turn_count: 0,
            first_at: created_at.clone(),
            last_at: created_at,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct MemoryDoc {
    id: String,
    content: String,
    weight: String,
    created_at: String,
}

impl From<&Memory> for MemoryDoc {
    fn from(value: &Memory) -> Self {
        Self {
            id: value.id.clone(),
            content: value.content.clone(),
            weight: value.weight.as_str().to_string(),
            created_at: value.created_at.to_rfc3339(),
        }
    }
}

impl TryFrom<MemoryDoc> for Memory {
    type Error = StoreError;

    fn try_from(value: MemoryDoc) -> Result<Self> {
        Ok(Self {
            id: value.id,
            content: value.content,
            weight: parse_weight(&value.weight)?,
            created_at: parse_timestamp(&value.created_at)?,
        })
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct TurnDoc {
    id: String,
    session_id: String,
    turn_number: u32,
    role: String,
    content: String,
    provider: Option<String>,
    model: Option<String>,
    tier: Option<String>,
    complexity_score: Option<u8>,
    token_estimate: Option<u32>,
    created_at: String,
}

impl From<&ConversationTurn> for TurnDoc {
    fn from(value: &ConversationTurn) -> Self {
        Self {
            id: value.id.clone(),
            session_id: value.session_id.clone(),
            turn_number: value.turn_number,
            role: value.role.clone(),
            content: value.content.clone(),
            provider: value.provider.clone(),
            model: value.model.clone(),
            tier: value.tier.clone(),
            complexity_score: value.complexity_score,
            token_estimate: value.token_estimate,
            created_at: value.created_at.to_rfc3339(),
        }
    }
}

impl TryFrom<TurnDoc> for ConversationTurn {
    type Error = StoreError;

    fn try_from(value: TurnDoc) -> Result<Self> {
        Ok(Self {
            id: value.id,
            session_id: value.session_id,
            turn_number: value.turn_number,
            role: value.role,
            content: value.content,
            provider: value.provider,
            model: value.model,
            tier: value.tier,
            complexity_score: value.complexity_score,
            token_estimate: value.token_estimate,
            created_at: parse_timestamp(&value.created_at)?,
        })
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct BirthDoc {
    id: String,
    content: String,
    created_at: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct ProtectedTopicDoc {
    id: String,
    topic_name: String,
    entity_id: String,
    protection_kind: String,
    entry_count: u64,
    last_secret_kind: String,
    last_redacted_excerpt: String,
    last_preview: TriangleEthicPreview,
    created_at: String,
    updated_at: String,
}

impl TryFrom<ProtectedTopicDoc> for ProtectedTopicSummary {
    type Error = StoreError;

    fn try_from(value: ProtectedTopicDoc) -> Result<Self> {
        Ok(Self {
            topic_name: value.topic_name,
            entity_id: value.entity_id,
            entry_count: value.entry_count,
            updated_at: parse_timestamp(&value.updated_at)?,
            last_secret_kind: parse_secret_kind(&value.last_secret_kind)?,
            last_redacted_excerpt: value.last_redacted_excerpt,
            last_preview: value.last_preview,
        })
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct ProtectedTopicEntryDoc {
    id: String,
    topic_name: String,
    session_id: String,
    role: String,
    source: String,
    secret_kind: String,
    redacted_excerpt: String,
    preview: TriangleEthicPreview,
    ciphertext: Vec<u8>,
    created_at: String,
}

struct PreparedProtectedSecret {
    id: String,
    plan: SecretMovePlan,
    session_id: String,
    role: String,
    source: String,
    ciphertext: Vec<u8>,
    created_at: chrono::DateTime<Utc>,
}

fn infer_entity_id(path: &Path) -> Option<String> {
    path.parent()
        .and_then(|parent| parent.file_name())
        .and_then(|name| name.to_str())
        .filter(|candidate| Uuid::parse_str(candidate).is_ok())
        .map(str::to_string)
}

fn synthetic_file_scope(path: &Path) -> Option<String> {
    path.file_stem().and_then(|name| name.to_str()).map(|name| {
        format!(
            "file_{}",
            name.replace(|c: char| !c.is_ascii_alphanumeric(), "_")
        )
    })
}

fn parse_weight(value: &str) -> Result<MemoryWeight> {
    match value {
        "ephemeral" => Ok(MemoryWeight::Ephemeral),
        "distilled" => Ok(MemoryWeight::Distilled),
        "crystallized" => Ok(MemoryWeight::Crystallized),
        other => Err(StoreError::InvalidData(format!(
            "unknown weight: {}",
            other
        ))),
    }
}

fn parse_secret_kind(value: &str) -> Result<SecretKind> {
    match value {
        "api_key" => Ok(SecretKind::ApiKey),
        "password" => Ok(SecretKind::Password),
        "token" => Ok(SecretKind::Token),
        "credential" => Ok(SecretKind::Credential),
        other => Err(StoreError::InvalidData(format!(
            "unknown secret kind: {}",
            other
        ))),
    }
}

fn parse_timestamp(value: &str) -> Result<chrono::DateTime<Utc>> {
    chrono::DateTime::parse_from_rfc3339(value)
        .map(|timestamp| timestamp.with_timezone(&Utc))
        .map_err(|error| StoreError::InvalidData(error.to_string()))
}

fn binding(key: &str, value: impl Serialize) -> Result<QueryBinding> {
    QueryBinding::new(key, value).map_err(StoreError::from)
}

fn contains_case_insensitive(content: &str, query: &str) -> bool {
    content.to_ascii_lowercase().contains(query)
}

fn stable_secret_id(
    topic_name: &str,
    session_id: &str,
    role: &str,
    source: &str,
    created_at: &str,
    content: &str,
) -> String {
    let mut hasher = Sha256::new();
    hasher.update(topic_name.as_bytes());
    hasher.update(b"|");
    hasher.update(session_id.as_bytes());
    hasher.update(b"|");
    hasher.update(role.as_bytes());
    hasher.update(b"|");
    hasher.update(source.as_bytes());
    hasher.update(b"|");
    hasher.update(created_at.as_bytes());
    hasher.update(b"|");
    hasher.update(content.as_bytes());
    let digest = hasher.finalize();
    digest.iter().map(|byte| format!("{:02x}", byte)).collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use abigail_core::AppConfig;

    fn test_config(base: &Path, db_path: PathBuf) -> AppConfig {
        let mut config = AppConfig::default_paths();
        config.data_dir = base.join("identities").join(Uuid::new_v4().to_string());
        config.models_dir = base.join("models");
        config.docs_dir = base.join("docs");
        config.db_path = db_path;
        config
    }

    #[test]
    fn test_birth_record() {
        let store = MemoryStore::open_in_memory().unwrap();
        assert!(!store.has_birth().unwrap());
        store
            .record_birth(&Memory::crystallized("I was born".into()))
            .unwrap();
        assert!(store.has_birth().unwrap());
    }

    #[test]
    fn test_insert_and_retrieve_memory() {
        let store = MemoryStore::open_in_memory().unwrap();
        let memory = Memory::ephemeral("hello".into());
        store.insert_memory(&memory).unwrap();

        let recent = store.recent_memories(10).unwrap();
        assert_eq!(recent.len(), 1);
        assert_eq!(recent[0].content, "hello");
    }

    #[test]
    fn test_insert_turn_or_ignore_idempotent() {
        let store = MemoryStore::open_in_memory().unwrap();
        let turn = ConversationTurn::new("session-1", "user", "hello");

        assert!(store.insert_turn_or_ignore(&turn).unwrap());
        assert!(!store.insert_turn_or_ignore(&turn).unwrap());
        assert_eq!(store.total_turn_count().unwrap(), 1);
    }

    #[test]
    #[ignore = "SurrealKV releases file locks asynchronously on drop; close-and-reopen races in CI"]
    fn test_file_backed_store_persists() {
        let tmp = std::env::temp_dir().join(format!("abigail_store_file_{}", Uuid::new_v4()));
        let db_path = tmp.join("store");
        std::fs::create_dir_all(&tmp).unwrap();

        {
            let store = MemoryStore::open(&db_path).unwrap();
            store
                .insert_memory(&Memory::ephemeral("persisted".into()))
                .unwrap();
        }

        {
            let reopened = MemoryStore::open(&db_path).unwrap();
            assert_eq!(reopened.count_memories().unwrap(), 1);
        }

        let _ = std::fs::remove_dir_all(&tmp);
    }

    #[test]
    fn test_open_with_config_uses_configured_db_path() {
        let tmp = std::env::temp_dir().join(format!("abigail_store_path_{}", Uuid::new_v4()));
        let db_path = tmp.join("sandbox").join("test.db");
        let config = test_config(&tmp, db_path.clone());
        let store = MemoryStore::open_with_config(&config).unwrap();

        assert_eq!(store.path(), db_path.as_path());

        let _ = std::fs::remove_dir_all(&tmp);
    }
}
