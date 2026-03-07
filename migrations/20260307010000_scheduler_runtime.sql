-- Scheduler/runtime support for safe parallel orchestration

CREATE TABLE IF NOT EXISTS plan_versions (
    id TEXT PRIMARY KEY,
    spec_id TEXT NOT NULL REFERENCES specs(id) ON DELETE CASCADE,
    version INTEGER NOT NULL,
    status TEXT NOT NULL CHECK (status IN ('active', 'superseded')),
    reason TEXT,
    plan_json TEXT NOT NULL,
    created_at TEXT NOT NULL,
    UNIQUE(spec_id, version)
);
CREATE INDEX IF NOT EXISTS idx_plan_versions_spec_id ON plan_versions(spec_id, version DESC);

CREATE TABLE IF NOT EXISTS task_leases (
    task_id TEXT PRIMARY KEY REFERENCES tasks(id) ON DELETE CASCADE,
    agent_id TEXT NOT NULL,
    status TEXT NOT NULL CHECK (status IN ('claimed', 'running', 'expired', 'released')),
    lease_expires_at TEXT NOT NULL,
    heartbeat_at TEXT NOT NULL,
    attempt_count INTEGER NOT NULL DEFAULT 1,
    created_at TEXT NOT NULL,
    updated_at TEXT NOT NULL
);
CREATE INDEX IF NOT EXISTS idx_task_leases_status ON task_leases(status, lease_expires_at);

CREATE TABLE IF NOT EXISTS task_locks (
    id TEXT PRIMARY KEY,
    task_id TEXT NOT NULL REFERENCES tasks(id) ON DELETE CASCADE,
    spec_id TEXT NOT NULL REFERENCES specs(id) ON DELETE CASCADE,
    lock_type TEXT NOT NULL CHECK (lock_type IN ('module', 'semantic', 'file')),
    resource TEXT NOT NULL,
    status TEXT NOT NULL CHECK (status IN ('active', 'released')),
    acquired_at TEXT NOT NULL,
    released_at TEXT
);
CREATE INDEX IF NOT EXISTS idx_task_locks_active ON task_locks(spec_id, lock_type, resource, status);

CREATE TABLE IF NOT EXISTS replan_requests (
    id TEXT PRIMARY KEY,
    spec_id TEXT NOT NULL REFERENCES specs(id) ON DELETE CASCADE,
    task_id TEXT REFERENCES tasks(id) ON DELETE SET NULL,
    agent_id TEXT NOT NULL,
    reason TEXT NOT NULL,
    impact TEXT NOT NULL DEFAULT '[]',
    proposed_action TEXT,
    status TEXT NOT NULL CHECK (status IN ('open', 'accepted', 'rejected', 'resolved')),
    created_at TEXT NOT NULL,
    updated_at TEXT NOT NULL
);
CREATE INDEX IF NOT EXISTS idx_replan_requests_spec_id ON replan_requests(spec_id, status);
