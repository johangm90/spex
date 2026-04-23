# ADR: Agent evals and scorecards (SPEC-005)

**Status**: Accepted
**Date**: 2026-04-22
**Deciders**: spex-architect, sdd-builder, adr-writer
**Related Specs**: SPEC-005
**Supersedes**: —

## Context

Before SPEC-005, `spex` could tell whether work had been completed, whether evidence existed, and whether policy gates had been satisfied, but it still could not answer a separate and important question: **how good was the result?**

That gap mattered in three places:

- operators could not persist a normalized quality judgment alongside tasks, specs, artifacts, and audit events
- agents and humans had no first-class way to compare one judged result against a prior baseline
- evaluation signals remained buried in prose, external tools, or ad hoc reviewer notes rather than in the local control plane

With SPEC-002, SPEC-003, and SPEC-004 already in place, `spex` had the transactional workflow, policy/evidence model, and attribution/session context needed to support a durable eval layer. The missing piece was a local-first, append-only eval system that reused the same SQLite state model and was inspectable from both CLI and MCP.

## Decision

Adopt a first-class eval model with four linked parts:

1. **Append-only eval run records**
   - Store eval runs in SQLite with stable IDs, evaluator identity, target scope, outcome, rationale, optional summary, metadata, and resolved scope columns.
   - Keep evals append-only rather than mutable so audit history is preserved.

2. **Structured scorecards with normalized dimensions**
   - Represent quality judgments as explicit scorecard dimensions rather than only freeform prose.
   - Normalize v1 dimensions to:
     - `correctness`
     - `validation_coverage`
     - `policy_compliance`
     - `risk`
   - Normalize statuses to:
     - `pass`
     - `warn`
     - `fail`
     - `not_applicable`
     - `unknown`
   - Allow optional numeric scores and structured JSON details per dimension.

3. **Comparison and baseline support**
   - Support explicit eval-to-eval comparison and a convenience latest-vs-baseline mode for the same logical scope.
   - Return per-dimension deltas plus an overall classification of:
     - `improved`
     - `regressed`
     - `unchanged`

4. **Operator and agent surfaces with audit integration**
   - Expose eval create/list/show/compare flows in the CLI.
   - Expose matching create/list/get/compare/latest-baseline flows via MCP tools.
   - Emit `EvalCreated` and `EvalCompared` domain events so eval activity appears in the audit log.

## Rationale

This design fits `spex`'s existing architecture better than external judge services or freeform-only notes.

- **Local-first**: evals live in the same project-local SQLite database as specs, tasks, policy, evidence, sessions, and artifacts.
- **Auditable**: append-only records and emitted events preserve an evaluation trail without overwriting prior judgments.
- **Comparable**: normalized dimensions make baseline/regression analysis possible without NLP over prose.
- **Compatible**: repositories with no eval data continue to function unchanged.
- **Shared surfaces**: humans and MCP agents can use the same domain model through CLI and MCP without direct DB access.

## Consequences

### Positive

- Teams can persist explicit quality judgments instead of inferring quality only from task completion
- Baseline comparisons make regressions and improvements visible over time
- Audit trails now include evaluation activity, not only workflow transitions and evidence
- CLI and MCP users have consistent access to the same eval semantics

### Negative

- The domain model becomes broader: eval runs, dimensions, links, and comparisons add more concepts to maintain
- Operators must still create evals explicitly; v1 does not provide an autonomous judge or scheduled benchmark runner
- Score normalization is intentionally opinionated in v1 and may need project-specific extensions later

### Risks

- Users may treat eval scores as objective truth when they still depend on evaluator judgment and local context
- Large, long-lived repositories may accumulate many eval rows over time; retention is deferred
- Future teams may want policy rules tied to score thresholds, but that is deliberately not part of this slice

## Implementation notes

- Eval storage is introduced by `migrations/20260422160000_evals.sql`.
- Scope resolution is transactional and fail-safe: eval creation rejects missing specs/tasks/artifacts and does not partially persist records.
- Provenance links support references to evidence bundles, validation runs, sessions, events, artifacts, approvals, specs, tasks, prior eval runs, or custom references.
- Latest-baseline comparison is inferred from the latest earlier eval for the same `target_kind` + `target_ref` pair.
- CLI surfaces live in `src/cli/eval.rs`; MCP surfaces live in `src/mcp/tools/evals.rs`.
- `EvalCreated` and `EvalCompared` are emitted from `src/sdd/evals.rs` with trace-friendly payloads.
- Integration coverage now exercises no-eval backward compatibility plus domain, CLI, and MCP eval flows.

## References

- `docs/specs/SPEC-005-sistema-de-evals-y-scorecards.md`
- `migrations/20260422160000_evals.sql`
- `src/sdd/evals.rs`
- `src/cli/eval.rs`
- `src/mcp/tools/evals.rs`
- `tests/evals_integration.rs`
