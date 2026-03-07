-- Task optimization hints for scheduler
ALTER TABLE tasks ADD COLUMN estimate_points INTEGER NOT NULL DEFAULT 3;
ALTER TABLE tasks ADD COLUMN unblock_value INTEGER NOT NULL DEFAULT 0;
