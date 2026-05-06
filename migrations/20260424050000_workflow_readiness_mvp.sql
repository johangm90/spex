-- Migration: workflow_readiness_mvp
-- Adds additive schema for phase state, review requirements, and session checkpoints

CREATE TABLE IF NOT EXISTS workflow_phases (
    id TEXT PRIMARY KEY,
    spec_id TEXT NOT NULL REFERENCES specs(id),
    phase TEXT NOT NULL,           -- 'planning'|'in_progress'|'review'|'done'
    entered_at TEXT NOT NULL,
    exited_at TEXT,
    entered_by TEXT,               -- agent or human
    notes TEXT,
    created_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ','now'))
);

CREATE INDEX IF NOT EXISTS idx_workflow_phases_spec_id ON workflow_phases(spec_id);

CREATE TABLE IF NOT EXISTS review_requirements (
    id TEXT PRIMARY KEY,
    spec_id TEXT NOT NULL REFERENCES specs(id),
    kind TEXT NOT NULL,            -- 'test_pass'|'lint_pass'|'review_approved'|'custom'
    description TEXT NOT NULL,
    satisfied INTEGER NOT NULL DEFAULT 0,   -- 0=false, 1=true
    satisfied_at TEXT,
    satisfied_by TEXT,
    created_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ','now'))
);

CREATE INDEX IF NOT EXISTS idx_review_requirements_spec_id ON review_requirements(spec_id);

CREATE TABLE IF NOT EXISTS session_checkpoints (
    id TEXT PRIMARY KEY,
    session_id TEXT NOT NULL REFERENCES sessions(id),
    spec_id TEXT REFERENCES specs(id),
    task_id TEXT REFERENCES tasks(id),
    agent TEXT NOT NULL,
    checkpoint_data TEXT NOT NULL,   -- JSON blob
    saved_at TEXT NOT NULL,
    label TEXT,                      -- human-readable label e.g. "before T062"
    created_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ','now'))
);

CREATE INDEX IF NOT EXISTS idx_session_checkpoints_session_id ON session_checkpoints(session_id);
CREATE INDEX IF NOT EXISTS idx_session_checkpoints_spec_id ON session_checkpoints(spec_id);
CREATE INDEX IF NOT EXISTS idx_session_checkpoints_agent ON session_checkpoints(agent);
CREATE INDEX IF NOT EXISTS idx_session_checkpoints_saved_at ON session_checkpoints(saved_at);
