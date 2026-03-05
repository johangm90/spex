-- Constitution / PRD
CREATE TABLE IF NOT EXISTS constitution (
    id TEXT PRIMARY KEY DEFAULT 'main',
    content TEXT NOT NULL DEFAULT '',
    status TEXT NOT NULL DEFAULT 'draft', -- draft|active|frozen
    version INTEGER NOT NULL DEFAULT 1,
    created_at TEXT NOT NULL,
    updated_at TEXT NOT NULL
);

-- Specs (equivalent to slices in aiteam)
CREATE TABLE IF NOT EXISTS specs (
    id TEXT PRIMARY KEY,                     -- e.g. SPEC-001
    title TEXT NOT NULL,
    status TEXT NOT NULL DEFAULT 'draft',    -- draft|approved|in_progress|done|paused
    priority TEXT NOT NULL DEFAULT 'P1',     -- P0|P1|P2|P3
    depends_on TEXT NOT NULL DEFAULT '[]',   -- JSON array of spec IDs
    agents TEXT NOT NULL DEFAULT '[]',       -- JSON array of agent names
    ac_total INTEGER NOT NULL DEFAULT 0,
    ac_passed INTEGER NOT NULL DEFAULT 0,
    created_at TEXT NOT NULL,
    updated_at TEXT NOT NULL,
    updated_by TEXT
);

-- Tasks within a spec
CREATE TABLE IF NOT EXISTS tasks (
    id TEXT PRIMARY KEY,
    spec TEXT NOT NULL REFERENCES specs(id) ON DELETE CASCADE,
    title TEXT NOT NULL,
    agent TEXT NOT NULL,
    status TEXT NOT NULL DEFAULT 'pending',  -- pending|in_progress|done|failed
    inputs TEXT NOT NULL DEFAULT '[]',       -- JSON array of input artifact IDs
    output_artifact TEXT,
    created_at TEXT NOT NULL,
    updated_at TEXT NOT NULL
);

-- Append-only domain event log
CREATE TABLE IF NOT EXISTS events (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    type TEXT NOT NULL,
    spec TEXT,
    agent TEXT,
    payload TEXT NOT NULL DEFAULT '{}',
    timestamp TEXT NOT NULL
);
CREATE INDEX IF NOT EXISTS idx_events_type ON events(type);
CREATE INDEX IF NOT EXISTS idx_events_spec ON events(spec);
CREATE INDEX IF NOT EXISTS idx_events_agent ON events(agent);
CREATE INDEX IF NOT EXISTS idx_events_timestamp ON events(timestamp);

-- Per-agent KV scratchpad (last-write-wins)
CREATE TABLE IF NOT EXISTS memory (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    agent TEXT NOT NULL,
    key TEXT NOT NULL,
    value TEXT NOT NULL,
    spec TEXT,
    updated_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ', 'now')),
    UNIQUE(agent, key)
);

-- Registered output artifacts
CREATE TABLE IF NOT EXISTS artifacts (
    id TEXT PRIMARY KEY,
    spec TEXT NOT NULL,
    task TEXT,
    agent TEXT NOT NULL,
    type TEXT NOT NULL,
    path TEXT,
    description TEXT,
    created_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ', 'now'))
);
CREATE INDEX IF NOT EXISTS idx_artifacts_spec ON artifacts(spec);
CREATE INDEX IF NOT EXISTS idx_artifacts_agent ON artifacts(agent);
CREATE INDEX IF NOT EXISTS idx_artifacts_type ON artifacts(type);

-- Project metadata
CREATE TABLE IF NOT EXISTS meta (
    key TEXT PRIMARY KEY,
    value TEXT NOT NULL
);
