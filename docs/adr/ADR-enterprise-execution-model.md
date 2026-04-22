# ADR: Enterprise execution model (SPEC-004)

**Status**: Accepted
**Date**: 2026-04-22
**Deciders**: spex-architect, sdd-builder, adr-writer
**Related Specs**: SPEC-004
**Supersedes**: —

## Context

`spex` already had specs, tasks, policy gates, and audit events, but it still lacked four pieces needed for enterprise operation:

- explicit agent/human session tracking
- stricter attribution on lifecycle transitions
- an outbound integration surface for workflow notifications
- a higher-level operational view across workspaces and timelines

Without those pieces, operators can see state changes, but they cannot reliably answer who was actively working, which host initiated the work, how to consume lifecycle changes externally, or how to reconstruct a unified execution timeline.

## Decision

Adopt an enterprise execution model with four linked capabilities:

1. **Session model in SQLite**
   - Add a first-class `sessions` table with start/end timestamps, optional spec/task scope, host, notes, and duration.
   - Expose it through domain functions, MCP tools, and CLI commands.

2. **Mandatory lifecycle attribution**
   - Require non-empty `updated_by` for spec and task completion/start/approval transitions that materially change execution state.
   - Reject blank or whitespace-only attribution at the workflow boundary.

3. **Fail-graceful outbound webhooks**
   - Load optional webhook configuration from `.spex/config.toml`.
   - Fire `TaskDone`, `SpecApproved`, `SpecDone`, and `ApprovalRequested` notifications after successful workflow transitions.
   - Never let webhook delivery failure block the state transition.

4. **Operator-facing session and trace surfaces**
   - Add `spex session start|end|list`.
   - Extend `spex trace` with `--task` scoping and `--full` unified timeline output.
   - Add `spex workspace status --paths` for read-only multi-workspace visibility.

## Rationale

This keeps the core model local-first and SQLite-backed while improving auditability and operator ergonomics.

- Sessions make active work explicit instead of inferred.
- Mandatory attribution closes a governance gap where important transitions could be recorded without a meaningful actor.
- Webhooks enable external observability without introducing a hard dependency on an external control plane.
- Unified trace/session/workspace views make operational debugging faster without changing the underlying domain model.

## Consequences

### Positive

- Better reconstruction of who did what, where, and when
- Easier external integrations with chatops, incident systems, or dashboards
- Stronger audit posture for spec/task lifecycle changes
- Cleaner CLI and MCP support for real operator workflows

### Negative

- More workflow call sites now need to pass attribution and, for CLI flows, loaded config
- Session data adds another audited entity to maintain and test
- Webhook delivery is best-effort only in v1 (no retries or dead-letter handling)

### Risks

- Operators may assume webhook delivery is guaranteed when it is intentionally fail-graceful
- Trace `--full` is useful but still derived from heterogeneous records rather than a single canonical timeline table
- Session start/end discipline depends on callers using the commands/tools consistently

## Implementation notes

- Sessions are emitted as normal domain events (`SessionStarted`, `SessionEnded`) in addition to being stored structurally.
- Webhook config stays optional; absence of `.spex/config.toml` preserves current behavior.
- MCP callers currently pass `None` for config, keeping the runtime boundary simple.
- CLI callers load config from project root and pass it into workflow operations.
- Integration coverage now exercises session CLI flow, trace `--full --task`, attribution rejection, and webhook failure tolerance.

## References

- `migrations/20260422120000_sessions.sql`
- `src/sdd/sessions.rs`
- `src/cli/session.rs`
- `src/cli/trace.rs`
- `src/webhooks.rs`
- `src/config.rs`
- `src/sdd/workflow.rs`
- `src/cli/workspace.rs`
- `tests/spec004_integration.rs`
