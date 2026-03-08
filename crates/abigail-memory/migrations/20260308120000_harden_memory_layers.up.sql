CREATE EXTENSION IF NOT EXISTS pgcrypto;
CREATE EXTENSION IF NOT EXISTS vector;

DO $$
BEGIN
    IF NOT EXISTS (SELECT 1 FROM pg_type WHERE typname = 'memory_layer') THEN
        CREATE TYPE memory_layer AS ENUM (
            'Working',
            'Episodic',
            'Semantic',
            'Crystallized'
        );
    END IF;
END
$$;

CREATE TABLE IF NOT EXISTS memory_entries (
    id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    entity_id UUID NOT NULL,
    layer memory_layer NOT NULL,
    content TEXT NOT NULL,
    embedding VECTOR(1536),
    timestamp TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    metadata JSONB NOT NULL DEFAULT '{}'::jsonb,
    pii_tagged BOOLEAN NOT NULL DEFAULT FALSE,
    ethical_hash TEXT,
    version INTEGER NOT NULL DEFAULT 1
);

CREATE UNIQUE INDEX IF NOT EXISTS idx_memory_entries_crystallized_unique
    ON memory_entries (entity_id, layer)
    WHERE layer = 'Crystallized';

CREATE INDEX IF NOT EXISTS idx_memory_entity_layer
    ON memory_entries (entity_id, layer);

CREATE INDEX IF NOT EXISTS idx_memory_embedding
    ON memory_entries USING hnsw (embedding vector_cosine_ops);

CREATE INDEX IF NOT EXISTS idx_memory_timestamp
    ON memory_entries (timestamp);
