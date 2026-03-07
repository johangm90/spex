-- Structured lock declarations and scheduling attributes
ALTER TABLE tasks ADD COLUMN lock_requirements TEXT NOT NULL DEFAULT '[]';
ALTER TABLE tasks ADD COLUMN priority INTEGER NOT NULL DEFAULT 100;
ALTER TABLE tasks ADD COLUMN risk_level TEXT NOT NULL DEFAULT 'medium';
ALTER TABLE tasks ADD COLUMN execution_bucket TEXT NOT NULL DEFAULT 'coordinated_parallel';
