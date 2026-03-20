-- Performance indexes for memory queries that filter/sort on type,
-- last_accessed_at, expires_at and deleted_at.

-- Used by memory_list/memory_search when filtering by mem_type
CREATE INDEX IF NOT EXISTS idx_memory_agent_type ON memory(agent, type);

-- Used by memory_context ORDER BY last_accessed_at DESC
CREATE INDEX IF NOT EXISTS idx_memory_agent_last_accessed ON memory(agent, last_accessed_at DESC);

-- Used by memory_gc and every read query that filters expired rows
CREATE INDEX IF NOT EXISTS idx_memory_expires_at ON memory(expires_at);

-- Used by memory_gc and every read query that filters soft-deleted rows
CREATE INDEX IF NOT EXISTS idx_memory_deleted_at ON memory(deleted_at);
