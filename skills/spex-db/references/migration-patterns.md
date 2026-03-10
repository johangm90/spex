# Migration Patterns — spex-db

Safe migration patterns for schema changes. All migrations must be safe to run forward and back.
Includes tool-specific snippets for Doctrine, Prisma, Alembic, and Flyway.

---

## Core Rule: Additive vs Destructive

| Type | Examples | Safety |
|------|----------|--------|
| **Additive** | Add column (nullable or with default), add table, add index, add FK | Always safe — zero downtime |
| **Destructive** | Drop column, drop table, rename column, change column type | Requires multi-step strategy + separate ADR |

**Default:** always prefer additive. If a destructive change is required, follow the multi-step strategy below.

---

## Zero-Downtime Techniques

1. **Add before remove** — add the new column/table, migrate data, then remove the old one in a later migration
2. **Dual-write period** — application writes to both old and new columns during transition
3. **Background backfill** — populate new column with a background job before making it NOT NULL
4. **Feature flag guard** — deploy the schema change behind a flag; activate after verifying backfill is complete
5. **Index concurrently** — on PostgreSQL, use `CREATE INDEX CONCURRENTLY` to avoid table locks
6. **Batched UPDATE** — backfill large tables in batches of 1,000–10,000 rows with a short sleep between batches to avoid lock contention

---

## Nullable-First Approach (Column Lifecycle)

### Adding a required column to an existing table

| Step | Migration | Description |
|------|-----------|-------------|
| 1 | `ALTER TABLE … ADD COLUMN foo TEXT NULL` | Add nullable — safe, no backfill needed yet |
| 2 | Backfill job (in-app or SQL script) | Populate `foo` for all existing rows |
| 3 | `ALTER TABLE … ALTER COLUMN foo SET NOT NULL` | Enforce NOT NULL after backfill is complete |

### Dropping a column

| Step | Migration | Description |
|------|-----------|-------------|
| 1 | Make column nullable (`SET NULL`), stop writing to it | Decouple app from column |
| 2 | Remove column from all reads in application | Column is orphaned |
| 3 | `ALTER TABLE … DROP COLUMN foo` | Safe to drop — no app dependency remains |

**Never drop a column in the same migration (or same PR) that stops using it.**

---

## Multi-Step Strategy for Breaking Changes

```
Migration N   — Add new structure (new column/table), keep old
Migration N+1 — Migrate data; application uses new structure (separate PR)
Migration N+2 — Drop old structure (separate PR, post-verification + backup)
```

Each step requires a separate PR. Steps N+1 and N+2 require a rollback plan.

---

## Rollback Plans

Every migration note in a `db_design` artifact must include a rollback plan:

```
Migration: Add `subscription_tier` column to `accounts`
Forward:   ALTER TABLE accounts ADD COLUMN subscription_tier TEXT NULL DEFAULT 'free';
Rollback:  ALTER TABLE accounts DROP COLUMN subscription_tier;
Risk:      Low — additive, nullable, has default
```

For destructive migrations:

```
Migration: Drop deprecated `legacy_plan` column from `accounts`
Forward:   ALTER TABLE accounts DROP COLUMN legacy_plan;
Rollback:  ALTER TABLE accounts ADD COLUMN legacy_plan TEXT NULL;
           -- Data cannot be recovered without a pre-migration backup snapshot
Risk:      HIGH — take a backup snapshot before running; verify in staging first
```

---

## Large Table Migration Strategies

For tables with > 10 million rows:

### Adding a NOT NULL column with a default (PostgreSQL 11+)
```sql
-- PostgreSQL 11+ stores the default in catalog metadata — no table rewrite needed
ALTER TABLE large_table ADD COLUMN new_col TEXT NOT NULL DEFAULT 'value';
```

### Adding a NOT NULL column (PostgreSQL < 11 or no constant default)
```sql
-- Step 1: Add nullable (instant)
ALTER TABLE large_table ADD COLUMN new_col TEXT NULL;

-- Step 2: Backfill in batches (run as a script or background job)
DO $$
DECLARE batch_size INT := 5000; last_id BIGINT := 0; max_id BIGINT;
BEGIN
  SELECT MAX(id) INTO max_id FROM large_table;
  WHILE last_id < max_id LOOP
    UPDATE large_table SET new_col = 'default_value'
    WHERE id > last_id AND id <= last_id + batch_size AND new_col IS NULL;
    last_id := last_id + batch_size;
    PERFORM pg_sleep(0.1);  -- brief pause to reduce lock pressure
  END LOOP;
END $$;

-- Step 3: Enforce NOT NULL (fast — all rows already populated)
ALTER TABLE large_table ALTER COLUMN new_col SET NOT NULL;
```

### Adding an index to a large table (PostgreSQL)
```sql
-- CONCURRENTLY: no table lock, but takes longer; cannot run inside a transaction block
CREATE INDEX CONCURRENTLY idx_large_table_new_col ON large_table(new_col);
```

### Renaming a column (zero-downtime 3-step)
```sql
-- Step 1: Add new column, write to both
ALTER TABLE orders ADD COLUMN order_ref TEXT NULL;
-- (app writes to both order_number and order_ref)

-- Step 2: Backfill, then switch reads to new column
UPDATE orders SET order_ref = order_number WHERE order_ref IS NULL;

-- Step 3: Drop old column after all app versions have been deployed
ALTER TABLE orders DROP COLUMN order_number;
```

---

## Tool-Specific Snippets

### Doctrine Migrations (Symfony / PHP)

```php
// migrations/Version20260101000000.php
declare(strict_types=1);

namespace DoctrineMigrations;

use Doctrine\DBAL\Schema\Schema;
use Doctrine\Migrations\AbstractMigration;

final class Version20260101000000 extends AbstractMigration
{
    public function getDescription(): string
    {
        return 'Add subscription_tier to accounts';
    }

    public function up(Schema $schema): void
    {
        // Additive — safe
        $this->addSql(
            'ALTER TABLE accounts ADD subscription_tier VARCHAR(50) NOT NULL DEFAULT \'free\''
        );
        $this->addSql(
            'CREATE INDEX idx_accounts_subscription_tier ON accounts (subscription_tier)'
        );
    }

    public function down(Schema $schema): void
    {
        $this->addSql('DROP INDEX idx_accounts_subscription_tier ON accounts');
        $this->addSql('ALTER TABLE accounts DROP subscription_tier');
    }
}
```

Run: `php bin/console doctrine:migrations:migrate`
Dry run: `php bin/console doctrine:migrations:migrate --dry-run`
Status: `php bin/console doctrine:migrations:status`

### Prisma Migrate (Node.js / TypeScript)

```prisma
// prisma/schema.prisma — add the field
model Account {
  id               BigInt    @id @default(autoincrement())
  subscriptionTier String    @default("free") @map("subscription_tier")
  createdAt        DateTime  @default(now()) @map("created_at")
  updatedAt        DateTime  @updatedAt @map("updated_at")

  @@index([subscriptionTier], name: "idx_accounts_subscription_tier")
  @@map("accounts")
}
```

```bash
# Generate and apply migration
npx prisma migrate dev --name add_subscription_tier_to_accounts

# Apply to production (no prompt, no shadow DB)
npx prisma migrate deploy

# Check migration status
npx prisma migrate status
```

Generated migration file (`prisma/migrations/<timestamp>_add_subscription_tier.sql`):
```sql
ALTER TABLE "accounts" ADD COLUMN "subscription_tier" TEXT NOT NULL DEFAULT 'free';
CREATE INDEX "idx_accounts_subscription_tier" ON "accounts"("subscription_tier");
```

### Alembic (Python / SQLAlchemy)

```python
# alembic/versions/abc123_add_subscription_tier.py
"""Add subscription_tier to accounts

Revision ID: abc123
Revises: prev_revision
Create Date: 2026-01-01 00:00:00
"""
from alembic import op
import sqlalchemy as sa

revision = "abc123"
down_revision = "prev_revision"
branch_labels = None
depends_on = None


def upgrade() -> None:
    op.add_column(
        "accounts",
        sa.Column("subscription_tier", sa.Text(), nullable=False, server_default="free"),
    )
    op.create_index(
        "idx_accounts_subscription_tier", "accounts", ["subscription_tier"]
    )


def downgrade() -> None:
    op.drop_index("idx_accounts_subscription_tier", table_name="accounts")
    op.drop_column("accounts", "subscription_tier")
```

```bash
# Generate migration from model diff
alembic revision --autogenerate -m "add_subscription_tier_to_accounts"

# Apply
alembic upgrade head

# Rollback one step
alembic downgrade -1

# Show history
alembic history --verbose
```

### Flyway (Kotlin / Spring Boot / Java)

```sql
-- src/main/resources/db/migration/V20260101__add_subscription_tier_to_accounts.sql
ALTER TABLE accounts ADD COLUMN subscription_tier VARCHAR(50) NOT NULL DEFAULT 'free';
CREATE INDEX idx_accounts_subscription_tier ON accounts(subscription_tier);
```

Naming convention: `V<version>__<description>.sql` (two underscores).
Rollback: `U<version>__<description>.sql` (Flyway Teams / paid only; or use manual undo scripts).

```bash
# Apply migrations
./mvnw flyway:migrate -Dflyway.url=... -Dflyway.user=... -Dflyway.password=...

# Check status
./mvnw flyway:info
```

---

## FK Index Requirements

Every foreign key column **must** have an explicit index in the same migration:

```sql
-- Add FK + index together
ALTER TABLE orders ADD COLUMN customer_id BIGINT NOT NULL REFERENCES customers(id);
CREATE INDEX idx_orders_customer_id ON orders(customer_id);
```

Without this index: FK lookups cause sequential scans; ON DELETE/UPDATE operations lock the entire table.

---

## Checklist for Each Migration

- [ ] Is this additive? If not, does it follow the 3-step strategy?
- [ ] Does every new FK column have a corresponding index in the same migration?
- [ ] Is a rollback (`down`) documented?
- [ ] Does the migration run without locking production tables?
  - PostgreSQL: use `CREATE INDEX CONCURRENTLY`; add nullable columns; avoid `ALTER TYPE`
  - MySQL: `ALTER TABLE … ALGORITHM=INPLACE, LOCK=NONE` where supported
- [ ] If adding a NOT NULL column, is there a constant default or a batched backfill plan?
- [ ] Is a separate ADR referenced for any destructive change?
- [ ] Has the migration been tested on a data snapshot of production size?
