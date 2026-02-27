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
