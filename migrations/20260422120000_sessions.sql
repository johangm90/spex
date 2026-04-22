-- Add sessions table for tracking agent/human work sessions
CREATE TABLE IF NOT EXISTS sessions (
    id              TEXT PRIMARY KEY,
    agent           TEXT NOT NULL,
    spec_id         TEXT REFERENCES specs(id),
    task_id         TEXT REFERENCES tasks(id),
    host            TEXT,
    started_at      TEXT NOT NULL,
    ended_at        TEXT,
    duration_secs   INTEGER,
    notes           TEXT,
    created_at      TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ','now'))
);

CREATE INDEX IF NOT EXISTS idx_sessions_spec_id    ON sessions(spec_id);
CREATE INDEX IF NOT EXISTS idx_sessions_agent       ON sessions(agent);
CREATE INDEX IF NOT EXISTS idx_sessions_started_at  ON sessions(started_at);
