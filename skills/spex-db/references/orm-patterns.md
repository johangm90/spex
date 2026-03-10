# ORM Patterns Reference — spex-db

Canonical entity definitions, repository patterns, query builders, and migration commands
for Doctrine ORM, Prisma, SQLAlchemy 2.x, and TypeORM.

---

## 1. Doctrine ORM (Symfony / PHP)

### Entity with full conventions
```php
<?php
// src/Entity/Order.php
declare(strict_types=1);

namespace App\Entity;

use Doctrine\ORM\Mapping as ORM;
use Symfony\Bridge\Doctrine\Types\UlidType;
use Symfony\Component\Uid\Ulid;

#[ORM\Entity(repositoryClass: OrderRepository::class)]
#[ORM\Table(name: 'orders')]
#[ORM\Index(columns: ['tenant_id'], name: 'idx_orders_tenant_id')]
#[ORM\Index(columns: ['customer_id'], name: 'idx_orders_customer_id')]
#[ORM\Index(columns: ['status'], name: 'idx_orders_status')]
#[ORM\HasLifecycleCallbacks]
class Order
{
    #[ORM\Id]
    #[ORM\GeneratedValue(strategy: 'IDENTITY')]
    #[ORM\Column(type: 'bigint')]
    private ?int $id = null;

    #[ORM\ManyToOne(targetEntity: Tenant::class)]
    #[ORM\JoinColumn(name: 'tenant_id', referencedColumnName: 'id', nullable: false)]
    private Tenant $tenant;

    #[ORM\ManyToOne(targetEntity: Customer::class)]
    #[ORM\JoinColumn(name: 'customer_id', referencedColumnName: 'id', nullable: false)]
    private Customer $customer;

    #[ORM\Column(type: 'string', length: 50)]
    private string $status = 'pending';

    // Money as integer cents — never float
    #[ORM\Column(type: 'bigint', options: ['unsigned' => true])]
    private int $totalCents = 0;

    #[ORM\Column(name: 'idempotency_key', type: 'string', length: 255, unique: true)]
    private string $idempotencyKey;

    #[ORM\Column(name: 'created_at', type: 'datetimetz_immutable')]
    private \DateTimeImmutable $createdAt;

    #[ORM\Column(name: 'updated_at', type: 'datetimetz_immutable')]
    private \DateTimeImmutable $updatedAt;

    #[ORM\Column(name: 'deleted_at', type: 'datetimetz_immutable', nullable: true)]
    private ?\DateTimeImmutable $deletedAt = null;

    #[ORM\PrePersist]
    public function onPrePersist(): void
    {
        $this->createdAt = new \DateTimeImmutable();
        $this->updatedAt = new \DateTimeImmutable();
    }

    #[ORM\PreUpdate]
    public function onPreUpdate(): void
    {
        $this->updatedAt = new \DateTimeImmutable();
    }

    // Getters / setters omitted for brevity
}
```

### Repository with QueryBuilder
```php
<?php
// src/Repository/OrderRepository.php
namespace App\Repository;

use App\Entity\Order;
use Doctrine\Bundle\DoctrineBundle\Repository\ServiceEntityRepository;
use Doctrine\Persistence\ManagerRegistry;

/**
 * @extends ServiceEntityRepository<Order>
 */
class OrderRepository extends ServiceEntityRepository
{
    public function __construct(ManagerRegistry $registry)
    {
        parent::__construct($registry, Order::class);
    }

    /**
     * @return Order[]
     */
    public function findActiveByTenant(int $tenantId, string $status = null): array
    {
        $qb = $this->createQueryBuilder('o')
            ->andWhere('o.tenant = :tenantId')
            ->andWhere('o.deletedAt IS NULL')
            ->setParameter('tenantId', $tenantId)
            ->orderBy('o.createdAt', 'DESC');

        if ($status !== null) {
            $qb->andWhere('o.status = :status')
               ->setParameter('status', $status);
        }

        return $qb->getQuery()->getResult();
    }

    public function findByIdempotencyKey(int $tenantId, string $key): ?Order
    {
        return $this->findOneBy([
            'tenant'          => $tenantId,
            'idempotencyKey'  => $key,
        ]);
    }
}
```

### Doctrine migration commands
```bash
# Generate a migration from entity diff
php bin/console doctrine:migrations:diff

# Apply all pending migrations
php bin/console doctrine:migrations:migrate

# Dry run (see SQL without executing)
php bin/console doctrine:migrations:migrate --dry-run

# Rollback one migration
php bin/console doctrine:migrations:execute --down 'DoctrineMigrations\Version20260101000000'

# Show migration status
php bin/console doctrine:migrations:status

# Validate entity <-> schema sync
php bin/console doctrine:schema:validate
```

---

## 2. Prisma (Node.js / TypeScript)

### Schema with all conventions
```prisma
// prisma/schema.prisma
generator client {
  provider = "prisma-client-js"
}

datasource db {
  provider = "postgresql"
  url      = env("DATABASE_URL")
}

model Tenant {
  id        BigInt    @id @default(autoincrement())
  name      String
  createdAt DateTime  @default(now()) @map("created_at")
  updatedAt DateTime  @updatedAt      @map("updated_at")
  orders    Order[]

  @@map("tenants")
}

model Order {
  id              BigInt    @id @default(autoincrement())
  tenant          Tenant    @relation(fields: [tenantId], references: [id])
  tenantId        BigInt    @map("tenant_id")
  customer        Customer  @relation(fields: [customerId], references: [id])
  customerId      BigInt    @map("customer_id")
  status          String    @default("pending")
  totalCents      BigInt    @default(0)     @map("total_cents")
  idempotencyKey  String    @unique         @map("idempotency_key")
  createdAt       DateTime  @default(now()) @map("created_at")
  updatedAt       DateTime  @updatedAt      @map("updated_at")
  deletedAt       DateTime?               @map("deleted_at")
  items           OrderItem[]

  @@index([tenantId],   name: "idx_orders_tenant_id")
  @@index([customerId], name: "idx_orders_customer_id")
  @@index([status],     name: "idx_orders_status")
  @@map("orders")
}
```

### Prisma Client — typed queries
```ts
import { PrismaClient } from "@prisma/client";
const prisma = new PrismaClient();

// Find with relations
const orders = await prisma.order.findMany({
  where: {
    tenantId: BigInt(42),
    deletedAt: null,
    status: "pending",
  },
  include: {
    customer: { select: { id: true, name: true, email: true } },
    items: true,
  },
  orderBy: { createdAt: "desc" },
  take: 20,
  skip: 0,
});

// Upsert with idempotency key
const order = await prisma.order.upsert({
  where: { idempotencyKey: "idem-key-abc" },
  create: {
    tenantId:        BigInt(42),
    customerId:      BigInt(7),
    totalCents:      BigInt(9900),
    idempotencyKey: "idem-key-abc",
  },
  update: {},  // no-op if already exists
});

// Transaction
const [order, payment] = await prisma.$transaction([
  prisma.order.create({ data: orderData }),
  prisma.payment.create({ data: paymentData }),
]);

// Raw query (when QueryBuilder is not enough)
const result = await prisma.$queryRaw<{ id: bigint; total: bigint }[]>`
  SELECT id, SUM(total_cents) AS total
  FROM orders
  WHERE tenant_id = ${tenantId}
    AND deleted_at IS NULL
  GROUP BY id
`;
```

### Prisma migration commands
```bash
# Create and apply a new migration (dev)
npx prisma migrate dev --name add_subscription_tier

# Apply to production (no schema generation, no shadow DB)
npx prisma migrate deploy

# Reset dev database (drop + re-migrate + seed)
npx prisma migrate reset

# Check migration status
npx prisma migrate status

# Generate Prisma Client after schema change
npx prisma generate

# Open Prisma Studio (browser-based DB explorer)
npx prisma studio
```

---

## 3. SQLAlchemy 2.x (Python / FastAPI)

### Declarative model with async
```python
# app/db/models/order.py
from __future__ import annotations
from datetime import datetime, timezone
from decimal import Decimal
from typing import Optional

from sqlalchemy import BigInteger, ForeignKey, Index, String, Text, func
from sqlalchemy.orm import DeclarativeBase, Mapped, mapped_column, relationship


class Base(DeclarativeBase):
    pass


class Order(Base):
    __tablename__ = "orders"
    __table_args__ = (
        Index("idx_orders_tenant_id",   "tenant_id"),
        Index("idx_orders_customer_id", "customer_id"),
        Index("idx_orders_status",      "status"),
    )

    id:              Mapped[int]            = mapped_column(BigInteger, primary_key=True)
    tenant_id:       Mapped[int]            = mapped_column(BigInteger, ForeignKey("tenants.id"), nullable=False)
    customer_id:     Mapped[int]            = mapped_column(BigInteger, ForeignKey("customers.id"), nullable=False)
    status:          Mapped[str]            = mapped_column(String(50), nullable=False, default="pending")
    total_cents:     Mapped[int]            = mapped_column(BigInteger, nullable=False, default=0)
    idempotency_key: Mapped[str]            = mapped_column(Text, nullable=False, unique=True)
    created_at:      Mapped[datetime]       = mapped_column(nullable=False, server_default=func.now())
    updated_at:      Mapped[datetime]       = mapped_column(nullable=False, server_default=func.now(), onupdate=func.now())
    deleted_at:      Mapped[Optional[datetime]] = mapped_column(nullable=True, default=None)

    tenant:    Mapped["Tenant"]    = relationship(back_populates="orders")
    customer:  Mapped["Customer"]  = relationship(back_populates="orders")
    items:     Mapped[list["OrderItem"]] = relationship(back_populates="order")
```

### Async session factory
```python
# app/db/session.py
from sqlalchemy.ext.asyncio import AsyncSession, async_sessionmaker, create_async_engine

engine = create_async_engine(
    "postgresql+asyncpg://user:pass@localhost/dbname",
    pool_size=10,
    max_overflow=20,
    pool_pre_ping=True,
)

AsyncSessionLocal = async_sessionmaker(engine, class_=AsyncSession, expire_on_commit=False)


async def get_session() -> AsyncSession:
    async with AsyncSessionLocal() as session:
        yield session
```

### Repository pattern
```python
# app/repositories/order_repository.py
from sqlalchemy import select
from sqlalchemy.ext.asyncio import AsyncSession
from sqlalchemy.orm import selectinload

from app.db.models.order import Order


class OrderRepository:
    def __init__(self, session: AsyncSession) -> None:
        self._session = session

    async def find_active_by_tenant(self, tenant_id: int, status: str | None = None) -> list[Order]:
        stmt = (
            select(Order)
            .where(Order.tenant_id == tenant_id, Order.deleted_at.is_(None))
            .options(selectinload(Order.items))
            .order_by(Order.created_at.desc())
        )
        if status is not None:
            stmt = stmt.where(Order.status == status)
        result = await self._session.execute(stmt)
        return list(result.scalars().all())

    async def find_by_idempotency_key(self, tenant_id: int, key: str) -> Order | None:
        result = await self._session.execute(
            select(Order).where(
                Order.tenant_id == tenant_id,
                Order.idempotency_key == key,
            )
        )
        return result.scalar_one_or_none()

    async def create(self, order: Order) -> Order:
        self._session.add(order)
        await self._session.flush()   # assigns PK without committing
        return order
```

### Alembic migration commands
```bash
# Generate migration from model diff
alembic revision --autogenerate -m "add_subscription_tier_to_accounts"

# Apply all pending
alembic upgrade head

# Apply one step
alembic upgrade +1

# Rollback one step
alembic downgrade -1

# Rollback to specific revision
alembic downgrade abc123

# Show migration history
alembic history --verbose

# Show current revision in DB
alembic current
```

### Alembic async setup
```python
# alembic/env.py
import asyncio
from sqlalchemy.ext.asyncio import create_async_engine
from alembic import context
from app.db.models import Base   # import all models to populate metadata

def run_migrations_online() -> None:
    connectable = create_async_engine(context.config.get_main_option("sqlalchemy.url"))

    async def do_run():
        async with connectable.connect() as conn:
            await conn.run_sync(
                lambda sync_conn: context.configure(
                    connection=sync_conn,
                    target_metadata=Base.metadata,
                    compare_type=True,
                )
            )
            async with conn.begin():
                await conn.run_sync(lambda _: context.run_migrations())

    asyncio.run(do_run())
```

---

## 4. TypeORM (Node.js / TypeScript)

### Entity with decorators
```ts
// src/entities/Order.ts
import {
  Entity, PrimaryGeneratedColumn, Column, ManyToOne, OneToMany,
  CreateDateColumn, UpdateDateColumn, DeleteDateColumn,
  JoinColumn, Index, Unique,
} from "typeorm";
import { Tenant } from "./Tenant";
import { Customer } from "./Customer";
import { OrderItem } from "./OrderItem";

@Entity("orders")
@Index("idx_orders_tenant_id",   ["tenantId"])
@Index("idx_orders_customer_id", ["customerId"])
@Index("idx_orders_status",      ["status"])
@Unique("uq_orders_idempotency", ["tenantId", "idempotencyKey"])
export class Order {
  @PrimaryGeneratedColumn("increment", { type: "bigint" })
  id: number;

  @Column({ name: "tenant_id", type: "bigint" })
  tenantId: number;

  @ManyToOne(() => Tenant, { nullable: false })
  @JoinColumn({ name: "tenant_id" })
  tenant: Tenant;

  @Column({ name: "customer_id", type: "bigint" })
  customerId: number;

  @ManyToOne(() => Customer, { nullable: false })
  @JoinColumn({ name: "customer_id" })
  customer: Customer;

  @Column({ type: "varchar", length: 50, default: "pending" })
  status: string;

  @Column({ name: "total_cents", type: "bigint", default: 0 })
  totalCents: number;

  @Column({ name: "idempotency_key", type: "text" })
  idempotencyKey: string;

  @CreateDateColumn({ name: "created_at", type: "timestamptz" })
  createdAt: Date;

  @UpdateDateColumn({ name: "updated_at", type: "timestamptz" })
  updatedAt: Date;

  @DeleteDateColumn({ name: "deleted_at", type: "timestamptz", nullable: true })
  deletedAt: Date | null;

  @OneToMany(() => OrderItem, (item) => item.order)
  items: OrderItem[];
}
```

### Repository with QueryBuilder
```ts
// src/repositories/OrderRepository.ts
import { DataSource, Repository } from "typeorm";
import { Order } from "../entities/Order";

export class OrderRepository {
  private repo: Repository<Order>;

  constructor(ds: DataSource) {
    this.repo = ds.getRepository(Order);
  }

  async findActiveByTenant(tenantId: number, status?: string): Promise<Order[]> {
    const qb = this.repo.createQueryBuilder("o")
      .leftJoinAndSelect("o.items", "item")
      .where("o.tenantId = :tenantId", { tenantId })
      .andWhere("o.deletedAt IS NULL")
      .orderBy("o.createdAt", "DESC");

    if (status) {
      qb.andWhere("o.status = :status", { status });
    }

    return qb.getMany();
  }

  async findByIdempotencyKey(tenantId: number, key: string): Promise<Order | null> {
    return this.repo.findOneBy({ tenantId, idempotencyKey: key });
  }
}
```

### TypeORM migration commands
```bash
# Generate migration from entity diff
npx typeorm migration:generate src/migrations/AddSubscriptionTier -d src/data-source.ts

# Run all pending migrations
npx typeorm migration:run -d src/data-source.ts

# Revert last migration
npx typeorm migration:revert -d src/data-source.ts

# Show migration status
npx typeorm migration:show -d src/data-source.ts
```

---

## 5. Common Pitfalls Across ORMs

| Pitfall | Fix |
|---------|-----|
| **N+1 queries** — loading relations in a loop | Use eager loading / JOIN in the initial query (`selectinload`, `include`, `joinAndSelect`) |
| **Float for money** | Always `BIGINT` cents or `DECIMAL(19,4)` in DB; map to `Decimal`/`bigint` in app |
| **Missing `onupdate` for `updated_at`** | Use `@UpdateDateColumn` (TypeORM), `onupdate=func.now()` (SQLAlchemy), `@PreUpdate` (Doctrine), `@updatedAt` (Prisma) |
| **Autogenerated migration ignores custom SQL** | Always review autogenerated migrations; hand-write trigger and RLS policy migrations |
| **`findAll()` without pagination on large tables** | Always add `LIMIT`/`take` + `OFFSET`/`skip`; use cursor-based pagination for > 10k rows |
| **`SELECT *` in production queries** | Use `select: [...]` (Prisma) / `.select([...])` (TypeORM) / `selectinload` with explicit columns (SQLAlchemy) |
| **Shared entity across bounded contexts** | Each bounded context should own its entity class; share data via API, not shared ORM objects |
| **Long-running transactions holding connections** | Keep transactions short; batch large backfills outside a transaction |
