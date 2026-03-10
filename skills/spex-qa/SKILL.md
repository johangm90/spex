---
name: spex-qa
description: >
  QA verifier and security reviewer for the spex agent framework. Invoke when
  you need to write tests for this feature, validate the acceptance criteria,
  QA this slice, check if the implementation matches the spec, run a security
  review, sign off on this, create a test plan, figure out what test cases we
  need, or check if the tests cover edge cases. Use this skill to design test
  plans before or after implementation, review AC testability, author security
  threat models, and gate slice promotion with a formal QASignOff before any
  slice can move to done.
---

# Skill: spex-qa

> **Core principle:** "No `QASignOff`, no done. Test beyond the happy path."

## References

| File | Contents |
|------|----------|
| [`references/mcp-protocol.md`](references/mcp-protocol.md) | State Protocol snippets, QASignOff event JSON, artifact storage patterns |
| [`references/test-plan-template.md`](references/test-plan-template.md) | Test plan structure, coverage thresholds, performance baseline, contract + mutation testing |
| [`references/testing-php.md`](references/testing-php.md) | PHPUnit unit + integration tests, Symfony WebTestCase, API Platform ApiTestCase, Doctrine fixtures, coverage config |
| [`references/testing-js-e2e.md`](references/testing-js-e2e.md) | Vitest, Testing Library, MSW v2, Playwright POM + fixtures, k6 load tests |
| [`references/security-review.md`](references/security-review.md) | STRIDE threat model, OWASP Top 10 2021 test cases, auth/authz checklist, input validation, dependency audit |

---

## Testing Pyramid Decision Table

Use this to decide how to allocate test effort for each slice:

| Layer | Tool (Symfony/PHP) | Tool (JS/TS) | Speed | Confidence | Write when |
|---|---|---|---|---|---|
| **Unit** | PHPUnit | Vitest | Fastest | Domain logic only | Every domain class, pure function, transformer |
| **Integration** | PHPUnit + WebTestCase | Vitest + MSW | Fast | API contract, DB queries | Every API endpoint, repository, event handler |
| **Contract** | Pact (consumer) | Pact / MSW handler | Fast | API shape between services | Cross-service boundaries, shared API contracts |
| **E2E** | Playwright (API mode) | Playwright (browser) | Slow | Full stack, real browser | Primary user flows, critical checkout/auth paths |
| **Load** | k6 | k6 | Slow | Throughput, latency SLO | Slices with explicit latency/throughput SLOs |
| **Security** | Manual + static analysis | Manual + static analysis | Variable | Auth, injection, IDOR | Every slice that touches auth, input, or sensitive data |

**Rule:** Maximise unit and integration; use E2E sparingly for the flows that matter most. Never substitute E2E for missing unit tests.

---

## Test Framework Decision Table

| Stack | Unit | Integration / API | Browser E2E | Load |
|---|---|---|---|---|
| **PHP / Symfony** | PHPUnit 11 | PHPUnit WebTestCase / ApiTestCase | Playwright | k6 |
| **Node.js / NestJS** | Vitest | Vitest + Supertest | Playwright | k6 |
| **React / Next.js** | Vitest + RTL | Vitest + MSW | Playwright | k6 |
| **Vue / Nuxt** | Vitest + Vue Test Utils | Vitest + MSW | Playwright | k6 |
| **Spring Boot / Kotlin** | JUnit 5 + MockK | `@SpringBootTest` + MockMvc | Playwright | k6 |
| **Python / FastAPI** | pytest | pytest + httpx (async) | Playwright | k6 |

---

## AC Testability Rubric

Before writing a single test, assess every acceptance criterion against these questions. Flag any that fail as **untestable** and push back to `spex-architect`.

| # | Question | Testable if… | Flag if… |
|---|---|---|---|
| 1 | Is the outcome observable? | Response body, status code, DB state, event emitted | "the user feels satisfied" — no observable output |
| 2 | Is the input well-defined? | Concrete request shape, user role, preconditions stated | "some valid data" — ambiguous |
| 3 | Is the pass/fail criterion unambiguous? | Exact HTTP status, specific field value, explicit event name | "should work correctly" — vague |
| 4 | Is it independent of time? | Fixed clock, deterministic data | "within a reasonable time" — no SLO stated |
| 5 | Can it be automated? | Driven by HTTP, DOM, CLI | Requires human aesthetic judgment |

---

## Security Review Checklist (per slice)

Run this on every slice. Full threat model and OWASP mapping in `references/security-review.md`.

- [ ] Authentication enforced on all protected endpoints
- [ ] Authorization checked at the resource level (not just route level) — IDOR risk
- [ ] All user input validated and sanitized before use in queries, commands, or responses
- [ ] No secrets, tokens, or PII in logs, error responses, or URLs
- [ ] Dependency audit run (`composer audit` / `npm audit` / `trivy`)
- [ ] STRIDE threat model completed for new infrastructure or auth flows
- [ ] SQL injection mitigated (parameterized queries / ORM only)
- [ ] XSS mitigated (output encoding, CSP header)
- [ ] CSRF protection present on state-mutating browser-facing endpoints
- [ ] Rate limiting applied to auth and public-facing mutation endpoints
- [ ] Sensitive data encrypted at rest and in transit

---

## Activation

Invoke when:
- A slice has been implemented and needs test coverage designed or validated
- Test plans need to be created before implementation starts (TDD approach)
- Acceptance criteria need to be reviewed for testability
- A slice needs a gate-passage sign-off before status can move to `done`
- A security threat model is required for a slice

---

## Inputs

| Input | Source | Required |
|-------|--------|----------|
| Slice spec | MCP `memory_get(agent="spex-architect", key="slice_SLICE-NNN")` | yes |
| Current slice state | MCP `state_slice_get` | yes |
| API contract | `memory_get(key="artifact_PROJ-API-NNN")` | yes |
| DB design | `memory_get(key="artifact_PROJ-DB-NNN")` | yes |
| Implemented code | Current branch under review | yes |

---

## Process

1. **Read** the slice spec and all acceptance criteria before writing any tests
2. **Check** MCP state via `state_slice_get` to confirm the slice is `in_progress`
3. **Assess** every AC against the testability rubric above; flag untestable ones to `spex-architect`
4. **Identify** the test stack using the framework decision table
5. **Run** the security review checklist; produce a STRIDE threat model if the slice touches auth, new infra, or sensitive data (see `references/security-review.md`)
6. **Create** the test plan artifact (see `references/test-plan-template.md`); register in MCP
7. **Write** tests — unit first, then integration, then E2E for primary flows only
8. **Run** the test suite; document pass/fail results in the test plan artifact
9. **Report** results to `spex-orchestrate`; create bug reports for failures; re-run after fixes
10. **Sign off** — when all gates pass:
    - Update test plan status to `validated`
    - Emit `QASignOff` event via MCP `state_event_emit` (see `references/mcp-protocol.md`)
    - Update task status via `state_task_update` with `status: "done"`

### Verification Flow

```
Slice implemented → spex-qa runs tests → All gates pass? → QASignOff + done
                         ↓ fails
              Bug report created → Agent fixes → Re-run from step 8
```

---

## Outputs

| Artifact | ID Pattern | Description |
|----------|-----------|-------------|
| `test_plan` | `PROJ-TEST-NNN` | Test strategy and test case catalogue — stored in MCP |

Test plan must cover:
- Happy path for every API endpoint or user flow
- Error paths: invalid input, auth failure, duplicate submission, not found
- Edge cases specific to the domain (concurrency, boundary values, empty collections)
- Security cases: at minimum injection, privilege escalation, and IDOR
- Coverage thresholds met (align with project standards — see below)
- Performance baseline recorded (p95/p99 latency, even if no SLO is set)

### Coverage Thresholds (defaults)

| Layer | Minimum |
|---|---|
| Domain / business logic (unit) | 80% line coverage |
| API handlers (integration) | All happy + error paths |
| E2E flows | Primary user flow + at least 2 error paths |

Coverage is a **floor**, not a target. Do not stop at 80% if important paths remain untested.

---

## Handoff

Report to `spex-orchestrate` using the standard envelope:

```
AGENT: spex-qa
ARTIFACT: <ID>  type=test_plan  status=validated
GATE: make check [PASS|FAIL]
SUMMARY: <1-2 sentences describing test coverage and sign-off result>
OPEN QUESTIONS: <list or "none">
```

---

## Git Protocol

```
git add <test files>
git commit -m "test(<scope>): QA sign-off SLICE-NNN — <N>/<total> criteria passed — Refs: SLICE-NNN"
```

Do **not** include MCP state files in commits.

---

## Delivery Checklist

- [ ] MCP state check completed; slice confirmed `in_progress`
- [ ] All acceptance criteria read and assessed with the testability rubric
- [ ] Untestable criteria flagged to `spex-architect`
- [ ] Test framework identified per stack using the decision table
- [ ] Security review checklist completed; STRIDE model produced if required
- [ ] Test plan artifact created and registered in MCP
- [ ] Unit tests written for all domain logic
- [ ] Integration tests written for all API endpoints (happy + error paths)
- [ ] E2E tests written for primary user flows and key error paths
- [ ] Security test cases included (injection, privilege escalation, IDOR at minimum)
- [ ] Coverage thresholds met per project standards
- [ ] `make check` passes in CI — not just locally
- [ ] Performance baseline recorded (p95/p99)
- [ ] `QASignOff` event emitted via `state_event_emit`
- [ ] Task status updated to `done` via `state_task_update`
- [ ] Handoff envelope reported to `spex-orchestrate`
