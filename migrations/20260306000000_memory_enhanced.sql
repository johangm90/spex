-- Migration: Enhanced Agent Memory System (MEMS-001, Wave 1)
-- Adds typed memories, soft-delete, TTL, access tracking, and FTS5 full-text search.

-- ─── New columns on memory table ────────────────────────────────────────────
-- All nullable / with defaults so existing rows are unaffected.

ALTER TABLE memory ADD COLUMN type TEXT
  CHECK(type IN ('decision','architecture','bugfix','pattern','config','discovery','learning'))
  DEFAULT NULL;

ALTER TABLE memory ADD COLUMN deleted_at TEXT DEFAULT NULL;

ALTER TABLE memory ADD COLUMN expires_at TEXT DEFAULT NULL;

ALTER TABLE memory ADD COLUMN access_count INTEGER NOT NULL DEFAULT 0;

ALTER TABLE memory ADD COLUMN last_accessed_at TEXT DEFAULT NULL;

ALTER TABLE memory ADD COLUMN revision_count INTEGER NOT NULL DEFAULT 1;

-- ─── FTS5 virtual table ──────────────────────────────────────────────────────
-- Content-based FTS backed by the memory table.
-- Searches key and value columns; rowid links back to memory.id.
CREATE VIRTUAL TABLE IF NOT EXISTS memory_fts USING fts5(
  key,
  value,
  content='memory',
  content_rowid='rowid'
);

-- ─── Triggers: keep FTS5 in sync with memory ────────────────────────────────

-- After INSERT: add new row to FTS index
CREATE TRIGGER IF NOT EXISTS memory_ai AFTER INSERT ON memory BEGIN
  INSERT INTO memory_fts(rowid, key, value) VALUES (new.rowid, new.key, new.value);
END;

-- After UPDATE: replace old FTS entry with new one
CREATE TRIGGER IF NOT EXISTS memory_au AFTER UPDATE ON memory BEGIN
  INSERT INTO memory_fts(memory_fts, rowid, key, value) VALUES ('delete', old.rowid, old.key, old.value);
  INSERT INTO memory_fts(rowid, key, value) VALUES (new.rowid, new.key, new.value);
END;

-- After DELETE: remove row from FTS index
CREATE TRIGGER IF NOT EXISTS memory_ad AFTER DELETE ON memory BEGIN
  INSERT INTO memory_fts(memory_fts, rowid, key, value) VALUES ('delete', old.rowid, old.key, old.value);
END;

-- ─── Backfill FTS with existing data ────────────────────────────────────────
-- Populates the FTS index from all rows already present in memory.
INSERT INTO memory_fts(rowid, key, value) SELECT rowid, key, value FROM memory;
