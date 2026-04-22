# ADR-002: Hardened Control Plane Rollout and Compatibility Contract

**Status**: Accepted
**Date**: 2026-04-21
**Deciders**: adr-writer, sdd-builder, spex-architect
**Related Specs**: SPEC-002
**Supersedes**: ADR-001 (control-plane lifecycle details only)

## Context

`spex` now has a hardened control plane after SPEC-002 tasks T015-T019. Before this work, lifecycle mutations were enforced in multiple entrypoints, which made it easier for CLI, MCP, or legacy mutation paths to drift in semantics, event emission, and invariant handling.

The implemented state now centralizes lifecycle behavior in `src/sdd/workflow.rs`, routes CLI and MCP mutation paths through that shared workflow, modularizes MCP tools under `src/mcp/tools/`, and adds `doctor` checks for control-plane drift. This ADR records the rollout contract for operators, agents, and compatibility callers.

The decision must preserve `spex`'s local-first model, keep human operators in control of rollouts and remediation, and avoid silently accepting invalid lifecycle changes that would weaken auditability.

## Decision Drivers

- Must provide one authoritative lifecycle path for CLI, MCP, and compatibility callers
- Must guarantee atomic state + event persistence for lifecycle mutations
- Must prevent specs from reaching `done` when task/AC invariants are not satisfied
- Must preserve existing public mutation entrypoints where reasonable without allowing semantic bypasses
- Must keep the product local-first, operator-readable, and human-gated

## Considered Options

1. **Workflow-owned mutations with compatibility wrappers** — centralize lifecycle rules and route legacy callers through guarded wrappers
2. **Keep separate mutation logic per surface** — preserve current CLI/MCP/legacy implementations with incremental fixes
3. **Big-bang API break** — remove legacy mutation routes immediately and require all callers to adopt new workflow entrypoints

## Decision Outcome

**Chosen option**: **Workflow-owned mutations with compatibility wrappers**

**Rationale**: This matches the implemented system and best satisfies the decision drivers. A shared workflow layer gives one source of truth for transitions, spec-done gates, and transactional event persistence. Compatibility wrappers preserve existing public routes where they still map to supported semantics, while rejecting unsupported legacy transitions instead of silently bypassing workflow rules. This is safer than keeping duplicated logic and less disruptive than a big-bang API break.

## Pros and Cons of the Options

### Option A: Workflow-owned mutations with compatibility wrappers

**Pros**:
- Keeps lifecycle rules, invariant checks, and event emission aligned across CLI, MCP, and compatibility callers
- Ensures lifecycle state changes and lifecycle events commit atomically or roll back together

**Cons**:
- Adds a compatibility layer that must be maintained until callers fully converge on canonical workflow paths
- Some previously tolerated legacy transitions now fail explicitly

---

### Option B: Keep separate mutation logic per surface

**Pros**:
- Lowest short-term code churn for existing entrypoints
- Avoids wrapper translation logic

**Cons**:
- Continues semantic drift risk across CLI, MCP, and direct state update routes
- Makes invariant and audit guarantees hard to trust globally

---

### Option C: Big-bang API break

**Pros**:
- Simplest long-term surface area
- Removes ambiguity about supported mutation routes immediately

**Cons**:
- Breaks compatibility for existing callers without a guided migration path
- Conflicts with the goal of a controlled, human-gated rollout

## Consequences

### Positive
- Lifecycle mutations now provide transactional guarantees for status changes and corresponding lifecycle events
- Spec completion is gated by workflow-owned invariants: defined ACs, all ACs passed, and no open tasks
- CLI and MCP lifecycle writes now share the same semantics
- `doctor` can surface control-plane drift with affected IDs, improving operator visibility without auto-mutating state
- Legacy public spec/task status update APIs remain available when they map to supported workflow transitions

### Negative
- Invalid or unsupported legacy transition routes are now rejected instead of being allowed through ad hoc behavior
- Operators may see stricter failures during rollout where old data or callers depended on weaker guarantees

### Risks
- Existing external callers may rely on unsupported legacy transitions — Mitigation: keep compatibility wrappers for supported routes and fail fast with explicit errors for unsupported ones
- Pre-existing state drift may block trust in the new guarantees until repaired — Mitigation: use `spex doctor` to identify affected spec/task/event IDs before or during rollout
- Validation noise during rollout can obscure signal — Mitigation: treat the observed `mcp::server::tests::state_snapshot_includes_subprojects_summary` failure as an unrelated test caveat unless reproduced against control-plane paths

## Implementation Notes

Rollout strategy:

- Treat `src/sdd/workflow.rs` as the only authority for lifecycle transitions, done-gate evaluation, and transactional status/event writes
- Route CLI and MCP lifecycle mutations through workflow entrypoints only; do not add new mutation paths that write status directly
- Preserve legacy public status update APIs only as workflow-owned compatibility wrappers
- Reject unsupported legacy transitions rather than translating them into non-canonical behavior
- Use `spex doctor` as the operator-facing verification step for rollout readiness and post-change audits; findings should be reviewed and repaired by humans, not auto-fixed silently

Compatibility expectations:

- Existing CLI semantics remain stable from an operator perspective, but now inherit stricter invariant enforcement
- Existing MCP lifecycle mutation tools keep canonical contracts while using shared workflow semantics internally
- Legacy wrapper behavior is compatibility-preserving only for transitions with canonical workflow equivalents (`SpecApproved`, `SpecStarted`, `SpecPaused`, `SpecResumed`, `SpecCompleted`, `TaskStarted`, `TaskCompleted`, `TaskFailed`, `TaskReplanned`)
- Callers must not assume that direct status updates can bypass workflow checks; that behavior is no longer supported

Guarantees provided by the hardened control plane:

- No partial persistence for lifecycle mutations: status and lifecycle event succeed together or fail together
- Spec `done` is human-gated by actual project state, not by status mutation intent alone
- Lifecycle drift is diagnosable locally from repository state and SQLite data, consistent with `spex`'s local-first model

## References

- ADR-001 — Core architecture and local-first system constraints
- `src/sdd/workflow.rs` — Shared lifecycle workflow, transactional mutation helpers, and compatibility wrappers
- `src/doctor/mod.rs` — Control-plane invariant checks and operator-facing diagnostics
