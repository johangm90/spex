CREATE TABLE memory_new (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    agent TEXT NOT NULL,
    key TEXT NOT NULL,
    value TEXT NOT NULL,
    spec TEXT NOT NULL DEFAULT '',
    updated_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ', 'now')),
    UNIQUE(agent, spec, key)
);

INSERT INTO memory_new (id, agent, key, value, spec, updated_at)
SELECT id, agent, key, value, COALESCE(spec, ''), updated_at
FROM memory;

DROP TABLE memory;
ALTER TABLE memory_new RENAME TO memory;

CREATE INDEX IF NOT EXISTS idx_memory_agent_spec ON memory(agent, spec);
CREATE INDEX IF NOT EXISTS idx_memory_agent_updated_at ON memory(agent, updated_at DESC);
