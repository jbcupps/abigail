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
