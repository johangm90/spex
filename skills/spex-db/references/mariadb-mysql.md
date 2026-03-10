# MariaDB / MySQL Reference — spex-db

Deep patterns for MariaDB 10.6+ and MySQL 8.0+: data types, InnoDB specifics, index types,
online DDL, full-text search, JSON support, Doctrine configuration, and key differences
from PostgreSQL.

> **Project default:** When the project uses Symfony + Doctrine, MariaDB is the preferred
> engine. Use this reference alongside `references/orm-patterns.md §Doctrine`.

---

## 1. MariaDB vs MySQL — Key Differences

| Feature | MariaDB 10.6+ | MySQL 8.0+ |
|---------|--------------|------------|
| JSON type | Alias for `LONGTEXT` — stored as text, not binary | Native binary JSON — faster path operations |
| Sequences | Native `CREATE SEQUENCE` | No native sequence (use `AUTO_INCREMENT`) |
| Window functions | Yes (10.2+) | Yes (8.0+) |
| CTEs | Yes (10.2+) | Yes (8.0+) |
| System-versioned tables | Yes (`WITH SYSTEM VERSIONING`) | No |
| `RETURNING` clause | Yes (10.5+) | No |
| Default auth plugin | `mysql_native_password` (10.x) / `ed25519` (11+) | `caching_sha2_password` |
| UUID v4 function | `UUID()` | `UUID()` (same) |
| `INVISIBLE` columns | Yes (10.3+) | Yes (8.0.23+) |

**Rule:** When targeting both MariaDB and MySQL, avoid MariaDB-only features (`SEQUENCE`, `RETURNING`, system-versioned tables) unless the project is MariaDB-only.

---

## 2. Storage Engine — Always Use InnoDB

| Engine | Use | Notes |
|--------|-----|-------|
| **InnoDB** | All tables | ACID, row-level locking, FK support, MVCC |
| Aria | MariaDB internal system tables | Not for application tables |
| MyISAM | Legacy only | No transactions, no FK — do not use |
| MEMORY | Temporary / session tables | No persistence |

```sql
-- Explicit InnoDB (usually the default; be explicit in CREATE TABLE)
CREATE TABLE orders (
  ...
) ENGINE=InnoDB DEFAULT CHARSET=utf8mb4 COLLATE=utf8mb4_unicode_ci;
```

---

## 3. Character Set and Collation

**Always use `utf8mb4` — never `utf8`** (MySQL's `utf8` is broken: 3-byte only, cannot store emoji or some CJK characters).

```sql
-- Server-level default (my.cnf / mariadb.cnf)
[mysqld]
character-set-server = utf8mb4
collation-server     = utf8mb4_unicode_ci

-- Database-level
CREATE DATABASE myapp
  CHARACTER SET utf8mb4
  COLLATE utf8mb4_unicode_ci;

-- Table-level (explicit — portable across DB dumps)
CREATE TABLE products (
  name VARCHAR(255) NOT NULL
) ENGINE=InnoDB
  DEFAULT CHARSET=utf8mb4
  COLLATE=utf8mb4_unicode_ci;

-- Column-level override (e.g. case-sensitive comparison)
code VARCHAR(20) NOT NULL COLLATE utf8mb4_bin
```

### Collation choice

| Collation | Behaviour | Use when |
|-----------|----------|---------|
| `utf8mb4_unicode_ci` | Case-insensitive, accent-insensitive | General text fields (names, descriptions) |
| `utf8mb4_bin` | Binary — byte-exact, case-sensitive | Tokens, codes, hashes |
| `utf8mb4_unicode_520_ci` | Updated Unicode rules | When upgraded Unicode support matters |
| `utf8mb4_0900_ai_ci` | MySQL 8 only (faster) | MySQL 8+ only projects |

---

## 4. Data Types

| Use case | Correct type | Notes |
|----------|-------------|-------|
| Primary key | `BIGINT UNSIGNED NOT NULL AUTO_INCREMENT` | Avoid `INT` overflow at ~4 billion |
| Foreign key | `BIGINT UNSIGNED NOT NULL` | Must match PK type exactly |
| Money | `DECIMAL(19,4)` or `BIGINT` cents | Never `FLOAT` or `DOUBLE` |
| Timestamps | `DATETIME(6)` stored in UTC | MariaDB has no native `TIMESTAMPTZ`; enforce UTC at app layer |
| Boolean | `TINYINT(1)` or `BIT(1)` | No native `BOOLEAN`; `TINYINT(1)` is conventional |
| UUID | `CHAR(36)` or `BINARY(16)` | `BINARY(16)` is more compact and faster to index |
| Short enum-like | `ENUM('a','b','c')` or `VARCHAR` + CHECK | `ENUM` is hard to migrate — prefer `VARCHAR` + CHECK |
| Large text | `TEXT` or `LONGTEXT` | `TEXT` = 64 KB, `MEDIUMTEXT` = 16 MB, `LONGTEXT` = 4 GB |
| JSON | `JSON` (MariaDB 10.2+ alias for LONGTEXT) | Use JSON functions; no binary storage in MariaDB |

### `DATETIME(6)` UTC pattern
```sql
-- Store all timestamps in UTC; the application converts to local timezone
created_at DATETIME(6) NOT NULL DEFAULT CURRENT_TIMESTAMP(6),
updated_at DATETIME(6) NOT NULL DEFAULT CURRENT_TIMESTAMP(6) ON UPDATE CURRENT_TIMESTAMP(6),
deleted_at DATETIME(6) NULL
```

### UUID as `BINARY(16)` (optimal for indexing)
```sql
id BINARY(16) NOT NULL DEFAULT (UUID_TO_BIN(UUID(), TRUE)) PRIMARY KEY
-- TRUE = swap time bytes for better locality (reduces B-tree fragmentation)

-- Insert
INSERT INTO orders (id, ...) VALUES (UUID_TO_BIN(UUID(), TRUE), ...);

-- Query
SELECT BIN_TO_UUID(id, TRUE) AS id, ... FROM orders WHERE id = UUID_TO_BIN('...', TRUE);
```

---

## 5. Index Types (InnoDB)

### B-tree (default — all standard queries)
```sql
CREATE INDEX idx_orders_customer_id ON orders (customer_id);
CREATE INDEX idx_orders_tenant_status ON orders (tenant_id, status);
```

### FULLTEXT index (full-text search)
```sql
-- Single or multi-column FULLTEXT
ALTER TABLE products ADD FULLTEXT INDEX ft_products_name_desc (name, description);

-- Query with MATCH...AGAINST (natural language mode — default)
SELECT id, name,
       MATCH(name, description) AGAINST ('mechanical keyboard' IN NATURAL LANGUAGE MODE) AS score
FROM products
WHERE MATCH(name, description) AGAINST ('mechanical keyboard' IN NATURAL LANGUAGE MODE)
ORDER BY score DESC
LIMIT 20;

-- Boolean mode (supports +, -, *, "", etc.)
SELECT * FROM products
WHERE MATCH(name, description) AGAINST ('+mechanical +keyboard -membrane' IN BOOLEAN MODE);

-- Minimum word length: default 4 chars (ft_min_word_len); configure in my.cnf
-- [mysqld]
-- ft_min_word_len = 3
-- innodb_ft_min_token_size = 3
```

### Spatial index (geometry columns)
```sql
ALTER TABLE locations ADD COLUMN coords POINT NOT NULL SRID 4326;
ALTER TABLE locations ADD SPATIAL INDEX idx_locations_coords (coords);
```

### Prefix index (long columns)
```sql
-- Index only the first N characters of a TEXT/VARCHAR column
CREATE INDEX idx_products_description_prefix ON products (description(100));
```

### Covering index
```sql
-- InnoDB covering index: all SELECT columns are in the index
-- The PK is always implicitly included in secondary indexes
CREATE INDEX idx_orders_tenant_status_total
  ON orders (tenant_id, status, total_cents);
-- Query: SELECT total_cents FROM orders WHERE tenant_id=? AND status=?
-- → index-only scan (no heap lookup)
```

---

## 6. Online DDL (InnoDB)

Most `ALTER TABLE` operations in MariaDB/MySQL InnoDB can run online (no full table lock):

```sql
-- Add column — online (INPLACE), no rebuild needed in MariaDB 10.3+
ALTER TABLE orders
  ADD COLUMN notes TEXT NULL,
  ALGORITHM=INPLACE, LOCK=NONE;

-- Add index — online
ALTER TABLE orders
  ADD INDEX idx_orders_notes (notes(100)),
  ALGORITHM=INPLACE, LOCK=NONE;

-- Change column default — instant (no copy)
ALTER TABLE orders
  ALTER COLUMN status SET DEFAULT 'pending',
  ALGORITHM=INSTANT;

-- Rename column — online (MariaDB 10.5.2+)
ALTER TABLE orders
  RENAME COLUMN order_number TO order_ref,
  ALGORITHM=INPLACE, LOCK=NONE;
```

### DDL operation cost guide

| Operation | Algorithm | Lock | Notes |
|-----------|-----------|------|-------|
| Add nullable column | INSTANT | NONE | Cheapest — metadata only |
| Add NOT NULL column with default (MariaDB 10.3+) | INSTANT | NONE | |
| Add column without default (old) | COPY | SHARED | Full table rebuild |
| Add index | INPLACE | NONE | Concurrent reads/writes |
| Drop index | INPLACE | NONE | |
| Add FK | INPLACE | SHARED | Validates existing rows |
| Change column type | COPY | SHARED | Full table rebuild |
| Rename table | INPLACE | NONE | |

**Rule:** Always specify `ALGORITHM` and `LOCK` explicitly in migration scripts for large tables.
If `LOCK=NONE` is rejected, the operation will fail rather than silently hold a lock.

---

## 7. JSON in MariaDB

MariaDB stores JSON as `LONGTEXT` internally but provides JSON functions:

```sql
ALTER TABLE orders ADD COLUMN metadata JSON;

-- Read a value
SELECT JSON_VALUE(metadata, '$.source') FROM orders;
SELECT JSON_EXTRACT(metadata, '$.address.city') FROM orders;

-- Check existence
SELECT * FROM orders WHERE JSON_CONTAINS_PATH(metadata, 'one', '$.promo_code');

-- Containment (MariaDB 10.9+)
SELECT * FROM orders WHERE JSON_CONTAINS(metadata, '"api"', '$.source');

-- Update a key
UPDATE orders SET metadata = JSON_SET(metadata, '$.reviewed', TRUE) WHERE id = 1;

-- Remove a key
UPDATE orders SET metadata = JSON_REMOVE(metadata, '$.promo_code') WHERE id = 1;
```

### Indexing JSON (virtual generated column)
MariaDB/MySQL cannot directly index a JSON column. Use a virtual/stored generated column:

```sql
ALTER TABLE orders
  ADD COLUMN metadata_source VARCHAR(50)
    GENERATED ALWAYS AS (JSON_UNQUOTE(JSON_EXTRACT(metadata, '$.source'))) VIRTUAL,
  ADD INDEX idx_orders_metadata_source (metadata_source);
```

---

## 8. Full DDL Template (MariaDB)

```sql
CREATE TABLE orders (
  -- Primary key
  id             BIGINT UNSIGNED     NOT NULL AUTO_INCREMENT,

  -- Tenant isolation
  tenant_id      BIGINT UNSIGNED     NOT NULL,

  -- Business columns
  customer_id    BIGINT UNSIGNED     NOT NULL,
  status         VARCHAR(50)         NOT NULL DEFAULT 'pending',
  total_cents    BIGINT UNSIGNED     NOT NULL DEFAULT 0,
  idempotency_key VARCHAR(255)       NOT NULL,
  notes          TEXT                NULL,

  -- Audit fields — UTC always
  created_at     DATETIME(6)         NOT NULL DEFAULT CURRENT_TIMESTAMP(6),
  updated_at     DATETIME(6)         NOT NULL DEFAULT CURRENT_TIMESTAMP(6)
                                               ON UPDATE CURRENT_TIMESTAMP(6),
  deleted_at     DATETIME(6)         NULL,

  PRIMARY KEY (id),
  UNIQUE KEY uq_orders_tenant_idempotency (tenant_id, idempotency_key),
  CONSTRAINT chk_orders_status CHECK (status IN ('pending','confirmed','shipped','cancelled')),

  CONSTRAINT fk_orders_tenant   FOREIGN KEY (tenant_id)   REFERENCES tenants(id),
  CONSTRAINT fk_orders_customer FOREIGN KEY (customer_id) REFERENCES customers(id)
) ENGINE=InnoDB
  DEFAULT CHARSET=utf8mb4
  COLLATE=utf8mb4_unicode_ci;

-- Separate index statements (after CREATE TABLE)
CREATE INDEX idx_orders_tenant_id   ON orders (tenant_id);
CREATE INDEX idx_orders_customer_id ON orders (customer_id);
CREATE INDEX idx_orders_status      ON orders (status);
-- Partial-like: filtered index via generated column + WHERE alternative
CREATE INDEX idx_orders_tenant_status ON orders (tenant_id, status);
```

> **Note:** MariaDB/MySQL do not support true partial indexes (`WHERE` clause on `CREATE INDEX`).
> Use a generated column + index, or accept that the index covers all rows.

---

## 9. Doctrine ORM — MariaDB Configuration

```yaml
# config/packages/doctrine.yaml
doctrine:
  dbal:
    driver:   pdo_mysql
    host:     '%env(DB_HOST)%'
    port:     '%env(int:DB_PORT)%'
    dbname:   '%env(DB_NAME)%'
    user:     '%env(DB_USER)%'
    password: '%env(DB_PASSWORD)%'
    charset:  utf8mb4
    default_table_options:
      charset: utf8mb4
      collate: utf8mb4_unicode_ci
      engine:  InnoDB
    # Tell Doctrine it is talking to MariaDB (not MySQL) for correct platform detection
    server_version: 'mariadb-10.6.0'
```

```php
// Entity — MariaDB-specific annotations
#[ORM\Entity]
#[ORM\Table(
    name: 'orders',
    options: ['engine' => 'InnoDB', 'charset' => 'utf8mb4', 'collation' => 'utf8mb4_unicode_ci']
)]
class Order
{
    #[ORM\Id]
    #[ORM\GeneratedValue(strategy: 'IDENTITY')]
    #[ORM\Column(type: 'bigint', options: ['unsigned' => true])]
    private ?int $id = null;

    // Money as DECIMAL — Doctrine 'decimal' type maps to DECIMAL(19,4) by default
    #[ORM\Column(type: 'decimal', precision: 19, scale: 4)]
    private string $totalAmount = '0.0000';

    // OR as integer cents
    #[ORM\Column(name: 'total_cents', type: 'bigint', options: ['unsigned' => true])]
    private int $totalCents = 0;

    // DATETIME(6) — use datetimetz_immutable or datetime_immutable
    #[ORM\Column(name: 'created_at', type: 'datetime_immutable')]
    private \DateTimeImmutable $createdAt;
}
```

### Doctrine migration — MariaDB online DDL hint
```php
public function up(Schema $schema): void
{
    // Use ALGORITHM=INPLACE for large tables to avoid full table lock
    $this->addSql(
        'ALTER TABLE orders ADD COLUMN notes TEXT NULL, ALGORITHM=INPLACE, LOCK=NONE'
    );
    $this->addSql(
        'CREATE INDEX idx_orders_notes ON orders (notes(100))'
    );
}
```

---

## 10. Prisma — MariaDB / MySQL Configuration

```prisma
datasource db {
  provider = "mysql"    // covers both MySQL and MariaDB
  url      = env("DATABASE_URL")
  // DATABASE_URL = "mysql://user:pass@host:3306/dbname"
}

model Order {
  id              BigInt    @id @default(autoincrement()) @db.UnsignedBigInt
  tenantId        BigInt    @map("tenant_id")             @db.UnsignedBigInt
  customerId      BigInt    @map("customer_id")           @db.UnsignedBigInt
  status          String    @default("pending")           @db.VarChar(50)
  totalCents      BigInt    @default(0)  @map("total_cents")  @db.UnsignedBigInt
  idempotencyKey  String    @map("idempotency_key")       @db.VarChar(255)
  createdAt       DateTime  @default(now()) @map("created_at") @db.DateTime(6)
  updatedAt       DateTime  @updatedAt      @map("updated_at") @db.DateTime(6)
  deletedAt       DateTime? @map("deleted_at")            @db.DateTime(6)

  @@unique([tenantId, idempotencyKey], name: "uq_orders_tenant_idempotency")
  @@index([tenantId],   name: "idx_orders_tenant_id")
  @@index([customerId], name: "idx_orders_customer_id")
  @@map("orders")
}
```

---

## 11. Key Differences from PostgreSQL — Watch Out For

| Feature | PostgreSQL | MariaDB/MySQL | Mitigation |
|---------|-----------|--------------|------------|
| Partial indexes | `WHERE` clause on `CREATE INDEX` | Not supported | Use generated column + index |
| Covering index | `INCLUDE` clause | Not supported; secondary indexes include PK implicitly | Select only indexed columns |
| `TIMESTAMPTZ` | Native timezone-aware | No equivalent; use `DATETIME(6)` in UTC | Enforce UTC at app layer |
| `RETURNING` | `INSERT ... RETURNING id` | Not in MySQL 8; MariaDB 10.5+ only | Use `LAST_INSERT_ID()` or ORM |
| Sequences | Native | MariaDB 10.3+ only; MySQL uses AUTO_INCREMENT | Stick to AUTO_INCREMENT for portability |
| RLS (Row Level Security) | Native `CREATE POLICY` | Not supported | App-layer `WHERE tenant_id = ?` guard |
| `JSONB` | Binary, indexed natively | No JSONB; JSON = LONGTEXT | Generated column + index for JSON paths |
| `CREATE INDEX CONCURRENTLY` | No table lock | `ALGORITHM=INPLACE, LOCK=NONE` | Use explicit ALGORITHM/LOCK hints |
| `TEXT` search | `tsvector` + `GIN` | `FULLTEXT` index + `MATCH...AGAINST` | Both are effective; syntax differs |
| Schema (namespace) | First-class `schema` | `schema` = `database` | Use separate databases for multi-tenancy |
| `CHECK` constraints | Enforced | Enforced (MariaDB 10.2+; MySQL 8.0.16+) | Verify server version |
| Transactional DDL | DDL is transactional | DDL causes implicit commit | Never mix DDL + DML in one transaction |

> **Critical:** MariaDB/MySQL DDL (ALTER TABLE, CREATE INDEX, etc.) causes an **implicit COMMIT**.
> A migration that mixes DDL and DML in one transaction will partially commit.
> Use separate transactions for schema changes and data migrations.
