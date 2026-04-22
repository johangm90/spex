# SPEC-004: Enterprise Execution Model con Auditoría e Integración

**Status:** Draft  
**Priority:** P1  
**Dependencies:** SPEC-002, SPEC-003  
**Created:** 2026-04-22  

---

## Overview

spex has a solid control plane for specs, tasks, policy gates, and evidence — but it lacks the execution-layer visibility that teams need to operate it at scale. There is no way to know *when* an agent session ran, *who* made a given state change, or *what happened* across a spec's full lifecycle in one view. State changes are attributable only optionally, and there is no integration surface for external systems (CI, Slack, GitHub) to react to spex events.

This spec adds four capabilities that together form an enterprise-grade execution model:

1. **Execution sessions** — lightweight records that capture when an agent or human session starts and ends, which spec/task it targeted, and which host it used. Sessions provide the "outer envelope" for all work done in a sitting.

2. **Mandatory attribution** — every state-mutating operation (task status change, spec status change) must carry an `updated_by` actor. Currently this field is optional and inconsistently populated; this spec makes it a hard enforcement point in the shared workflow.

3. **Enriched trace view** — the existing `spex trace` command shows only domain events. This spec extends it to show evidence submissions, approvals, artifacts, and memory writes in a single chronological timeline, scoped to a spec or task.

4. **Webhook notifications** — when key transitions occur (task done, spec approved, approval requested), spex fires an HTTP POST to a configured webhook URL. This is opt-in, fail-graceful, and requires no external dependencies at startup.

5. **Workspace aggregation** — a `spex workspace status` command that reads from multiple `.spex/state.db` files and shows a cross-project summary. Read-only in v1.

---

## Acceptance Criteria

1. **[Sessions - start]** `spex session start --spec <id> --agent <name> [--host <host>]` inserts a session row with a unique ID, `started_at` timestamp, and emits a `SessionStarted` domain event. The session ID is printed to stdout.

2. **[Sessions - end]** `spex session end <session-id>` sets `ended_at`, computes duration, and emits a `SessionEnded` event with duration in seconds.

3. **[Sessions - list]** `spex sessions [--spec <id>] [--agent <name>] [--active]` prints a table of sessions with ID, agent, spec, host, started_at, duration (or "active").

4. **[Attribution - tasks]** `workflow::complete_task` and `workflow::start_task` called without a non-empty `updated_by` return `Err` and make no DB mutation. Existing callers that pass `updated_by` are unaffected.

5. **[Attribution - specs]** `workflow::approve_spec`, `workflow::start_spec`, and `workflow::complete_spec` called without a non-empty `updated_by` return `Err` and make no DB mutation.

6. **[Trace - full spec]** `spex trace <spec-id> --full` outputs a chronological timeline that includes: domain events, evidence bundle submissions, approval requests/decisions, artifact registrations, and memory writes — all interleaved by timestamp.

7. **[Trace - task scope]** `spex trace --task <task-id>` scopes the full timeline to a single task (events, evidence, approvals, artifacts, memory entries referencing that task).

8. **[Webhooks - fired]** When a webhook URL is configured in `.spex/config.toml` under `[webhooks]`, spex fires an HTTP POST with a JSON payload on: `TaskDone`, `SpecApproved`, `SpecDone`, `ApprovalRequested`.

9. **[Webhooks - fail-graceful]** If the HTTP POST fails (timeout, non-2xx, DNS error), spex prints a warning to stderr and continues normally. The triggering operation is not rolled back.

10. **[Webhooks - no-op when unconfigured]** If `.spex/config.toml` does not exist or has no `[webhooks]` section, no HTTP calls are made and no errors are emitted.

11. **[Workspace - status]** `spex workspace status --paths <path1> [<path2> ...]` reads each `.spex/state.db` and prints a summary table: project path, open specs, open tasks, last activity timestamp.

12. **[Workspace - read-only]** Any write operation attempted via `spex workspace` (e.g. `spex workspace task done`) returns a clear error: `workspace commands are read-only in v1`.

---

## Out of Scope for v1

- Multi-approver / delegated approval workflows (deferred to SPEC-006)
- Streaming webhook delivery with retries and dead-letter queues
- Cross-project task dependencies or spec linking
- Authentication on webhook endpoints (HMAC signing deferred)
- `spex workspace` write operations
- Session-level resource usage tracking (tokens, cost)
- Real-time event streaming (SSE / WebSocket)

---

## Open Questions

1. **Attribution grace period** — Should `updated_by` enforcement be warn-only for one release cycle before becoming a hard error? This is a breaking change for any agent that calls workflow functions without the field. Recommendation: hard-error immediately since all bundled agents already pass the field.

2. **Session ID format** — `ulid` (sortable, URL-safe) vs `uuid-v4`? spex currently uses plain string IDs. Recommendation: use `format!("sess-{}", Utc::now().timestamp_nanos())` for simplicity, consistent with existing ID patterns.

3. **Config file location** — `.spex/config.toml` (file-based, gitignore-able) vs a `meta` table in the DB (portable with the DB). Recommendation: `.spex/config.toml` — keeps secrets out of the DB and is easy to gitignore.

4. **Workspace discovery** — Walk up the directory tree looking for `.spex/` dirs vs explicit `--paths` flag. Recommendation: explicit `--paths` in v1 for predictability.

5. **`reqwest` dependency** — Confirm `reqwest` is not already a transitive dep before adding. If it is, use it; otherwise add `reqwest` with `default-features = false, features = ["json", "blocking"]` or use `ureq` for a lighter footprint.

---

## Proposed Task Breakdown (T034–T045)

| ID | Type | Title |
|----|------|-------|
| T034 | SCHEMA | Add `sessions` table migration |
| T035 | API | Implement session start/end domain functions and events |
| T036 | CLI | Add `spex session start/end/list` commands |
| T037 | API | Enforce mandatory `updated_by` in workflow::complete_task and workflow::start_task |
| T038 | API | Enforce mandatory `updated_by` in workflow::approve_spec, start_spec, complete_spec |
| T039 | CLI | Extend `spex trace` with evidence, approvals, artifacts, memory in unified timeline |
| T040 | SCHEMA | Add `.spex/config.toml` reader with `[webhooks]` section support |
| T041 | API | Implement webhook dispatcher (fire-and-forget, fail-graceful) |
| T042 | API | Wire webhook dispatcher into workflow transitions (TaskDone, SpecApproved, SpecDone, ApprovalRequested) |
| T043 | CLI | Add `spex workspace status --paths` command |
| T044 | TEST | Domain and integration tests for sessions, attribution enforcement, trace, webhooks |
| T045 | DOCS | ADR documenting session model, attribution policy, webhook design, workspace scope |
