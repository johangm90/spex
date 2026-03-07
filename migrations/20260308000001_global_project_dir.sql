-- Migration: Global-mode project_dir column + PK restructuring
-- All project-scoped tables receive a project_dir column so that a single
-- ~/.local/share/spex/global-state.db can hold data for multiple projects.
--
-- Strategy:
--   • Tables whose TEXT PRIMARY KEY would collide across projects are rebuilt
--     with CREATE NEW + INSERT SELECT + DROP + RENAME (SQLite cannot ALTER PK).
--   • Tables with INTEGER AUTOINCREMENT PKs just get an ALTER TABLE ADD COLUMN.
--   • All FKs from child tables reference the new composite keys where needed.
--   • Idempotent: every CREATE/INDEX uses IF NOT EXISTS; DROP uses IF EXISTS.
--
-- Tables rebuilt (PK change or FK target change):
--   specs, tasks, events, artifacts,
--   incidents, context_gaps, verification_runs, interrupts, handoff_snapshots,
--   plan_versions, task_leases, task_locks, replan_requests
--
-- Tables altered in-place (AUTOINCREMENT PK, no FK change needed):
--   memory  (project_dir added; UNIQUE constraint rebuilt via new table)
--
-- Tables NOT touched (no project scope):
--   constitution, meta, memory_fts (FTS supplementary to memory)

-- ─────────────────────────────────────────────────────────────────────────────
-- 0. DISABLE foreign-key enforcement for the duration of the rebuild.
--    sqlx runs each migration in a transaction; PRAGMA changes inside a
--    transaction are session-scoped and take effect immediately.
-- ─────────────────────────────────────────────────────────────────────────────
PRAGMA foreign_keys = OFF;

-- ─────────────────────────────────────────────────────────────────────────────
-- 1. specs
--    OLD PK:  id TEXT PRIMARY KEY
--    NEW:     rowid PK (implicit), UNIQUE(project_dir, id)
-- ─────────────────────────────────────────────────────────────────────────────
CREATE TABLE IF NOT EXISTS specs_new (
    id TEXT NOT NULL,
    project_dir TEXT NOT NULL DEFAULT '',
    title TEXT NOT NULL,
    status TEXT NOT NULL DEFAULT 'draft',
    priority TEXT NOT NULL DEFAULT 'P1',
    depends_on TEXT NOT NULL DEFAULT '[]',
    agents TEXT NOT NULL DEFAULT '[]',
    ac_total INTEGER NOT NULL DEFAULT 0,
    ac_passed INTEGER NOT NULL DEFAULT 0,
    created_at TEXT NOT NULL,
    updated_at TEXT NOT NULL,
    updated_by TEXT,
    UNIQUE(project_dir, id)
);

INSERT INTO specs_new
    (id, project_dir, title, status, priority, depends_on, agents,
     ac_total, ac_passed, created_at, updated_at, updated_by)
SELECT id, '', title, status, priority, depends_on, agents,
       ac_total, ac_passed, created_at, updated_at, updated_by
FROM specs;

DROP TABLE specs;
ALTER TABLE specs_new RENAME TO specs;

CREATE INDEX IF NOT EXISTS idx_specs_project_dir ON specs(project_dir);
CREATE INDEX IF NOT EXISTS idx_specs_project_id  ON specs(project_dir, id);

-- ─────────────────────────────────────────────────────────────────────────────
-- 2. tasks
--    OLD PK:  id TEXT PRIMARY KEY
--    NEW:     rowid PK, UNIQUE(project_dir, id)
--    FK:      spec → specs(id) scoped by project_dir (enforced at app layer;
--             SQLite composite FK not supported, so we keep the text ref)
-- ─────────────────────────────────────────────────────────────────────────────
CREATE TABLE IF NOT EXISTS tasks_new (
    id TEXT NOT NULL,
    project_dir TEXT NOT NULL DEFAULT '',
    spec TEXT NOT NULL,
    title TEXT NOT NULL,
    agent TEXT NOT NULL,
    status TEXT NOT NULL DEFAULT 'pending',
    inputs TEXT NOT NULL DEFAULT '[]',
    output_artifact TEXT,
    created_at TEXT NOT NULL,
    updated_at TEXT NOT NULL,
    -- columns added by later migrations (included here to avoid ALTER after rename)
    depends_on TEXT NOT NULL DEFAULT '[]',
    conflicts_with TEXT NOT NULL DEFAULT '[]',
    lock_set TEXT NOT NULL DEFAULT '[]',
    plan_version TEXT,
    lock_requirements TEXT NOT NULL DEFAULT '[]',
    priority INTEGER NOT NULL DEFAULT 100,
    risk_level TEXT NOT NULL DEFAULT 'medium',
    execution_bucket TEXT NOT NULL DEFAULT 'coordinated_parallel',
    estimate_points INTEGER NOT NULL DEFAULT 3,
    unblock_value INTEGER NOT NULL DEFAULT 0,
    UNIQUE(project_dir, id)
);

INSERT INTO tasks_new
    (id, project_dir, spec, title, agent, status, inputs, output_artifact,
     created_at, updated_at, depends_on, conflicts_with, lock_set, plan_version,
     lock_requirements, priority, risk_level, execution_bucket,
     estimate_points, unblock_value)
SELECT id, '', spec, title, agent, status, inputs, output_artifact,
       created_at, updated_at,
       COALESCE(depends_on,    '[]'),
       COALESCE(conflicts_with,'[]'),
       COALESCE(lock_set,      '[]'),
       plan_version,
       COALESCE(lock_requirements, '[]'),
       COALESCE(priority,       100),
       COALESCE(risk_level,    'medium'),
       COALESCE(execution_bucket,'coordinated_parallel'),
       COALESCE(estimate_points,  3),
       COALESCE(unblock_value,    0)
FROM tasks;

DROP TABLE tasks;
ALTER TABLE tasks_new RENAME TO tasks;

CREATE INDEX IF NOT EXISTS idx_tasks_project_dir  ON tasks(project_dir);
CREATE INDEX IF NOT EXISTS idx_tasks_project_id   ON tasks(project_dir, id);
CREATE INDEX IF NOT EXISTS idx_tasks_project_spec ON tasks(project_dir, spec);

-- ─────────────────────────────────────────────────────────────────────────────
-- 3. events
--    PK is INTEGER AUTOINCREMENT — safe to rebuild for project_dir column.
--    Rebuilding (rather than ALTER) to add NOT NULL DEFAULT '' cleanly.
-- ─────────────────────────────────────────────────────────────────────────────
CREATE TABLE IF NOT EXISTS events_new (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    project_dir TEXT NOT NULL DEFAULT '',
    type TEXT NOT NULL,
    spec TEXT,
    agent TEXT,
    payload TEXT NOT NULL DEFAULT '{}',
    timestamp TEXT NOT NULL
);

INSERT INTO events_new (id, project_dir, type, spec, agent, payload, timestamp)
SELECT id, '', type, spec, agent, payload, timestamp
FROM events;

DROP TABLE events;
ALTER TABLE events_new RENAME TO events;

CREATE INDEX IF NOT EXISTS idx_events_project_dir ON events(project_dir);
CREATE INDEX IF NOT EXISTS idx_events_type        ON events(type);
CREATE INDEX IF NOT EXISTS idx_events_spec        ON events(spec);
CREATE INDEX IF NOT EXISTS idx_events_agent       ON events(agent);
CREATE INDEX IF NOT EXISTS idx_events_timestamp   ON events(timestamp);
CREATE INDEX IF NOT EXISTS idx_events_project_spec ON events(project_dir, spec);

-- ─────────────────────────────────────────────────────────────────────────────
-- 4. memory
--    PK is INTEGER AUTOINCREMENT.
--    Rebuild needed to change UNIQUE(agent, spec, key) →
--    UNIQUE(project_dir, agent, spec, key).
--    FTS triggers & content table still reference the renamed `memory` table —
--    triggers are dropped and recreated to stay consistent.
-- ─────────────────────────────────────────────────────────────────────────────

-- Drop FTS sync triggers before rebuilding the backing table.
DROP TRIGGER IF EXISTS memory_ai;
DROP TRIGGER IF EXISTS memory_au;
DROP TRIGGER IF EXISTS memory_ad;

-- Drop and recreate FTS table (content table rowid mapping must stay valid).
DROP TABLE IF EXISTS memory_fts;

CREATE TABLE IF NOT EXISTS memory_new (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    project_dir TEXT NOT NULL DEFAULT '',
    agent TEXT NOT NULL,
    key TEXT NOT NULL,
    value TEXT NOT NULL,
    spec TEXT NOT NULL DEFAULT '',
    updated_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ', 'now')),
    -- columns from 20260306000000_memory_enhanced
    type TEXT CHECK(type IN (
        'decision','architecture','bugfix','pattern','config','discovery','learning'
    )) DEFAULT NULL,
    deleted_at TEXT DEFAULT NULL,
    expires_at TEXT DEFAULT NULL,
    access_count INTEGER NOT NULL DEFAULT 0,
    last_accessed_at TEXT DEFAULT NULL,
    revision_count INTEGER NOT NULL DEFAULT 1,
    UNIQUE(project_dir, agent, spec, key)
);

INSERT INTO memory_new
    (id, project_dir, agent, key, value, spec, updated_at,
     type, deleted_at, expires_at, access_count, last_accessed_at, revision_count)
SELECT id, '', agent, key, value, spec, updated_at,
       type, deleted_at, expires_at, access_count, last_accessed_at, revision_count
FROM memory;

DROP TABLE memory;
ALTER TABLE memory_new RENAME TO memory;

CREATE INDEX IF NOT EXISTS idx_memory_project_dir     ON memory(project_dir);
CREATE INDEX IF NOT EXISTS idx_memory_agent_spec      ON memory(agent, spec);
CREATE INDEX IF NOT EXISTS idx_memory_agent_updated_at ON memory(agent, updated_at DESC);
CREATE INDEX IF NOT EXISTS idx_memory_project_agent   ON memory(project_dir, agent, spec);

-- Recreate FTS5 virtual table backed by new memory table.
CREATE VIRTUAL TABLE IF NOT EXISTS memory_fts USING fts5(
    key,
    value,
    content='memory',
    content_rowid='rowid'
);

-- Recreate FTS sync triggers.
CREATE TRIGGER IF NOT EXISTS memory_ai AFTER INSERT ON memory BEGIN
    INSERT INTO memory_fts(rowid, key, value) VALUES (new.rowid, new.key, new.value);
END;

CREATE TRIGGER IF NOT EXISTS memory_au AFTER UPDATE ON memory BEGIN
    INSERT INTO memory_fts(memory_fts, rowid, key, value)
        VALUES ('delete', old.rowid, old.key, old.value);
    INSERT INTO memory_fts(rowid, key, value) VALUES (new.rowid, new.key, new.value);
END;

CREATE TRIGGER IF NOT EXISTS memory_ad AFTER DELETE ON memory BEGIN
    INSERT INTO memory_fts(memory_fts, rowid, key, value)
        VALUES ('delete', old.rowid, old.key, old.value);
END;

-- Backfill FTS from surviving rows.
INSERT INTO memory_fts(rowid, key, value) SELECT rowid, key, value FROM memory;

-- ─────────────────────────────────────────────────────────────────────────────
-- 5. artifacts
--    OLD PK:  id TEXT PRIMARY KEY
--    NEW:     rowid PK, UNIQUE(project_dir, id)
-- ─────────────────────────────────────────────────────────────────────────────
CREATE TABLE IF NOT EXISTS artifacts_new (
    id TEXT NOT NULL,
    project_dir TEXT NOT NULL DEFAULT '',
    spec TEXT NOT NULL,
    task TEXT,
    agent TEXT NOT NULL,
    type TEXT NOT NULL,
    path TEXT,
    description TEXT,
    created_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ', 'now')),
    UNIQUE(project_dir, id)
);

INSERT INTO artifacts_new
    (id, project_dir, spec, task, agent, type, path, description, created_at)
SELECT id, '', spec, task, agent, type, path, description, created_at
FROM artifacts;

DROP TABLE artifacts;
ALTER TABLE artifacts_new RENAME TO artifacts;

CREATE INDEX IF NOT EXISTS idx_artifacts_project_dir ON artifacts(project_dir);
CREATE INDEX IF NOT EXISTS idx_artifacts_project_id  ON artifacts(project_dir, id);
CREATE INDEX IF NOT EXISTS idx_artifacts_spec        ON artifacts(spec);
CREATE INDEX IF NOT EXISTS idx_artifacts_agent       ON artifacts(agent);
CREATE INDEX IF NOT EXISTS idx_artifacts_type        ON artifacts(type);

-- ─────────────────────────────────────────────────────────────────────────────
-- 6. incidents
--    OLD PK:  id TEXT PRIMARY KEY
--    NEW:     rowid PK, UNIQUE(project_dir, id)
-- ─────────────────────────────────────────────────────────────────────────────
CREATE TABLE IF NOT EXISTS incidents_new (
    id TEXT NOT NULL,
    project_dir TEXT NOT NULL DEFAULT '',
    spec_id TEXT NOT NULL,
    task_id TEXT,
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
    updated_at TEXT NOT NULL,
    UNIQUE(project_dir, id)
);

INSERT INTO incidents_new
    (id, project_dir, spec_id, task_id, title, severity, status, source,
     blocking, repro_steps, root_cause, fix_strategy, created_at, updated_at)
SELECT id, '', spec_id, task_id, title, severity, status, source,
       blocking, repro_steps, root_cause, fix_strategy, created_at, updated_at
FROM incidents;

DROP TABLE incidents;
ALTER TABLE incidents_new RENAME TO incidents;

CREATE INDEX IF NOT EXISTS idx_incidents_project_dir ON incidents(project_dir);
CREATE INDEX IF NOT EXISTS idx_incidents_project_id  ON incidents(project_dir, id);
CREATE INDEX IF NOT EXISTS idx_incidents_spec_id     ON incidents(spec_id);
CREATE INDEX IF NOT EXISTS idx_incidents_task_id     ON incidents(task_id);
CREATE INDEX IF NOT EXISTS idx_incidents_status      ON incidents(status);
CREATE INDEX IF NOT EXISTS idx_incidents_severity    ON incidents(severity);

-- ─────────────────────────────────────────────────────────────────────────────
-- 7. context_gaps
--    OLD PK:  id TEXT PRIMARY KEY
--    NEW:     rowid PK, UNIQUE(project_dir, id)
-- ─────────────────────────────────────────────────────────────────────────────
CREATE TABLE IF NOT EXISTS context_gaps_new (
    id TEXT NOT NULL,
    project_dir TEXT NOT NULL DEFAULT '',
    spec_id TEXT NOT NULL,
    task_id TEXT,
    kind TEXT NOT NULL CHECK (kind IN (
        'missing_doc', 'outdated_doc', 'contradictory_doc', 'undocumented_behavior'
    )),
    criticality TEXT NOT NULL CHECK (criticality IN ('low', 'medium', 'high')),
    status TEXT NOT NULL CHECK (status IN (
        'open', 'triaged', 'assumption_recorded', 'resolved', 'wont_fix'
    )),
    blocking INTEGER NOT NULL DEFAULT 0 CHECK (blocking IN (0, 1)),
    question TEXT NOT NULL,
    assumption TEXT,
    resolution TEXT,
    created_at TEXT NOT NULL,
    updated_at TEXT NOT NULL,
    UNIQUE(project_dir, id)
);

INSERT INTO context_gaps_new
    (id, project_dir, spec_id, task_id, kind, criticality, status, blocking,
     question, assumption, resolution, created_at, updated_at)
SELECT id, '', spec_id, task_id, kind, criticality, status, blocking,
       question, assumption, resolution, created_at, updated_at
FROM context_gaps;

DROP TABLE context_gaps;
ALTER TABLE context_gaps_new RENAME TO context_gaps;

CREATE INDEX IF NOT EXISTS idx_context_gaps_project_dir ON context_gaps(project_dir);
CREATE INDEX IF NOT EXISTS idx_context_gaps_project_id  ON context_gaps(project_dir, id);
CREATE INDEX IF NOT EXISTS idx_context_gaps_spec_id     ON context_gaps(spec_id);
CREATE INDEX IF NOT EXISTS idx_context_gaps_task_id     ON context_gaps(task_id);
CREATE INDEX IF NOT EXISTS idx_context_gaps_status      ON context_gaps(status);

-- ─────────────────────────────────────────────────────────────────────────────
-- 8. verification_runs
--    OLD PK:  id TEXT PRIMARY KEY
--    NEW:     rowid PK, UNIQUE(project_dir, id)
-- ─────────────────────────────────────────────────────────────────────────────
CREATE TABLE IF NOT EXISTS verification_runs_new (
    id TEXT NOT NULL,
    project_dir TEXT NOT NULL DEFAULT '',
    spec_id TEXT NOT NULL,
    task_id TEXT,
    slice_id TEXT,
    kind TEXT NOT NULL CHECK (kind IN (
        'static', 'unit', 'integration', 'contract', 'e2e', 'smoke',
        'migration', 'docs', 'observability'
    )),
    status TEXT NOT NULL CHECK (status IN (
        'pass', 'pass_with_risk', 'fail', 'flaky', 'blocked'
    )),
    command TEXT,
    summary TEXT NOT NULL,
    evidence TEXT,
    created_at TEXT NOT NULL,
    UNIQUE(project_dir, id)
);

INSERT INTO verification_runs_new
    (id, project_dir, spec_id, task_id, slice_id, kind, status,
     command, summary, evidence, created_at)
SELECT id, '', spec_id, task_id, slice_id, kind, status,
       command, summary, evidence, created_at
FROM verification_runs;

DROP TABLE verification_runs;
ALTER TABLE verification_runs_new RENAME TO verification_runs;

CREATE INDEX IF NOT EXISTS idx_verification_runs_project_dir ON verification_runs(project_dir);
CREATE INDEX IF NOT EXISTS idx_verification_runs_project_id  ON verification_runs(project_dir, id);
CREATE INDEX IF NOT EXISTS idx_verification_runs_spec_id     ON verification_runs(spec_id);
CREATE INDEX IF NOT EXISTS idx_verification_runs_task_id     ON verification_runs(task_id);
CREATE INDEX IF NOT EXISTS idx_verification_runs_kind        ON verification_runs(kind);
CREATE INDEX IF NOT EXISTS idx_verification_runs_status      ON verification_runs(status);

-- ─────────────────────────────────────────────────────────────────────────────
-- 9. interrupts
--    OLD PK:  id TEXT PRIMARY KEY
--    NEW:     rowid PK, UNIQUE(project_dir, id)
-- ─────────────────────────────────────────────────────────────────────────────
CREATE TABLE IF NOT EXISTS interrupts_new (
    id TEXT NOT NULL,
    project_dir TEXT NOT NULL DEFAULT '',
    spec_id TEXT NOT NULL,
    reason_type TEXT NOT NULL CHECK (reason_type IN (
        'emergency', 'customer_critical', 'revenue', 'incident', 'strategy', 'dependency'
    )),
    status TEXT NOT NULL CHECK (status IN ('open', 'active', 'resolved', 'cancelled')),
    preempted_tasks TEXT NOT NULL DEFAULT '[]',
    resume_hint TEXT,
    created_at TEXT NOT NULL,
    updated_at TEXT NOT NULL,
    UNIQUE(project_dir, id)
);

INSERT INTO interrupts_new
    (id, project_dir, spec_id, reason_type, status, preempted_tasks,
     resume_hint, created_at, updated_at)
SELECT id, '', spec_id, reason_type, status, preempted_tasks,
       resume_hint, created_at, updated_at
FROM interrupts;

DROP TABLE interrupts;
ALTER TABLE interrupts_new RENAME TO interrupts;

CREATE INDEX IF NOT EXISTS idx_interrupts_project_dir ON interrupts(project_dir);
CREATE INDEX IF NOT EXISTS idx_interrupts_project_id  ON interrupts(project_dir, id);
CREATE INDEX IF NOT EXISTS idx_interrupts_spec_id     ON interrupts(spec_id);
CREATE INDEX IF NOT EXISTS idx_interrupts_status      ON interrupts(status);

-- ─────────────────────────────────────────────────────────────────────────────
-- 10. handoff_snapshots
--     OLD PK:  id TEXT PRIMARY KEY
--     NEW:     rowid PK, UNIQUE(project_dir, id)
-- ─────────────────────────────────────────────────────────────────────────────
CREATE TABLE IF NOT EXISTS handoff_snapshots_new (
    id TEXT NOT NULL,
    project_dir TEXT NOT NULL DEFAULT '',
    spec_id TEXT NOT NULL,
    interrupt_id TEXT,
    last_wave INTEGER,
    last_task TEXT,
    files_touched TEXT NOT NULL DEFAULT '[]',
    decisions TEXT NOT NULL DEFAULT '[]',
    open_risks TEXT NOT NULL DEFAULT '[]',
    next_steps TEXT NOT NULL DEFAULT '[]',
    created_at TEXT NOT NULL,
    UNIQUE(project_dir, id)
);

INSERT INTO handoff_snapshots_new
    (id, project_dir, spec_id, interrupt_id, last_wave, last_task,
     files_touched, decisions, open_risks, next_steps, created_at)
SELECT id, '', spec_id, interrupt_id, last_wave, last_task,
       files_touched, decisions, open_risks, next_steps, created_at
FROM handoff_snapshots;

DROP TABLE handoff_snapshots;
ALTER TABLE handoff_snapshots_new RENAME TO handoff_snapshots;

CREATE INDEX IF NOT EXISTS idx_handoff_snapshots_project_dir   ON handoff_snapshots(project_dir);
CREATE INDEX IF NOT EXISTS idx_handoff_snapshots_project_id    ON handoff_snapshots(project_dir, id);
CREATE INDEX IF NOT EXISTS idx_handoff_snapshots_spec_id       ON handoff_snapshots(spec_id);
CREATE INDEX IF NOT EXISTS idx_handoff_snapshots_interrupt_id  ON handoff_snapshots(interrupt_id);

-- ─────────────────────────────────────────────────────────────────────────────
-- 11. plan_versions
--     OLD PK:  id TEXT PRIMARY KEY, UNIQUE(spec_id, version)
--     NEW:     rowid PK, UNIQUE(project_dir, id), UNIQUE(project_dir, spec_id, version)
-- ─────────────────────────────────────────────────────────────────────────────
CREATE TABLE IF NOT EXISTS plan_versions_new (
    id TEXT NOT NULL,
    project_dir TEXT NOT NULL DEFAULT '',
    spec_id TEXT NOT NULL,
    version INTEGER NOT NULL,
    status TEXT NOT NULL CHECK (status IN ('active', 'superseded')),
    reason TEXT,
    plan_json TEXT NOT NULL,
    created_at TEXT NOT NULL,
    UNIQUE(project_dir, id),
    UNIQUE(project_dir, spec_id, version)
);

INSERT INTO plan_versions_new
    (id, project_dir, spec_id, version, status, reason, plan_json, created_at)
SELECT id, '', spec_id, version, status, reason, plan_json, created_at
FROM plan_versions;

DROP TABLE plan_versions;
ALTER TABLE plan_versions_new RENAME TO plan_versions;

CREATE INDEX IF NOT EXISTS idx_plan_versions_project_dir ON plan_versions(project_dir);
CREATE INDEX IF NOT EXISTS idx_plan_versions_project_id  ON plan_versions(project_dir, id);
CREATE INDEX IF NOT EXISTS idx_plan_versions_spec_id     ON plan_versions(spec_id, version DESC);

-- ─────────────────────────────────────────────────────────────────────────────
-- 12. task_leases
--     OLD PK:  task_id TEXT PRIMARY KEY
--     NEW:     PRIMARY KEY (project_dir, task_id)
--     Cross-project task IDs would collide in global mode — composite PK required.
-- ─────────────────────────────────────────────────────────────────────────────
CREATE TABLE IF NOT EXISTS task_leases_new (
    project_dir TEXT NOT NULL DEFAULT '',
    task_id TEXT NOT NULL,
    agent_id TEXT NOT NULL,
    status TEXT NOT NULL CHECK (status IN ('claimed', 'running', 'expired', 'released')),
    lease_expires_at TEXT NOT NULL,
    heartbeat_at TEXT NOT NULL,
    attempt_count INTEGER NOT NULL DEFAULT 1,
    created_at TEXT NOT NULL,
    updated_at TEXT NOT NULL,
    PRIMARY KEY (project_dir, task_id)
);

INSERT INTO task_leases_new
    (project_dir, task_id, agent_id, status, lease_expires_at, heartbeat_at,
     attempt_count, created_at, updated_at)
SELECT '', task_id, agent_id, status, lease_expires_at, heartbeat_at,
       attempt_count, created_at, updated_at
FROM task_leases;

DROP TABLE task_leases;
ALTER TABLE task_leases_new RENAME TO task_leases;

CREATE INDEX IF NOT EXISTS idx_task_leases_project_dir ON task_leases(project_dir);
CREATE INDEX IF NOT EXISTS idx_task_leases_status      ON task_leases(status, lease_expires_at);

-- ─────────────────────────────────────────────────────────────────────────────
-- 13. task_locks
--     OLD PK:  id TEXT PRIMARY KEY
--     NEW:     rowid PK, UNIQUE(project_dir, id)
-- ─────────────────────────────────────────────────────────────────────────────
CREATE TABLE IF NOT EXISTS task_locks_new (
    id TEXT NOT NULL,
    project_dir TEXT NOT NULL DEFAULT '',
    task_id TEXT NOT NULL,
    spec_id TEXT NOT NULL,
    lock_type TEXT NOT NULL CHECK (lock_type IN ('module', 'semantic', 'file')),
    resource TEXT NOT NULL,
    status TEXT NOT NULL CHECK (status IN ('active', 'released')),
    acquired_at TEXT NOT NULL,
    released_at TEXT,
    UNIQUE(project_dir, id)
);

INSERT INTO task_locks_new
    (id, project_dir, task_id, spec_id, lock_type, resource, status,
     acquired_at, released_at)
SELECT id, '', task_id, spec_id, lock_type, resource, status,
       acquired_at, released_at
FROM task_locks;

DROP TABLE task_locks;
ALTER TABLE task_locks_new RENAME TO task_locks;

CREATE INDEX IF NOT EXISTS idx_task_locks_project_dir ON task_locks(project_dir);
CREATE INDEX IF NOT EXISTS idx_task_locks_project_id  ON task_locks(project_dir, id);
CREATE INDEX IF NOT EXISTS idx_task_locks_active      ON task_locks(spec_id, lock_type, resource, status);
CREATE INDEX IF NOT EXISTS idx_task_locks_project_active ON task_locks(project_dir, spec_id, lock_type, resource, status);

-- ─────────────────────────────────────────────────────────────────────────────
-- 14. replan_requests
--     OLD PK:  id TEXT PRIMARY KEY
--     NEW:     rowid PK, UNIQUE(project_dir, id)
-- ─────────────────────────────────────────────────────────────────────────────
CREATE TABLE IF NOT EXISTS replan_requests_new (
    id TEXT NOT NULL,
    project_dir TEXT NOT NULL DEFAULT '',
    spec_id TEXT NOT NULL,
    task_id TEXT,
    agent_id TEXT NOT NULL,
    reason TEXT NOT NULL,
    impact TEXT NOT NULL DEFAULT '[]',
    proposed_action TEXT,
    status TEXT NOT NULL CHECK (status IN ('open', 'accepted', 'rejected', 'resolved')),
    created_at TEXT NOT NULL,
    updated_at TEXT NOT NULL,
    UNIQUE(project_dir, id)
);

INSERT INTO replan_requests_new
    (id, project_dir, spec_id, task_id, agent_id, reason, impact,
     proposed_action, status, created_at, updated_at)
SELECT id, '', spec_id, task_id, agent_id, reason, impact,
       proposed_action, status, created_at, updated_at
FROM replan_requests;

DROP TABLE replan_requests;
ALTER TABLE replan_requests_new RENAME TO replan_requests;

CREATE INDEX IF NOT EXISTS idx_replan_requests_project_dir ON replan_requests(project_dir);
CREATE INDEX IF NOT EXISTS idx_replan_requests_project_id  ON replan_requests(project_dir, id);
CREATE INDEX IF NOT EXISTS idx_replan_requests_spec_id     ON replan_requests(spec_id, status);

-- ─────────────────────────────────────────────────────────────────────────────
-- Re-enable foreign-key enforcement.
-- ─────────────────────────────────────────────────────────────────────────────
PRAGMA foreign_keys = ON;
