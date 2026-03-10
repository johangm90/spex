# Python / FastAPI — Deep Reference

## Project Structure

```
src/
├── domain/
│   ├── model/              ← Dataclasses / pure Python classes (entities, VOs)
│   ├── repository/         ← Repository ABCs (Abstract Base Classes)
│   ├── event/              ← Domain event dataclasses
│   └── service/            ← Domain services
├── application/
│   ├── use_case/           ← Application services / use cases
│   └── dto/                ← Pydantic input/output schemas
├── infrastructure/
│   ├── persistence/        ← SQLAlchemy repository implementations
│   ├── worker/             ← Celery tasks
│   └── security/           ← JWT utilities
├── api/
│   ├── router/             ← FastAPI routers (one per domain)
│   ├── dependency/         ← FastAPI Depends() factories
│   └── exception_handler/  ← HTTP exception mappers
├── main.py                 ← FastAPI app factory
└── config.py               ← Settings (pydantic-settings)
```

---

## Pydantic v2 — Models and Validation

```python
# application/dto/order.py
from __future__ import annotations
from pydantic import BaseModel, Field, field_validator, model_validator
from pydantic import ConfigDict
from decimal import Decimal
from datetime import datetime
import uuid

class CreateOrderRequest(BaseModel):
    model_config = ConfigDict(str_strip_whitespace=True)

    currency: str = Field(..., min_length=3, max_length=3, pattern=r'^[A-Z]{3}$')
    idempotency_key: str = Field(..., alias='idempotencyKey', min_length=1, max_length=64)

    @field_validator('currency')
    @classmethod
    def currency_must_be_supported(cls, v: str) -> str:
        supported = {'EUR', 'USD', 'GBP'}
        if v not in supported:
            raise ValueError(f'Currency must be one of {supported}')
        return v

class OrderResponse(BaseModel):
    model_config = ConfigDict(from_attributes=True)  # allows ORM model → Pydantic

    id: uuid.UUID
    user_id: uuid.UUID
    status: str
    total_cents: int   # never float for money
    currency: str
    created_at: datetime

class PaginatedOrderResponse(BaseModel):
    data: list[OrderResponse]
    total: int
    page: int
    page_size: int
    total_pages: int
```

---

## Domain Model

```python
# domain/model/order.py
from __future__ import annotations
from dataclasses import dataclass, field
from datetime import datetime, timezone
from enum import Enum
from uuid import UUID, uuid4


class OrderStatus(str, Enum):
    PENDING = 'PENDING'
    CONFIRMED = 'CONFIRMED'
    CANCELLED = 'CANCELLED'


@dataclass
class Money:
    amount_cents: int   # always integer cents — never float
    currency: str

    def __post_init__(self) -> None:
        if self.amount_cents < 0:
            raise ValueError('Amount cannot be negative')
        if len(self.currency) != 3:
            raise ValueError('Currency must be a 3-letter ISO code')

    def add(self, other: Money) -> Money:
        if self.currency != other.currency:
            raise ValueError('Cannot add different currencies')
        return Money(self.amount_cents + other.amount_cents, self.currency)


class Order:
    def __init__(self, user_id: UUID, currency: str, order_id: UUID | None = None) -> None:
        self.id: UUID = order_id or uuid4()
        self.user_id: UUID = user_id
        self.status: OrderStatus = OrderStatus.PENDING
        self.total: Money = Money(0, currency)
        self.created_at: datetime = datetime.now(tz=timezone.utc)

    def cancel(self) -> None:
        if self.status != OrderStatus.PENDING:
            raise ValueError(
                f'Only PENDING orders can be cancelled. Current status: {self.status}'
            )
        self.status = OrderStatus.CANCELLED
```

---

## SQLAlchemy 2.0 Async

```python
# infrastructure/persistence/models.py
from sqlalchemy.orm import DeclarativeBase, Mapped, mapped_column
from sqlalchemy import String, Integer, DateTime, func
import uuid

class Base(DeclarativeBase):
    pass

class OrderORM(Base):
    __tablename__ = 'orders'

    id: Mapped[uuid.UUID] = mapped_column(primary_key=True, default=uuid.uuid4)
    user_id: Mapped[uuid.UUID] = mapped_column(nullable=False, index=True)
    status: Mapped[str] = mapped_column(String(20), nullable=False, default='PENDING')
    total_cents: Mapped[int] = mapped_column(Integer, nullable=False, default=0)
    currency: Mapped[str] = mapped_column(String(3), nullable=False)
    created_at: Mapped[datetime] = mapped_column(
        DateTime(timezone=True), server_default=func.now(), nullable=False
    )
    updated_at: Mapped[datetime | None] = mapped_column(
        DateTime(timezone=True), onupdate=func.now(), nullable=True
    )

# infrastructure/persistence/database.py
from sqlalchemy.ext.asyncio import create_async_engine, async_sessionmaker, AsyncSession
from contextlib import asynccontextmanager

engine = create_async_engine(settings.database_url, pool_pre_ping=True, pool_size=10)
AsyncSessionLocal = async_sessionmaker(engine, expire_on_commit=False)

@asynccontextmanager
async def get_session() -> AsyncSession:
    async with AsyncSessionLocal() as session:
        try:
            yield session
            await session.commit()
        except Exception:
            await session.rollback()
            raise

# infrastructure/persistence/order_repository.py
from sqlalchemy.ext.asyncio import AsyncSession
from sqlalchemy import select
from domain.repository.order import OrderRepositoryABC
from domain.model.order import Order

class SQLAlchemyOrderRepository(OrderRepositoryABC):
    def __init__(self, session: AsyncSession) -> None:
        self.session = session

    async def find_by_id(self, order_id: uuid.UUID) -> Order | None:
        row = await self.session.get(OrderORM, order_id)
        return self._to_domain(row) if row else None

    async def save(self, order: Order) -> Order:
        row = await self.session.get(OrderORM, order.id)
        if row is None:
            row = OrderORM(id=order.id, user_id=order.user_id,
                           status=order.status.value,
                           total_cents=order.total.amount_cents,
                           currency=order.total.currency)
            self.session.add(row)
        else:
            row.status = order.status.value
            row.total_cents = order.total.amount_cents
        return order

    @staticmethod
    def _to_domain(row: OrderORM) -> Order:
        o = Order.__new__(Order)
        o.id = row.id
        o.user_id = row.user_id
        o.status = OrderStatus(row.status)
        o.total = Money(row.total_cents, row.currency)
        o.created_at = row.created_at
        return o
```

---

## Alembic Migrations

```bash
# Setup
alembic init alembic
# Edit alembic/env.py to import Base and use async engine

# Generate migration
alembic revision --autogenerate -m "create orders table"

# Apply
alembic upgrade head

# Rollback
alembic downgrade -1
```

```python
# alembic/env.py (async pattern)
from sqlalchemy.ext.asyncio import async_engine_from_config
from sqlalchemy import pool
from alembic import context
from infrastructure.persistence.models import Base

def run_migrations_online() -> None:
    connectable = async_engine_from_config(
        context.config.get_section(context.config.config_ini_section),
        prefix='sqlalchemy.',
        poolclass=pool.NullPool,
    )
    # use asyncio.run() wrapper — see Alembic async docs
```

**Migration rules:**
- Never drop or rename a column in the same migration that adds a replacement — do it in two separate PRs
- Always review autogenerated migrations before committing — SQLAlchemy doesn't always detect `server_default` changes correctly
- Run `alembic check` in CI to verify no unapplied migrations are pending

---

## FastAPI Dependency Injection

```python
# api/dependency/database.py
from fastapi import Depends
from sqlalchemy.ext.asyncio import AsyncSession
from infrastructure.persistence.database import AsyncSessionLocal

async def get_db() -> AsyncSession:
    async with AsyncSessionLocal() as session:
        yield session

# api/dependency/repositories.py
def get_order_repository(db: AsyncSession = Depends(get_db)) -> OrderRepositoryABC:
    return SQLAlchemyOrderRepository(db)

def get_create_order_use_case(
    repo: OrderRepositoryABC = Depends(get_order_repository),
) -> CreateOrderUseCase:
    return CreateOrderUseCase(repo)

# api/router/orders.py
from fastapi import APIRouter, Depends, Header, HTTPException, status

router = APIRouter(prefix='/orders', tags=['orders'])

@router.post('/', response_model=OrderResponse, status_code=201)
async def create_order(
    body: CreateOrderRequest,
    idempotency_key: str = Header(alias='Idempotency-Key'),
    current_user: User = Depends(get_current_user),
    use_case: CreateOrderUseCase = Depends(get_create_order_use_case),
) -> OrderResponse:
    try:
        order = await use_case.execute(CreateOrderCommand(
            idempotency_key=idempotency_key,
            currency=body.currency,
            user_id=current_user.id,
        ))
    except DuplicateIdempotencyKeyError as e:
        raise HTTPException(status_code=409, detail={
            'code': 'DUPLICATE_IDEMPOTENCY_KEY',
            'message': str(e),
        })
    return OrderResponse.model_validate(order)
```

---

## JWT Authentication

```python
# infrastructure/security/jwt.py
from datetime import datetime, timedelta, timezone
from jose import jwt, JWTError
from passlib.context import CryptContext
from fastapi import Depends, HTTPException, status
from fastapi.security import HTTPBearer, HTTPAuthorizationCredentials
from config import settings

pwd_context = CryptContext(schemes=['bcrypt'], deprecated='auto')
bearer_scheme = HTTPBearer()

def create_access_token(sub: str, extra: dict = {}) -> str:
    payload = {
        'sub': sub,
        'iat': datetime.now(tz=timezone.utc),
        'exp': datetime.now(tz=timezone.utc) + timedelta(minutes=15),
        **extra,
    }
    return jwt.encode(payload, settings.jwt_secret, algorithm='HS256')

async def get_current_user(
    credentials: HTTPAuthorizationCredentials = Depends(bearer_scheme),
    db: AsyncSession = Depends(get_db),
) -> User:
    try:
        payload = jwt.decode(credentials.credentials, settings.jwt_secret, algorithms=['HS256'])
        user_id: str = payload.get('sub')
        if user_id is None:
            raise JWTError()
    except JWTError:
        raise HTTPException(
            status_code=status.HTTP_401_UNAUTHORIZED,
            detail={'code': 'INVALID_TOKEN', 'message': 'Invalid or expired token'},
            headers={'WWW-Authenticate': 'Bearer'},
        )
    user = await user_repo.find_by_id(user_id)
    if user is None:
        raise HTTPException(status_code=401, detail={'code': 'USER_NOT_FOUND', 'message': ''})
    return user
```

---

## Global Exception Handler

```python
# main.py
from fastapi import FastAPI, Request
from fastapi.responses import JSONResponse
from fastapi.exceptions import RequestValidationError

app = FastAPI()

@app.exception_handler(RequestValidationError)
async def validation_handler(request: Request, exc: RequestValidationError) -> JSONResponse:
    return JSONResponse(status_code=400, content={
        'code': 'VALIDATION_ERROR',
        'message': 'Request validation failed',
        'details': [
            {'field': '.'.join(str(l) for l in e['loc'][1:]), 'issue': e['msg']}
            for e in exc.errors()
        ],
    })

@app.exception_handler(ValueError)
async def value_error_handler(request: Request, exc: ValueError) -> JSONResponse:
    return JSONResponse(status_code=422, content={
        'code': 'BUSINESS_RULE_VIOLATION',
        'message': str(exc),
    })

@app.exception_handler(Exception)
async def generic_handler(request: Request, exc: Exception) -> JSONResponse:
    # Log but never leak stack trace
    import logging; logging.exception(exc)
    return JSONResponse(status_code=500, content={
        'code': 'INTERNAL_ERROR',
        'message': 'An error occurred',
    })
```

---

## Celery (Background Tasks)

```python
# infrastructure/worker/celery.py
from celery import Celery
from config import settings

celery_app = Celery('tasks', broker=settings.redis_url, backend=settings.redis_url)
celery_app.conf.task_serializer = 'json'
celery_app.conf.result_serializer = 'json'
celery_app.conf.task_acks_late = True  # re-queue on worker crash

# infrastructure/worker/tasks.py
from .celery import celery_app

@celery_app.task(bind=True, max_retries=3, default_retry_delay=60)
def send_order_confirmation(self, order_id: str) -> None:
    try:
        mailer.send_order_confirmation(order_id)
    except Exception as exc:
        raise self.retry(exc=exc, countdown=2 ** self.request.retries)

# Enqueue from use case
from infrastructure.worker.tasks import send_order_confirmation
send_order_confirmation.apply_async(args=[str(order.id)], countdown=0)
```

---

## Testing — pytest + httpx

```python
# tests/unit/domain/test_order.py
import pytest
from domain.model.order import Order, OrderStatus

def test_cancel_pending_order():
    order = Order(user_id=uuid4(), currency='EUR')
    order.cancel()
    assert order.status == OrderStatus.CANCELLED

def test_cannot_cancel_cancelled_order():
    order = Order(user_id=uuid4(), currency='EUR')
    order.cancel()
    with pytest.raises(ValueError, match='Only PENDING'):
        order.cancel()

# tests/integration/test_orders_api.py
import pytest
from httpx import AsyncClient, ASGITransport
from main import app

@pytest.fixture
async def client():
    async with AsyncClient(transport=ASGITransport(app=app), base_url='http://test') as c:
        yield c

@pytest.mark.asyncio
async def test_create_order(client, auth_headers):
    response = await client.post('/api/orders',
        json={'currency': 'EUR'},
        headers={**auth_headers, 'Idempotency-Key': str(uuid4())},
    )
    assert response.status_code == 201
    assert response.json()['status'] == 'PENDING'

@pytest.mark.asyncio
async def test_create_order_idempotency(client, auth_headers):
    key = str(uuid4())
    headers = {**auth_headers, 'Idempotency-Key': key}

    r1 = await client.post('/api/orders', json={'currency': 'EUR'}, headers=headers)
    r2 = await client.post('/api/orders', json={'currency': 'EUR'}, headers=headers)

    assert r1.status_code == 201
    assert r2.status_code == 201
    assert r1.json()['id'] == r2.json()['id']  # same resource returned
```

```toml
# pyproject.toml — test configuration
[tool.pytest.ini_options]
asyncio_mode = "auto"
testpaths = ["tests"]

[tool.coverage.run]
source = ["src"]
omit = ["*/migrations/*", "*/test*"]
```

---

## Settings (pydantic-settings)

```python
# config.py
from pydantic_settings import BaseSettings, SettingsConfigDict

class Settings(BaseSettings):
    model_config = SettingsConfigDict(env_file='.env', env_file_encoding='utf-8')

    database_url: str
    jwt_secret: str
    jwt_algorithm: str = 'HS256'
    redis_url: str = 'redis://localhost:6379/0'
    debug: bool = False

settings = Settings()
```

---

## Common Gotchas

🔴 **`async def` route but sync DB call** — mixing asyncio and sync SQLAlchemy blocks the event loop. Always use `async_engine` + `AsyncSession` for async FastAPI routes, or run sync calls in a thread pool via `asyncio.get_event_loop().run_in_executor()`.

🔴 **Pydantic v2 breaking changes from v1** — `orm_mode = True` is now `model_config = ConfigDict(from_attributes=True)`; `validator` is now `field_validator`; `__fields__` is now `model_fields`. Don't mix v1 and v2 syntax.

🟠 **SQLAlchemy `expire_on_commit=True` (default)** — after `session.commit()`, all loaded objects are expired and accessing their attributes triggers a new lazy load. Use `expire_on_commit=False` in `async_sessionmaker` for async patterns, or load what you need before committing.

🟠 **Celery and asyncio** — Celery tasks are synchronous by default. Do not use `asyncio.run()` inside a Celery task worker; use a sync DB session or the `asgiref` sync-to-async bridge if needed.

🔵 **`Depends()` caching** — FastAPI caches dependency instances per request by default. If your repository holds a session and you inject it in multiple places, they share the same session — which is usually correct. Use `use_cache=False` if you need a fresh instance.
