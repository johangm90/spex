-- Extend spec/task lifecycle for operational exception handling
-- Existing rows remain valid; these comments document the new allowed values.

-- specs.status: draft|approved|in_progress|blocked|stabilizing|done|paused|discarded|superseded
-- tasks.status: pending|in_progress|blocked|review|verified|done|cancelled

CREATE TABLE IF NOT EXISTS incidents (
    id TEXT PRIMARY KEY,
    spec_id TEXT NOT NULL REFERENCES specs(id) ON DELETE CASCADE,
    task_id TEXT REFERENCES tasks(id) ON DELETE SET NULL,
    title TEXT NOT NULL,
    severity TEXT NOT NULL CHECK (severity IN ('low', 'medium', 'high', 'critical')),
    status TEXT NOT NULL CHECK (status IN (
        'new', 'triaged', 'linked_to_spec', 'fix_planned', 'fix_in_progress',
        'verifying', 'resolved', 'deferred', 'duplicate', 'not_reproducible'
    )),
    source TEXT NOT NULL CHECK (source IN (
        'spec_defect', 'implementation_defect', 'verification_gap',
        'documentation_gap', 'environment', 'unknown'
    )),
    blocking INTEGER NOT NULL DEFAULT 0 CHECK (blocking IN (0, 1)),
    repro_steps TEXT,
    root_cause TEXT,
    fix_strategy TEXT,
    created_at TEXT NOT NULL,
    updated_at TEXT NOT NULL
);
CREATE INDEX IF NOT EXISTS idx_incidents_spec_id ON incidents(spec_id);
CREATE INDEX IF NOT EXISTS idx_incidents_task_id ON incidents(task_id);
CREATE INDEX IF NOT EXISTS idx_incidents_status ON incidents(status);
CREATE INDEX IF NOT EXISTS idx_incidents_severity ON incidents(severity);

CREATE TABLE IF NOT EXISTS context_gaps (
    id TEXT PRIMARY KEY,
    spec_id TEXT NOT NULL REFERENCES specs(id) ON DELETE CASCADE,
    task_id TEXT REFERENCES tasks(id) ON DELETE SET NULL,
    kind TEXT NOT NULL CHECK (kind IN (
        'missing_doc', 'outdated_doc', 'contradictory_doc', 'undocumented_behavior'
    )),
    criticality TEXT NOT NULL CHECK (criticality IN ('low', 'medium', 'high')),
    status TEXT NOT NULL CHECK (status IN ('open', 'triaged', 'assumption_recorded', 'resolved', 'wont_fix')),
    blocking INTEGER NOT NULL DEFAULT 0 CHECK (blocking IN (0, 1)),
    question TEXT NOT NULL,
    assumption TEXT,
    resolution TEXT,
    created_at TEXT NOT NULL,
    updated_at TEXT NOT NULL
);
CREATE INDEX IF NOT EXISTS idx_context_gaps_spec_id ON context_gaps(spec_id);
CREATE INDEX IF NOT EXISTS idx_context_gaps_task_id ON context_gaps(task_id);
CREATE INDEX IF NOT EXISTS idx_context_gaps_status ON context_gaps(status);

CREATE TABLE IF NOT EXISTS verification_runs (
    id TEXT PRIMARY KEY,
    spec_id TEXT NOT NULL REFERENCES specs(id) ON DELETE CASCADE,
    task_id TEXT REFERENCES tasks(id) ON DELETE SET NULL,
    slice_id TEXT,
    kind TEXT NOT NULL CHECK (kind IN (
        'static', 'unit', 'integration', 'contract', 'e2e', 'smoke',
        'migration', 'docs', 'observability'
    )),
    status TEXT NOT NULL CHECK (status IN ('pass', 'pass_with_risk', 'fail', 'flaky', 'blocked')),
    command TEXT,
    summary TEXT NOT NULL,
    evidence TEXT,
    created_at TEXT NOT NULL
);
CREATE INDEX IF NOT EXISTS idx_verification_runs_spec_id ON verification_runs(spec_id);
CREATE INDEX IF NOT EXISTS idx_verification_runs_task_id ON verification_runs(task_id);
CREATE INDEX IF NOT EXISTS idx_verification_runs_kind ON verification_runs(kind);
CREATE INDEX IF NOT EXISTS idx_verification_runs_status ON verification_runs(status);

CREATE TABLE IF NOT EXISTS interrupts (
    id TEXT PRIMARY KEY,
    spec_id TEXT NOT NULL REFERENCES specs(id) ON DELETE CASCADE,
    reason_type TEXT NOT NULL CHECK (reason_type IN (
        'emergency', 'customer_critical', 'revenue', 'incident', 'strategy', 'dependency'
    )),
    status TEXT NOT NULL CHECK (status IN ('open', 'active', 'resolved', 'cancelled')),
    preempted_tasks TEXT NOT NULL DEFAULT '[]',
    resume_hint TEXT,
    created_at TEXT NOT NULL,
    updated_at TEXT NOT NULL
);
CREATE INDEX IF NOT EXISTS idx_interrupts_spec_id ON interrupts(spec_id);
CREATE INDEX IF NOT EXISTS idx_interrupts_status ON interrupts(status);

CREATE TABLE IF NOT EXISTS handoff_snapshots (
    id TEXT PRIMARY KEY,
    spec_id TEXT NOT NULL REFERENCES specs(id) ON DELETE CASCADE,
    interrupt_id TEXT REFERENCES interrupts(id) ON DELETE SET NULL,
    last_wave INTEGER,
    last_task TEXT,
    files_touched TEXT NOT NULL DEFAULT '[]',
    decisions TEXT NOT NULL DEFAULT '[]',
    open_risks TEXT NOT NULL DEFAULT '[]',
    next_steps TEXT NOT NULL DEFAULT '[]',
    created_at TEXT NOT NULL
);
CREATE INDEX IF NOT EXISTS idx_handoff_snapshots_spec_id ON handoff_snapshots(spec_id);
CREATE INDEX IF NOT EXISTS idx_handoff_snapshots_interrupt_id ON handoff_snapshots(interrupt_id);
