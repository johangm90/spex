-- Allow artifacts to be registered without a parent spec.
-- Use case: skill-builder registers agents as artifacts (type='agent')
-- that are global and not tied to any specific spec.
--
-- SQLite does not support ALTER COLUMN, so we recreate the table.
CREATE TABLE IF NOT EXISTS artifacts_new (
    id          TEXT PRIMARY KEY,
    spec        TEXT,                -- nullable: NULL for global/cross-spec artifacts
    task        TEXT,
    agent       TEXT NOT NULL,
    type        TEXT NOT NULL,
    path        TEXT,
    description TEXT,
    created_at  TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ', 'now'))
);

INSERT INTO artifacts_new SELECT * FROM artifacts;
DROP TABLE artifacts;
ALTER TABLE artifacts_new RENAME TO artifacts;

CREATE INDEX IF NOT EXISTS idx_artifacts_spec  ON artifacts(spec);
CREATE INDEX IF NOT EXISTS idx_artifacts_agent ON artifacts(agent);
CREATE INDEX IF NOT EXISTS idx_artifacts_type  ON artifacts(type);
