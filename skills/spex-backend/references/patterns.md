# Backend Patterns — Universal Reference

## Clean Architecture — Layer Rules

```
Presentation → Application → Domain ← Infrastructure
```

| Layer | Allowed dependencies | Forbidden |
|-------|---------------------|-----------|
| Presentation | Application, Domain DTOs | Domain entities, Infrastructure |
| Application | Domain | Presentation, Infrastructure (inject via interface) |
| Domain | Nothing (pure) | All framework imports |
| Infrastructure | Domain interfaces | Presentation, Application (except via injection) |

**DTO placement:**
- **Request DTOs**: Presentation layer — validated before reaching Application
- **Response DTOs**: Presentation layer — assembled from domain output
- **Domain models**: Never serialised directly to HTTP; always mapped

---

## DDD Building Blocks

### Entity
Has identity; equality by ID; mutable lifecycle.
```
User { id, email, name }  → identity = id
Order { id, userId, lines[], status }  → identity = id
```

### Value Object
No identity; equality by value; immutable.
```
Money { amount: int, currency: string }
Email { value: string }  ← validated in constructor
Address { street, city, country }
```

### Aggregate
Cluster of entities + value objects with one Aggregate Root.
- **Rule**: Only the root is accessible from outside the aggregate
- **Rule**: All invariants are enforced inside the aggregate
- **Rule**: Aggregates reference other aggregates by ID only (never by direct object reference)

### Repository Interface (Domain layer)
```
interface OrderRepository {
    findById(id: string): Order | null
    findByUserId(userId: string): Order[]
    save(order: Order): void
    delete(id: string): void
}
```

### Domain Event
Represents something that happened. Immutable, named in past tense.
```
OrderPlaced { orderId, userId, total, occurredAt }
PaymentFailed { orderId, reason, occurredAt }
```

### Domain Service
Stateless logic that doesn't belong in a single entity.
```
PricingService.calculateOrderTotal(order, discountPolicy): Money
```

---

## REST vs GraphQL — Decision Table

| Criterion | REST | GraphQL |
|-----------|------|---------|
| Simple CRUD resources | ✅ Ideal | Overkill |
| Complex nested data requirements | Many round-trips | ✅ Single query |
| Public / 3rd-party API | ✅ Familiar, cacheable | Possible but complex |
| Mobile with bandwidth constraints | N+1 risk | ✅ Precise fetching |
| Team experience | Universal | Requires expertise |
| Caching (CDN, HTTP) | ✅ Native | Hard (POST-based) |
| File upload | ✅ Multipart | Awkward |

**Default**: REST. Use GraphQL only when the client's variable data shape requirements justify the complexity overhead.

---

## URL Conventions

```
# Collections
GET    /api/v1/orders          → list (paginated)
POST   /api/v1/orders          → create

# Single resource
GET    /api/v1/orders/{id}     → read
PUT    /api/v1/orders/{id}     → full replace
PATCH  /api/v1/orders/{id}     → partial update
DELETE /api/v1/orders/{id}     → delete

# Sub-resources
GET    /api/v1/orders/{id}/items           → list items of order
POST   /api/v1/orders/{id}/items           → add item
DELETE /api/v1/orders/{id}/items/{itemId}  → remove item

# Actions (avoid verbs in URLs; prefer sub-resource nouns when possible)
POST   /api/v1/orders/{id}/cancellation    → cancel order
POST   /api/v1/orders/{id}/payment         → pay for order
```

---

## HTTP Status Code Guide

| Code | Use when |
|------|----------|
| `200 OK` | Successful GET, PATCH, PUT |
| `201 Created` | Successful POST that creates a resource; include `Location` header |
| `204 No Content` | Successful DELETE; successful action with no body |
| `400 Bad Request` | Request body fails schema validation |
| `401 Unauthorized` | Missing or invalid authentication credentials |
| `403 Forbidden` | Authenticated but not authorised for this resource |
| `404 Not Found` | Resource does not exist |
| `409 Conflict` | Duplicate idempotency key; conflicting state (e.g. already cancelled) |
| `422 Unprocessable Entity` | Well-formed but fails business rules |
| `429 Too Many Requests` | Rate limit exceeded; include `Retry-After` header |
| `500 Internal Server Error` | Unhandled exception — never leak stack traces |

---

## Error Envelope Standard

All error responses use this shape:
```json
{
  "code": "ORDER_ALREADY_CANCELLED",
  "message": "The order cannot be paid because it is already cancelled.",
  "details": [
    { "field": "status", "issue": "Expected PENDING, got CANCELLED" }
  ]
}
```

- `code`: machine-readable SCREAMING_SNAKE_CASE string — stable across releases
- `message`: human-readable string — may change
- `details`: array of field-level issues for validation errors; empty array `[]` otherwise

---

## Authentication Patterns

### Decision table

| Use case | Pattern |
|----------|---------|
| User-facing web/mobile app | OAuth2 Authorization Code + PKCE → short-lived JWT access token + refresh token |
| Machine-to-machine (M2M) | OAuth2 Client Credentials → short-lived JWT |
| Simple internal service | Static API key in header (`X-Api-Key`) — rotate regularly |
| Session-based (legacy/SSR) | Server-side session with secure, httpOnly, SameSite=Strict cookie |

### JWT access + refresh token pattern

```
1. POST /auth/token { username, password }
   → { accessToken (15 min), refreshToken (7 days) }

2. Client sends: Authorization: Bearer <accessToken> on every request

3. On 401: POST /auth/refresh { refreshToken }
   → { accessToken (new), refreshToken (rotated) }

4. On logout: POST /auth/logout { refreshToken }
   → server blacklists refreshToken; 204 No Content
```

**Rules:**
- Access tokens: short-lived (15 min), stored in memory (not localStorage)
- Refresh tokens: long-lived, stored in httpOnly cookie or secure storage; rotated on every use
- Never store access tokens in localStorage — XSS risk
- Blacklist refresh tokens on logout (Redis or DB table)
- Sign with RS256 (asymmetric) for multi-service environments; HS256 for single-service

### RBAC vs ABAC

| | RBAC | ABAC |
|-|------|------|
| Basis | Role (`admin`, `editor`) | Attributes (owner, plan tier, resource state) |
| Complexity | Low | High |
| Best for | Simple permission hierarchies | Fine-grained, context-dependent rules |
| Example | "Admins can delete any user" | "Users can delete their own resources if status=DRAFT" |

---

## Idempotency Pattern

```
POST /api/v1/orders
Idempotency-Key: <uuid-v4>   ← client-generated, per-request

Server algorithm:
1. Hash key → look up in idempotency_keys table
2. If found AND status=COMPLETE → return cached response (200/201)
3. If found AND status=IN_FLIGHT → return 409 (concurrent request)
4. If not found → insert row (status=IN_FLIGHT), process, update (status=COMPLETE, response=...)
5. Expire keys after 24 h
```

**Implementation rules:**
- Store: `{ key, status, request_hash, response_status, response_body, created_at }`
- Verify request hash matches — reject with 422 if same key + different body
- Use a DB unique constraint on `key` as the concurrency guard
- The idempotency table is infrastructure — never reference it from domain layer

---

## Pagination Patterns

### Cursor-based (preferred for feeds, large datasets)
```json
GET /api/v1/orders?limit=20&cursor=eyJpZCI6IjEyMyJ9

Response:
{
  "data": [...],
  "pagination": {
    "nextCursor": "eyJpZCI6IjE0MyJ9",
    "hasMore": true
  }
}
```
- Cursor encodes the last item's sort key (base64 or opaque string)
- Stable under concurrent inserts — no "page drift"
- Cannot jump to page N directly

### Offset-based (for UI tables with known total)
```json
GET /api/v1/orders?page=2&pageSize=20

Response:
{
  "data": [...],
  "pagination": {
    "total": 347,
    "page": 2,
    "pageSize": 20,
    "totalPages": 18
  }
}
```
- Simple; supports jumping to arbitrary page
- Unstable under concurrent inserts (items can appear twice or be skipped)
- Performance degrades at large offsets (avoid for > 10k rows)

---

## CQRS — When to Use

**Command Query Responsibility Segregation**: separate the write model (commands) from the read model (queries).

| Signal | Use CQRS |
|--------|----------|
| Read and write models differ significantly | ✅ |
| High read/write ratio disparity | ✅ |
| Complex aggregates with simple read projections | ✅ |
| Simple CRUD with symmetrical read/write | ❌ Overkill |
| Small team / early product | ❌ Premature |

**Minimal CQRS (no event sourcing required):**
```
Commands → Domain Aggregate → Write DB (normalized)
                                   ↓ (event triggers projection update)
Queries  → Read Model       → Read DB (denormalized view / materialized view)
```

---

## Testing Pyramid

```
        ╱‾‾‾‾‾‾‾‾‾‾‾╲
       ╱   E2E (5%)   ╲       Slow, expensive, fragile
      ╱─────────────────╲     Full stack: real DB, real HTTP
     ╱  Integration (25%) ╲   API endpoints, DB queries, auth
    ╱─────────────────────╲
   ╱     Unit (70%)        ╲  Fast, isolated, no I/O
  ╱─────────────────────────╲ Domain logic, use cases, validators
```

| Layer | What to test | What to mock |
|-------|-------------|--------------|
| Unit | Domain entities, use cases, validators, mappers | External services, DB, HTTP |
| Integration | API endpoint → DB round-trip; auth middleware; queue publishing | External third-party APIs |
| E2E | Critical user flows through the full stack | Nothing (or external payments only) |

**Coverage targets:**
- Domain layer: ≥ 90% line coverage
- Application layer: ≥ 80%
- Infrastructure / adapters: covered by integration tests

---

## OWASP Top 10 — Backend Checklist

- [ ] **Broken Access Control** — every endpoint has explicit auth + authz check; test with wrong-user token
- [ ] **Cryptographic Failures** — passwords hashed with bcrypt/argon2 (never MD5/SHA1); TLS on all transport
- [ ] **Injection** — use parameterised queries / ORM everywhere; never concatenate user input into SQL
- [ ] **Insecure Design** — domain validates business rules; don't trust client-supplied IDs without ownership check
- [ ] **Security Misconfiguration** — remove debug endpoints in production; disable stack traces in error responses
- [ ] **Vulnerable Components** — `composer audit` / `npm audit` / `pip-audit` in CI; fix high/critical findings
- [ ] **Auth Failures** — rate-limit auth endpoints; lock account after N failures; rotate refresh tokens
- [ ] **Software Integrity** — pin dependency versions; verify checksums in CI
- [ ] **Logging Failures** — log auth events, access control failures, and input validation errors; never log passwords or tokens
- [ ] **SSRF** — validate and allowlist any URL accepted from clients; never fetch user-supplied URLs server-side without validation
