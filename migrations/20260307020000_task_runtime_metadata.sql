-- Task metadata for dependency-aware scheduling
ALTER TABLE tasks ADD COLUMN depends_on TEXT NOT NULL DEFAULT '[]';
ALTER TABLE tasks ADD COLUMN conflicts_with TEXT NOT NULL DEFAULT '[]';
ALTER TABLE tasks ADD COLUMN lock_set TEXT NOT NULL DEFAULT '[]';
ALTER TABLE tasks ADD COLUMN plan_version TEXT;
