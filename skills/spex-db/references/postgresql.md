# PostgreSQL Deep Reference — spex-db

Advanced PostgreSQL patterns: index types, Row Level Security, table partitioning,
full-text search, EXPLAIN ANALYZE, triggers, connection pooling, and JSONB.

---

## 1. Index Types

### B-tree (default — equality and range queries)
```sql
CREATE INDEX idx_orders_customer_id ON orders(customer_id);
CREATE INDEX idx_orders_created_at  ON orders(created_at DESC);
```

### GIN — JSONB containment, arrays, full-text search
```sql
-- JSONB: enables @>, ?, ?|, ?& operators
CREATE INDEX idx_orders_metadata ON orders USING GIN (metadata);

-- Array containment
CREATE INDEX idx_products_tags ON products USING GIN (tags);

-- Full-text search on a tsvector column
CREATE INDEX idx_products_fts ON products USING GIN (search_vector);
```

### GiST — geometric types, ranges, nearest-neighbour
```sql
-- Date/time range overlap queries
CREATE INDEX idx_bookings_period ON bookings USING GiST (period);

-- IP network containment
CREATE INDEX idx_acl_network ON acl_entries USING GiST (network inet_ops);
```

### BRIN — very large, naturally ordered tables (append-only logs, time-series)
```sql
-- Tiny index footprint; works when physical order correlates with column value
CREATE INDEX idx_events_created_at_brin ON events USING BRIN (created_at);
```

### Hash — equality only, slightly faster than B-tree for pure `=`
```sql
CREATE INDEX idx_sessions_token ON sessions USING HASH (token);
-- Note: hash indexes are not WAL-logged before PostgreSQL 10; avoid on < 10
```

### Partial index
```sql
-- Index only active records — smaller, faster for the hot query path
CREATE INDEX idx_orders_status_active
  ON orders(tenant_id, created_at DESC)
  WHERE deleted_at IS NULL AND status != 'cancelled';
```

### Covering index (INCLUDE clause — PostgreSQL 11+)
```sql
-- Query: SELECT id, status, total_cents FROM orders WHERE tenant_id = ? AND status = 'pending'
-- Columns in INCLUDE are stored but not part of the search key
CREATE INDEX idx_orders_tenant_status_covering
  ON orders(tenant_id, status)
  INCLUDE (id, total_cents);
```

### Expression index
```sql
-- Case-insensitive email uniqueness
CREATE UNIQUE INDEX uq_users_email_lower ON users (lower(email));

-- Computed expression
CREATE INDEX idx_orders_year ON orders (EXTRACT(YEAR FROM created_at));
```

### Concurrent index creation (zero downtime on existing tables)
```sql
-- Must run outside a transaction block (not inside BEGIN/COMMIT)
CREATE INDEX CONCURRENTLY idx_orders_customer_id ON orders(customer_id);
```

---

## 2. JSONB Patterns

### Column definition
```sql
ALTER TABLE orders ADD COLUMN metadata JSONB NOT NULL DEFAULT '{}';
```

### Query operators
```sql
-- Containment: does metadata contain this subset?
SELECT * FROM orders WHERE metadata @> '{"source": "api", "channel": "web"}';

-- Key existence
SELECT * FROM orders WHERE metadata ? 'promo_code';

-- Path extraction (returns JSONB)
SELECT metadata -> 'address' -> 'city' FROM orders;

-- Path extraction (returns text)
SELECT metadata ->> 'source' FROM orders WHERE id = 1;

-- Path exists
SELECT * FROM orders WHERE metadata #> '{address,city}' IS NOT NULL;
```

### GIN index variants
```sql
-- Default GIN — supports @>, ?, ?|, ?& (most useful)
CREATE INDEX idx_orders_metadata ON orders USING GIN (metadata);

-- jsonb_path_ops — supports @> only, but smaller and faster for containment
CREATE INDEX idx_orders_metadata_path ON orders USING GIN (metadata jsonb_path_ops);
```

### Update patterns
```sql
-- Set a key
UPDATE orders SET metadata = metadata || '{"reviewed": true}' WHERE id = 1;

-- Remove a key
UPDATE orders SET metadata = metadata - 'promo_code' WHERE id = 1;

-- Set a nested key
UPDATE orders SET metadata = jsonb_set(metadata, '{address,city}', '"London"') WHERE id = 1;
```

---

## 3. Full-Text Search

### `tsvector` column + trigger
```sql
-- Add the column
ALTER TABLE products ADD COLUMN search_vector TSVECTOR;

-- Populate on INSERT/UPDATE via trigger
CREATE OR REPLACE FUNCTION products_search_vector_update()
RETURNS TRIGGER LANGUAGE plpgsql AS $$
BEGIN
  NEW.search_vector :=
    setweight(to_tsvector('english', coalesce(NEW.name, '')),       'A') ||
    setweight(to_tsvector('english', coalesce(NEW.description, '')), 'B') ||
    setweight(to_tsvector('english', coalesce(NEW.tags::text, '')), 'C');
  RETURN NEW;
END;
$$;

CREATE TRIGGER trg_products_search_vector
  BEFORE INSERT OR UPDATE OF name, description, tags ON products
  FOR EACH ROW EXECUTE FUNCTION products_search_vector_update();

-- GIN index
CREATE INDEX idx_products_fts ON products USING GIN (search_vector);
```

### Querying
```sql
-- Basic search
SELECT id, name, ts_rank(search_vector, query) AS rank
FROM products, to_tsquery('english', 'laptop & keyboard') query
WHERE search_vector @@ query
ORDER BY rank DESC
LIMIT 20;

-- Phrase search
SELECT * FROM products
WHERE search_vector @@ phraseto_tsquery('english', 'mechanical keyboard');

-- Prefix search (autocomplete)
SELECT * FROM products
WHERE search_vector @@ to_tsquery('english', 'mech:*');
```

### Trigram similarity (pg_trgm — fuzzy match / LIKE acceleration)
```sql
CREATE EXTENSION IF NOT EXISTS pg_trgm;

-- Accelerate LIKE/ILIKE queries
CREATE INDEX idx_products_name_trgm ON products USING GIN (name gin_trgm_ops);

-- Fuzzy similarity search
SELECT name, similarity(name, 'keybord') AS sim
FROM products
WHERE name % 'keybord'        -- % operator: similarity > pg_trgm.similarity_threshold
ORDER BY sim DESC
LIMIT 10;
```

---

## 4. Row Level Security (RLS)

```sql
-- Enable RLS on the table
ALTER TABLE orders ENABLE ROW LEVEL SECURITY;
ALTER TABLE orders FORCE ROW LEVEL SECURITY;  -- applies to table owner too

-- Policy: tenant can only see their own rows
CREATE POLICY orders_tenant_isolation ON orders
  AS PERMISSIVE
  FOR ALL
  TO application_role          -- PostgreSQL role used by the app
  USING (tenant_id = current_setting('app.tenant_id')::BIGINT);

-- Separate insert policy (WITH CHECK)
CREATE POLICY orders_tenant_insert ON orders
  AS PERMISSIVE
  FOR INSERT
  TO application_role
  WITH CHECK (tenant_id = current_setting('app.tenant_id')::BIGINT);
```

### Setting the tenant context in the application
```sql
-- Set per transaction (preferred — reset on transaction end)
BEGIN;
SET LOCAL app.tenant_id = '42';
SELECT * FROM orders;  -- automatically filtered
COMMIT;
```

### Bypassing RLS (superuser / service account)
```sql
-- Create a role that bypasses RLS for admin/migration use
CREATE ROLE migration_role BYPASSRLS;
```

---

## 5. Table Partitioning

### Range partitioning (time-series, archive by month)
```sql
CREATE TABLE events (
  id          BIGINT GENERATED ALWAYS AS IDENTITY,
  tenant_id   BIGINT NOT NULL,
  event_type  TEXT NOT NULL,
  payload     JSONB NOT NULL DEFAULT '{}',
  created_at  TIMESTAMPTZ NOT NULL DEFAULT NOW()
) PARTITION BY RANGE (created_at);

-- Create monthly partitions
CREATE TABLE events_2026_01 PARTITION OF events
  FOR VALUES FROM ('2026-01-01') TO ('2026-02-01');

CREATE TABLE events_2026_02 PARTITION OF events
  FOR VALUES FROM ('2026-02-01') TO ('2026-03-01');

-- Default partition catches anything outside defined ranges
CREATE TABLE events_default PARTITION OF events DEFAULT;

-- Indexes are per-partition; attach a global index to the parent
CREATE INDEX idx_events_tenant_created ON events(tenant_id, created_at DESC);
```

### Hash partitioning (distribute load evenly)
```sql
CREATE TABLE order_items (
  id        BIGINT GENERATED ALWAYS AS IDENTITY,
  order_id  BIGINT NOT NULL,
  -- ...
) PARTITION BY HASH (order_id);

CREATE TABLE order_items_p0 PARTITION OF order_items FOR VALUES WITH (MODULUS 4, REMAINDER 0);
CREATE TABLE order_items_p1 PARTITION OF order_items FOR VALUES WITH (MODULUS 4, REMAINDER 1);
CREATE TABLE order_items_p2 PARTITION OF order_items FOR VALUES WITH (MODULUS 4, REMAINDER 2);
CREATE TABLE order_items_p3 PARTITION OF order_items FOR VALUES WITH (MODULUS 4, REMAINDER 3);
```

### List partitioning (by region or status)
```sql
CREATE TABLE accounts (
  id      BIGINT GENERATED ALWAYS AS IDENTITY,
  region  TEXT NOT NULL,
  -- ...
) PARTITION BY LIST (region);

CREATE TABLE accounts_eu PARTITION OF accounts FOR VALUES IN ('eu-west', 'eu-central');
CREATE TABLE accounts_us PARTITION OF accounts FOR VALUES IN ('us-east', 'us-west');
```

---

## 6. EXPLAIN ANALYZE — Reading Query Plans

```sql
EXPLAIN (ANALYZE, BUFFERS, FORMAT TEXT) SELECT ...;
```

### Key output fields

| Field | What it means |
|-------|--------------|
| `Seq Scan` | Full table scan — likely missing index |
| `Index Scan` | Uses index but fetches heap rows |
| `Index Only Scan` | Uses covering index — fastest; no heap access |
| `Bitmap Heap Scan` | Combines multiple index scans; good for low-selectivity queries |
| `Nested Loop` | Efficient for small inner sets; bad for large cross joins |
| `Hash Join` | Efficient for large equi-joins |
| `Merge Join` | Efficient when both sides are sorted |
| `actual rows=` vs `rows=` | Large discrepancy → stale statistics; run `ANALYZE <table>` |
| `Buffers: shared hit=` | Pages served from cache (good) |
| `Buffers: shared read=` | Pages read from disk (I/O cost) |

### Common problems and fixes

| Symptom | Likely cause | Fix |
|---------|-------------|-----|
| `Seq Scan` on large table | Missing index | Add index on the filtered/sorted column |
| `Index Scan` when `Index Only Scan` expected | SELECT includes non-indexed columns | Add `INCLUDE` columns to the index |
| Planner uses wrong index | Stale statistics | `ANALYZE table_name;` |
| Very slow `UPDATE`/`DELETE` | Unindexed FK in child table | Add FK index |
| `rows=` estimate wildly off | Low stats target | `ALTER TABLE t ALTER COLUMN c SET STATISTICS 500; ANALYZE t;` |

---

## 7. Statistics and Maintenance

```sql
-- Force statistics refresh on a table
ANALYZE orders;

-- View table bloat and dead tuples
SELECT relname, n_live_tup, n_dead_tup,
       round(n_dead_tup::numeric / nullif(n_live_tup + n_dead_tup, 0) * 100, 2) AS dead_pct
FROM pg_stat_user_tables
ORDER BY n_dead_tup DESC;

-- View index usage (unused indexes are write overhead)
SELECT indexrelname, idx_scan, idx_tup_read, idx_tup_fetch
FROM pg_stat_user_indexes
ORDER BY idx_scan ASC;

-- Check table sizes
SELECT relname AS table,
       pg_size_pretty(pg_total_relation_size(relid)) AS total_size,
       pg_size_pretty(pg_relation_size(relid)) AS table_size,
       pg_size_pretty(pg_total_relation_size(relid) - pg_relation_size(relid)) AS index_size
FROM pg_catalog.pg_statio_user_tables
ORDER BY pg_total_relation_size(relid) DESC;
```

---

## 8. Connection Pooling — PgBouncer

| Mode | Description | Use case |
|------|-------------|----------|
| **Transaction** | Connection returned to pool after each transaction | Most web apps (best throughput) |
| **Session** | Connection held for entire client session | Apps using session-level features (`SET`, temp tables) |
| **Statement** | Connection returned after each statement | Rare — breaks multi-statement transactions |

**RLS with PgBouncer transaction mode:** `SET LOCAL` (scoped to transaction) works correctly. `SET` (session-level) is reset when the connection is returned — do not use session-level `SET` for tenant context.

---

## 9. Useful Extensions

| Extension | Purpose | Install |
|-----------|---------|---------|
| `pg_trgm` | Trigram fuzzy search, ILIKE acceleration | `CREATE EXTENSION pg_trgm;` |
| `uuid-ossp` | `uuid_generate_v4()` (prefer `gen_random_uuid()` built-in on PG 13+) | `CREATE EXTENSION "uuid-ossp";` |
| `pgcrypto` | `crypt()`, `gen_salt()`, `digest()` | `CREATE EXTENSION pgcrypto;` |
| `pg_stat_statements` | Track slow query statistics | Requires `shared_preload_libraries` restart |
| `btree_gist` | GiST support for B-tree types (needed for exclusion constraints) | `CREATE EXTENSION btree_gist;` |
| `hstore` | Key-value store (prefer JSONB for new work) | `CREATE EXTENSION hstore;` |
