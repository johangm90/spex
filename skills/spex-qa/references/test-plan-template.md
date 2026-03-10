# Test Plan Template — spex-qa Reference

Structure, coverage thresholds, performance baseline, and artifact front-matter for test plans produced by `spex-qa`.

---

## Artifact Front-Matter

Every test plan stored in MCP should begin with this header block:

```
id: PROJ-TEST-NNN
slice: SLICE-NNN
task: T0NN-N
agent: spex-qa
status: draft | validated
created: <ISO timestamp>
updated: <ISO timestamp>
passed_criteria: <integer>
total_criteria: <integer>
security_review: none | checklist | stride
```

---

## Test Plan Structure

### 1. Scope

- Slice being tested: `SLICE-NNN`
- Acceptance criteria under test: list each AC ID
- Test stack: e.g. PHPUnit 11 + WebTestCase + Playwright
- Out of scope: list anything explicitly excluded and why

---

### 2. Happy Path Cases

For every API endpoint or user flow, document at least one happy path case:

| ID | AC | Description | Method + Path | Input | Expected status | Expected body (key fields) | Result |
|----|----|-------------|---------------|-------|-----------------|---------------------------|--------|
| TC-001 | AC-1 | Create resource with valid payload | POST `/api/orders` | `{"product_id":1,"qty":2}` | 201 | `{"id": "<uuid>", "status": "pending"}` | — |
| TC-002 | AC-2 | Retrieve existing resource | GET `/api/orders/{id}` | valid UUID | 200 | `{"id": "...", "status": "pending"}` | — |

---

### 3. Error Paths

Cover at minimum all of the following:

| ID | AC | Description | Input / Precondition | Expected status | Expected error | Result |
|----|----|-------------|----------------------|-----------------|----------------|--------|
| TC-010 | — | Missing required field | omit `product_id` | 400 | `"product_id is required"` | — |
| TC-011 | — | Malformed UUID in path | `GET /api/orders/not-a-uuid` | 400 or 404 | validation error | — |
| TC-012 | — | Unauthenticated request | no `Authorization` header | 401 | `"Unauthorized"` | — |
| TC-013 | — | Insufficient permissions | authenticated as `ROLE_USER`, accesses admin endpoint | 403 | `"Forbidden"` | — |
| TC-014 | — | Resource not found | valid UUID that does not exist | 404 | `"Not Found"` | — |
| TC-015 | — | Duplicate / idempotency | submit same unique payload twice | 409 or 200 (idempotent) | no duplicate created | — |
| TC-016 | — | Payload too large | body exceeds `content_max_length` | 413 | `"Payload Too Large"` | — |

---

### 4. Edge Cases

Document domain-specific edge cases. Common categories:

| ID | Description | Setup / Precondition | Expected Behaviour | Result |
|----|-------------|----------------------|--------------------|--------|
| TC-020 | Concurrent update conflict | Two simultaneous PATCH requests on the same resource | One wins; other gets 409 or 200 with merged state | — |
| TC-021 | Empty collection | No items match filter | 200 with `{"data": [], "total": 0}` | — |
| TC-022 | Maximum page size | `?limit=1000` | Capped at configured max; no 500 | — |
| TC-023 | Boundary value — minimum | `qty=1` (minimum allowed) | 201 success | — |
| TC-024 | Boundary value — maximum | `qty=<max+1>` | 400 validation error | — |
| TC-025 | Fiscal/temporal boundary | Request at 23:59:59 UTC on last day of month | Correct period assignment | — |
| TC-026 | Soft-deleted resource | Access resource after soft delete | 404 (not leaked) | — |

---

### 5. Security Cases

Minimum coverage — one test per vector. Full mapping in `references/security-review.md`.

| ID | OWASP ref | Description | Attack Vector | Expected Defence | Result |
|----|-----------|-------------|---------------|------------------|--------|
| TC-030 | A03 | SQL injection in query param | `?search=1' OR '1'='1` | Parameterized query; 400 or safe empty result | — |
| TC-031 | A01 | IDOR — access another user's resource | Use own valid token, target another user's ID | 403 or 404 | — |
| TC-032 | A07 | Auth bypass — missing token | Remove `Authorization` header | 401 | — |
| TC-033 | A03 | XSS payload in text field | `<script>alert(1)</script>` | Stored escaped; not executed | — |
| TC-034 | A05 | Mass assignment — inject forbidden field | Include `role: "admin"` in create payload | Field ignored; role unchanged | — |
| TC-035 | A09 | Sensitive data in error response | Trigger a 500 | No stack trace, DB details, or secrets in body | — |

---

### 6. Performance Baseline

Record even if no explicit SLO is defined — used as baseline for future regression detection.

| Metric | SLO (if set) | Measured | Pass / Fail |
|--------|-------------|----------|-------------|
| p50 response time | — | — ms | — |
| p95 response time | ≤ 200 ms | — ms | — |
| p99 response time | — | — ms | — |
| Throughput | ≥ 100 req/s | — req/s | — |
| Error rate under load | < 0.1% | —% | — |

---

## Contract Testing (cross-service boundaries)

When the slice exposes or consumes an API that is shared with another service, add a contract test:

```
Consumer: <service name>
Provider: <service name>
Contract tool: Pact
Pact file location: pacts/<consumer>-<provider>.json
```

| ID | Interaction | Consumer expectation | Provider verification | Result |
|----|-------------|----------------------|-----------------------|--------|
| CT-001 | `GET /api/users/{id}` | Returns `{id, email, role}` | Provider test confirms shape | — |

Contract tests must pass **before** `QASignOff` on any slice that crosses a service boundary.

---

## Mutation Testing

Run mutation testing when:
- The slice contains critical business logic (pricing, auth, calculations)
- Unit test line coverage is at or near 80% but confidence is low
- A bug was found in QA that unit tests missed (regression signal)

| Stack | Tool | Command |
|---|---|---|
| PHP | Infection PHP | `vendor/bin/infection --min-msi=70 --min-covered-msi=80` |
| JS/TS | Stryker | `npx stryker run` |

**Mutation Score Indicator (MSI) targets:**

| Layer | Minimum MSI |
|---|---|
| Domain / business logic | 70% |
| Auth / security logic | 80% |

---

## Coverage Thresholds

| Layer | Minimum coverage |
|---|---|
| Domain / business logic (unit) | 80% line coverage |
| API handlers (integration) | All happy + error paths exercised |
| E2E flows | Primary user flow + at least 2 error paths |

Coverage is a **floor**, not a target. Do not stop at 80% if important paths remain untested.

---

## MCP Storage Pattern

After completing the test plan, store it in MCP:

```js
artifact_register(id="PROJ-TEST-NNN", spec="SLICE-NNN", task="T0NN-N",
  agent="spex-qa", type="test_plan", path="mcp:test_plans/PROJ-TEST-NNN")

memory_set(agent="spex-qa", key="artifact_PROJ-TEST-NNN", value=<this document as JSON string>)
```
