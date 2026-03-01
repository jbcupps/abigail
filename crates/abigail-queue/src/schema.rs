//! SQLite migration V3: job queue table.

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
