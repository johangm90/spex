-- Policy engine v1 schema for local-first governance.
-- Keep this additive so existing projects remain compatible while later
-- domain/model tasks layer policy resolution and evidence workflows on top.

CREATE TABLE IF NOT EXISTS policy_configs (
    id               TEXT PRIMARY KEY,
    scope_kind       TEXT NOT NULL CHECK (scope_kind IN ('project', 'spec', 'task')),
    scope_ref        TEXT NOT NULL,
    agent            TEXT NOT NULL DEFAULT '',
    enabled          INTEGER NOT NULL DEFAULT 1 CHECK (enabled IN (0, 1)),
    enforcement_mode TEXT NOT NULL DEFAULT 'enforced' CHECK (enforcement_mode IN ('advisory', 'enforced')),
    rules_json       TEXT NOT NULL DEFAULT '{}' CHECK (json_valid(rules_json)),
    rationale        TEXT,
    created_by       TEXT,
    updated_by       TEXT,
    created_at       TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ', 'now')),
    updated_at       TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ', 'now')),
    UNIQUE(scope_kind, scope_ref, agent)
);
CREATE INDEX IF NOT EXISTS idx_policy_configs_scope ON policy_configs(scope_kind, scope_ref);
CREATE INDEX IF NOT EXISTS idx_policy_configs_agent_scope ON policy_configs(agent, scope_kind, scope_ref);
CREATE INDEX IF NOT EXISTS idx_policy_configs_enabled_mode ON policy_configs(enabled, enforcement_mode);

CREATE TABLE IF NOT EXISTS evidence_bundles (
    id              TEXT PRIMARY KEY,
    entity_kind     TEXT NOT NULL CHECK (entity_kind IN ('task', 'spec')),
    entity_id       TEXT NOT NULL,
    spec            TEXT NOT NULL REFERENCES specs(id) ON DELETE CASCADE,
    task            TEXT REFERENCES tasks(id) ON DELETE CASCADE,
    status          TEXT NOT NULL DEFAULT 'draft' CHECK (status IN ('draft', 'submitted', 'accepted', 'rejected')),
    summary         TEXT,
    behavior_change INTEGER NOT NULL DEFAULT 0 CHECK (behavior_change IN (0, 1)),
    metadata_json   TEXT NOT NULL DEFAULT '{}' CHECK (json_valid(metadata_json)),
    created_by      TEXT,
    updated_by      TEXT,
    created_at      TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ', 'now')),
    updated_at      TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ', 'now')),
    CHECK (
        (entity_kind = 'task' AND task IS NOT NULL AND entity_id = task)
        OR (entity_kind = 'spec' AND task IS NULL AND entity_id = spec)
    ),
    UNIQUE(entity_kind, entity_id)
);
CREATE INDEX IF NOT EXISTS idx_evidence_bundles_spec_status ON evidence_bundles(spec, status);
CREATE INDEX IF NOT EXISTS idx_evidence_bundles_task_status ON evidence_bundles(task, status);
CREATE INDEX IF NOT EXISTS idx_evidence_bundles_updated_at ON evidence_bundles(updated_at DESC);

CREATE TABLE IF NOT EXISTS validation_runs (
    id                 TEXT PRIMARY KEY,
    evidence_bundle_id TEXT REFERENCES evidence_bundles(id) ON DELETE SET NULL,
    entity_kind        TEXT NOT NULL CHECK (entity_kind IN ('task', 'spec')),
    entity_id          TEXT NOT NULL,
    spec               TEXT NOT NULL REFERENCES specs(id) ON DELETE CASCADE,
    task               TEXT REFERENCES tasks(id) ON DELETE CASCADE,
    command_alias      TEXT NOT NULL CHECK (command_alias IN ('fast', 'primary', 'full', 'custom')),
    command            TEXT NOT NULL,
    source             TEXT NOT NULL DEFAULT 'recorded' CHECK (source IN ('recorded', 'cli', 'mcp')),
    exit_code          INTEGER,
    success            INTEGER NOT NULL CHECK (success IN (0, 1)),
    ran_at             TEXT NOT NULL,
    recorded_at        TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ', 'now')),
    recorded_by        TEXT,
    output_summary     TEXT,
    metadata_json      TEXT NOT NULL DEFAULT '{}' CHECK (json_valid(metadata_json)),
    CHECK (
        (entity_kind = 'task' AND task IS NOT NULL AND entity_id = task)
        OR (entity_kind = 'spec' AND task IS NULL AND entity_id = spec)
    )
);
CREATE INDEX IF NOT EXISTS idx_validation_runs_bundle ON validation_runs(evidence_bundle_id);
CREATE INDEX IF NOT EXISTS idx_validation_runs_entity ON validation_runs(entity_kind, entity_id, ran_at DESC);
CREATE INDEX IF NOT EXISTS idx_validation_runs_alias_success ON validation_runs(command_alias, success, ran_at DESC);
CREATE INDEX IF NOT EXISTS idx_validation_runs_spec_task ON validation_runs(spec, task);

CREATE TABLE IF NOT EXISTS evidence_bundle_artifacts (
    evidence_bundle_id TEXT NOT NULL REFERENCES evidence_bundles(id) ON DELETE CASCADE,
    artifact_id        TEXT NOT NULL REFERENCES artifacts(id) ON DELETE CASCADE,
    role               TEXT NOT NULL DEFAULT 'supporting' CHECK (role IN ('supporting', 'primary_output', 'test_evidence')),
    created_at         TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ', 'now')),
    PRIMARY KEY (evidence_bundle_id, artifact_id)
);
CREATE INDEX IF NOT EXISTS idx_evidence_bundle_artifacts_artifact ON evidence_bundle_artifacts(artifact_id);

CREATE TABLE IF NOT EXISTS evidence_bundle_validations (
    evidence_bundle_id TEXT NOT NULL REFERENCES evidence_bundles(id) ON DELETE CASCADE,
    validation_run_id  TEXT NOT NULL REFERENCES validation_runs(id) ON DELETE CASCADE,
    requirement_level  TEXT NOT NULL DEFAULT 'primary' CHECK (requirement_level IN ('fast', 'primary', 'full', 'custom')),
    created_at         TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ', 'now')),
    PRIMARY KEY (evidence_bundle_id, validation_run_id)
);
CREATE INDEX IF NOT EXISTS idx_evidence_bundle_validations_run ON evidence_bundle_validations(validation_run_id);

CREATE TABLE IF NOT EXISTS approvals (
    id                 TEXT PRIMARY KEY,
    entity_kind        TEXT NOT NULL CHECK (entity_kind IN ('task', 'spec', 'operation')),
    entity_id          TEXT NOT NULL,
    spec               TEXT REFERENCES specs(id) ON DELETE CASCADE,
    task               TEXT REFERENCES tasks(id) ON DELETE CASCADE,
    operation_kind     TEXT NOT NULL,
    status             TEXT NOT NULL DEFAULT 'pending' CHECK (status IN ('pending', 'approved', 'rejected', 'cancelled', 'expired')),
    policy_config_id   TEXT REFERENCES policy_configs(id) ON DELETE SET NULL,
    evidence_bundle_id TEXT REFERENCES evidence_bundles(id) ON DELETE SET NULL,
    requested_by       TEXT NOT NULL,
    decided_by         TEXT,
    decision_reason    TEXT,
    request_context_json TEXT NOT NULL DEFAULT '{}' CHECK (json_valid(request_context_json)),
    created_at         TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ', 'now')),
    decided_at         TEXT,
    expires_at         TEXT,
    CHECK (
        (entity_kind = 'task' AND task IS NOT NULL AND entity_id = task)
        OR (entity_kind = 'spec' AND spec IS NOT NULL AND task IS NULL AND entity_id = spec)
        OR (entity_kind = 'operation')
    )
);
CREATE INDEX IF NOT EXISTS idx_approvals_status_created_at ON approvals(status, created_at DESC);
CREATE INDEX IF NOT EXISTS idx_approvals_spec_status ON approvals(spec, status, created_at DESC);
CREATE INDEX IF NOT EXISTS idx_approvals_task_status ON approvals(task, status, created_at DESC);
CREATE INDEX IF NOT EXISTS idx_approvals_policy_config ON approvals(policy_config_id);
CREATE UNIQUE INDEX IF NOT EXISTS idx_approvals_pending_operation
    ON approvals(entity_kind, entity_id, operation_kind)
    WHERE status = 'pending';

CREATE TABLE IF NOT EXISTS policy_audit_refs (
    id         INTEGER PRIMARY KEY AUTOINCREMENT,
    event_id   INTEGER NOT NULL REFERENCES events(id) ON DELETE CASCADE,
    ref_kind   TEXT NOT NULL CHECK (ref_kind IN ('policy_config', 'evidence_bundle', 'validation_run', 'approval', 'artifact', 'spec', 'task')),
    ref_id     TEXT NOT NULL,
    relation   TEXT NOT NULL DEFAULT 'subject' CHECK (relation IN ('subject', 'input', 'result', 'blocking', 'derived_from')),
    created_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ', 'now')),
    UNIQUE(event_id, ref_kind, ref_id, relation)
);
CREATE INDEX IF NOT EXISTS idx_policy_audit_refs_event ON policy_audit_refs(event_id);
CREATE INDEX IF NOT EXISTS idx_policy_audit_refs_ref ON policy_audit_refs(ref_kind, ref_id, relation);
