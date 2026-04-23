-- Evals + scorecards foundation for SPEC-005.
-- Keep eval records append-only and favor stable soft references for provenance
-- while exposing optional direct scope columns/indexes for common query paths.

CREATE TABLE IF NOT EXISTS eval_runs (
    id            TEXT PRIMARY KEY,
    evaluator     TEXT NOT NULL,
    target_kind   TEXT NOT NULL CHECK (target_kind IN ('spec', 'task', 'artifact', 'scope')),
    target_ref    TEXT NOT NULL,
    spec          TEXT REFERENCES specs(id) ON DELETE SET NULL,
    task          TEXT REFERENCES tasks(id) ON DELETE SET NULL,
    artifact_id   TEXT REFERENCES artifacts(id) ON DELETE SET NULL,
    summary       TEXT,
    rationale     TEXT,
    outcome       TEXT NOT NULL CHECK (outcome IN ('pass', 'warn', 'fail', 'mixed', 'unknown')),
    overall_score REAL,
    source        TEXT NOT NULL DEFAULT 'recorded' CHECK (source IN ('recorded', 'cli', 'mcp')),
    metadata_json TEXT NOT NULL DEFAULT '{}' CHECK (json_valid(metadata_json)),
    created_at    TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ', 'now')),
    CHECK (length(trim(target_ref)) > 0)
);
CREATE INDEX IF NOT EXISTS idx_eval_runs_target_created_at ON eval_runs(target_kind, target_ref, created_at DESC);
CREATE INDEX IF NOT EXISTS idx_eval_runs_spec_created_at ON eval_runs(spec, created_at DESC);
CREATE INDEX IF NOT EXISTS idx_eval_runs_task_created_at ON eval_runs(task, created_at DESC);
CREATE INDEX IF NOT EXISTS idx_eval_runs_artifact_created_at ON eval_runs(artifact_id, created_at DESC);
CREATE INDEX IF NOT EXISTS idx_eval_runs_outcome_created_at ON eval_runs(outcome, created_at DESC);

CREATE TABLE IF NOT EXISTS eval_scorecard_dimensions (
    eval_run_id         TEXT NOT NULL REFERENCES eval_runs(id) ON DELETE CASCADE,
    dimension_name      TEXT NOT NULL,
    normalized_status   TEXT NOT NULL CHECK (normalized_status IN ('pass', 'warn', 'fail', 'not_applicable', 'unknown')),
    normalized_score    REAL,
    normalized_value    TEXT,
    rationale           TEXT,
    details_json        TEXT NOT NULL DEFAULT '{}' CHECK (json_valid(details_json)),
    created_at          TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ', 'now')),
    CHECK (length(trim(dimension_name)) > 0),
    PRIMARY KEY (eval_run_id, dimension_name)
);
CREATE INDEX IF NOT EXISTS idx_eval_scorecard_dimensions_name_status ON eval_scorecard_dimensions(dimension_name, normalized_status);
CREATE INDEX IF NOT EXISTS idx_eval_scorecard_dimensions_name_score ON eval_scorecard_dimensions(dimension_name, normalized_score);

CREATE TABLE IF NOT EXISTS eval_run_links (
    id          INTEGER PRIMARY KEY AUTOINCREMENT,
    eval_run_id TEXT NOT NULL REFERENCES eval_runs(id) ON DELETE CASCADE,
    link_kind   TEXT NOT NULL CHECK (link_kind IN (
        'evidence_bundle',
        'validation_run',
        'session',
        'event',
        'artifact',
        'approval',
        'spec',
        'task',
        'eval_run',
        'custom'
    )),
    link_ref    TEXT NOT NULL,
    relation    TEXT NOT NULL DEFAULT 'context' CHECK (relation IN ('subject', 'context', 'input', 'baseline', 'derived_from', 'evidence', 'result')),
    created_at  TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ', 'now')),
    CHECK (length(trim(link_ref)) > 0),
    UNIQUE(eval_run_id, link_kind, link_ref, relation)
);
CREATE INDEX IF NOT EXISTS idx_eval_run_links_eval_run ON eval_run_links(eval_run_id);
CREATE INDEX IF NOT EXISTS idx_eval_run_links_ref ON eval_run_links(link_kind, link_ref, relation);
