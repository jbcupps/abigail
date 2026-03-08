// Schema and migrations for SQLite. sqlite-vec integration stubbed for MVP; retrieval by recency/weight.

pub const CREATE_MEMORIES: &str = r#"
CREATE TABLE IF NOT EXISTS memories (
    id TEXT PRIMARY KEY,
    content TEXT NOT NULL,
    weight TEXT NOT NULL CHECK (weight IN ('ephemeral', 'distilled', 'crystallized')),
    created_at TEXT NOT NULL
);
CREATE INDEX IF NOT EXISTS idx_memories_created_at ON memories(created_at DESC);
"#;

pub const CREATE_BIRTH: &str = r#"
CREATE TABLE IF NOT EXISTS birth (
    id INTEGER PRIMARY KEY CHECK (id = 1),
    content TEXT NOT NULL,
    created_at TEXT NOT NULL
);
"#;

pub const CREATE_SCHEMA_VERSIONS: &str = r#"
CREATE TABLE IF NOT EXISTS schema_versions (
    version INTEGER PRIMARY KEY,
    applied_at TEXT NOT NULL
);
"#;

pub const MIGRATION_V2_CONVERSATION_TURNS: &str = r#"
CREATE TABLE IF NOT EXISTS conversation_turns (
    id TEXT PRIMARY KEY,
    session_id TEXT NOT NULL,
    turn_number INTEGER NOT NULL,
    role TEXT NOT NULL CHECK (role IN ('user', 'assistant', 'system')),
    content TEXT NOT NULL,
    provider TEXT,
    model TEXT,
    tier TEXT,
    complexity_score INTEGER,
    token_estimate INTEGER,
    created_at TEXT NOT NULL
);
CREATE INDEX IF NOT EXISTS idx_turns_session ON conversation_turns(session_id, turn_number);
CREATE INDEX IF NOT EXISTS idx_turns_time ON conversation_turns(created_at DESC);
"#;

pub const MIGRATION_V3_JOB_QUEUE: &str = r#"
CREATE TABLE IF NOT EXISTS job_queue (
    id TEXT PRIMARY KEY,
    topic TEXT NOT NULL,
    goal TEXT NOT NULL,
    capability TEXT NOT NULL DEFAULT 'general',
    priority INTEGER NOT NULL DEFAULT 1,
    status TEXT NOT NULL DEFAULT 'queued'
        CHECK (status IN ('queued','running','completed','failed','cancelled','expired')),
    time_budget_ms INTEGER NOT NULL DEFAULT 120000,
    max_turns INTEGER NOT NULL DEFAULT 10,
    system_context TEXT,
    allowed_skill_ids TEXT,
    input_data TEXT,
    parent_job_id TEXT,
    agent_id TEXT,
    model_used TEXT,
    provider_used TEXT,
    result TEXT,
    error TEXT,
    turns_consumed INTEGER NOT NULL DEFAULT 0,
    ttl_seconds INTEGER NOT NULL DEFAULT 3600,
    created_at TEXT NOT NULL,
    started_at TEXT,
    completed_at TEXT,
    expires_at TEXT NOT NULL,
    FOREIGN KEY (parent_job_id) REFERENCES job_queue(id)
);

CREATE INDEX IF NOT EXISTS idx_job_queue_pending
    ON job_queue(status, priority DESC, created_at ASC)
    WHERE status = 'queued';

CREATE INDEX IF NOT EXISTS idx_job_queue_topic
    ON job_queue(topic, created_at DESC);

CREATE INDEX IF NOT EXISTS idx_job_queue_expires
    ON job_queue(expires_at)
    WHERE status = 'queued';

CREATE INDEX IF NOT EXISTS idx_job_queue_running
    ON job_queue(status)
    WHERE status = 'running';
"#;

pub const MIGRATION_V4_PROTECTED_TOPICS: &str = r#"
CREATE TABLE IF NOT EXISTS protected_topics (
    topic_name TEXT PRIMARY KEY,
    entity_id TEXT NOT NULL,
    protection_kind TEXT NOT NULL DEFAULT 'secret'
        CHECK (protection_kind IN ('secret')),
    entry_count INTEGER NOT NULL DEFAULT 0,
    last_secret_kind TEXT NOT NULL,
    last_redacted_excerpt TEXT NOT NULL,
    last_preview_json TEXT NOT NULL,
    created_at TEXT NOT NULL,
    updated_at TEXT NOT NULL
);

CREATE TABLE IF NOT EXISTS protected_topic_entries (
    id TEXT PRIMARY KEY,
    topic_name TEXT NOT NULL,
    session_id TEXT NOT NULL,
    role TEXT NOT NULL,
    source TEXT NOT NULL,
    secret_kind TEXT NOT NULL,
    redacted_excerpt TEXT NOT NULL,
    preview_json TEXT NOT NULL,
    ciphertext BLOB NOT NULL,
    created_at TEXT NOT NULL,
    FOREIGN KEY (topic_name) REFERENCES protected_topics(topic_name) ON DELETE CASCADE
);

CREATE INDEX IF NOT EXISTS idx_protected_topic_entries_topic
    ON protected_topic_entries(topic_name, created_at DESC);

CREATE INDEX IF NOT EXISTS idx_protected_topic_entries_session
    ON protected_topic_entries(session_id, created_at DESC);
"#;

pub const MIGRATION_V5_CONVERSATION_TURN_ROLES: &str = r#"
DROP INDEX IF EXISTS idx_turns_session;
DROP INDEX IF EXISTS idx_turns_time;

ALTER TABLE conversation_turns RENAME TO conversation_turns_v2_old;

CREATE TABLE conversation_turns (
    id TEXT PRIMARY KEY,
    session_id TEXT NOT NULL,
    turn_number INTEGER NOT NULL,
    role TEXT NOT NULL CHECK (role IN ('user', 'assistant', 'system', 'mentor_monitor')),
    content TEXT NOT NULL,
    provider TEXT,
    model TEXT,
    tier TEXT,
    complexity_score INTEGER,
    token_estimate INTEGER,
    created_at TEXT NOT NULL
);

INSERT INTO conversation_turns (
    id,
    session_id,
    turn_number,
    role,
    content,
    provider,
    model,
    tier,
    complexity_score,
    token_estimate,
    created_at
)
SELECT
    id,
    session_id,
    turn_number,
    role,
    content,
    provider,
    model,
    tier,
    complexity_score,
    token_estimate,
    created_at
FROM conversation_turns_v2_old;

DROP TABLE conversation_turns_v2_old;

CREATE INDEX IF NOT EXISTS idx_turns_session ON conversation_turns(session_id, turn_number);
CREATE INDEX IF NOT EXISTS idx_turns_time ON conversation_turns(created_at DESC);
"#;
