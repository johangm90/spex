# ADR-003: Policy Engine and Evidence-Based Execution Gates (SPEC-003)

**Status**: Accepted
**Date**: 2026-04-22
**Deciders**: adr-writer, sdd-builder, spex-architect
**Related Specs**: SPEC-003
**Supersedes**: —

## Context

`spex` agents can complete tasks and specs by calling `complete_task` or `complete_spec` without any verification that the work was actually done correctly. Before SPEC-003, the lifecycle workflow enforced structural invariants (all ACs passed, no open tasks) but had no mechanism to require that agents produce evidence of correctness — test results, lint output, build artifacts, or human review — before marking work done.

This creates a governance gap: an agent can silently declare success, emit a `TaskCompleted` event, and advance the spec without any operator-visible proof that the implementation is sound. For teams operating `spex` in regulated or high-stakes environments, this is unacceptable.

The policy engine closes this gap by introducing evidence bundles, risky-operation evaluation, and an approval workflow that can gate task and spec completion. The design must preserve `spex`'s local-first model, keep humans in control of approval decisions, and avoid blocking velocity for teams that do not need governance.

## Decision Drivers

- Agents must not be able to silently mark work done without verifiable evidence
- Operators must be able to configure governance per-spec or per-task without touching code
- Policy enforcement must be opt-in; teams that do not configure policies must not be affected
- The system must remain local-first and SQLite-only — no external policy service
- Approval decisions must be human-gated; agents may request approval but cannot grant it
- All policy evaluations and approval events must be auditable from the event log
- v1 scope must be deliverable without auto-run validation or multi-approver workflows

## Considered Options

1. **Evidence bundles + policy engine with fail-closed gates** — agents register evidence manually; policy evaluation blocks completion when evidence or approval is missing
2. **Auto-run validation commands** — the workflow layer runs configured validation commands and uses their exit codes as evidence
3. **Policy-as-code files** — policy rules are stored in TOML/YAML files in the repository rather than in SQLite

## Decision Outcome

**Chosen option**: **Evidence bundles + policy engine with fail-closed gates**

**Rationale**: This matches the implemented system and best satisfies the decision drivers. Evidence is registered manually by agents via MCP or CLI, which keeps the system environment-agnostic and avoids the complexity of sandboxed command execution. Policy configs live in SQLite alongside all other project state, consistent with `spex`'s local-first model. Fail-closed evaluation on approved specs ensures that governance cannot be bypassed by omission. Auto-run validation and policy-as-code are deferred to v2 as incremental improvements rather than prerequisites.

## Pros and Cons of the Options

### Option A: Evidence bundles + policy engine with fail-closed gates

**Pros**:
- Environment-agnostic: agents register evidence regardless of how validation was run
- Policy configs stored in SQLite — no additional file format, tooling, or sync required
- Fail-closed on approved specs prevents silent bypass by omission
- Approval workflow is human-gated by design; agents cannot self-approve

**Cons**:
- Evidence registration is manual; agents must explicitly call `register_evidence` before completing work
- No automated verification that registered evidence is genuine (deferred to v2)

---

### Option B: Auto-run validation commands

**Pros**:
- Evidence is produced automatically; agents cannot forget to register it
- Exit codes provide objective pass/fail signal

**Cons**:
- Requires sandboxed command execution in the `spex` process — significant complexity and security surface
- Environment-dependent: commands that pass on one machine may fail on another
- Blocked on defining a portable command configuration format; not deliverable in v1

---

### Option C: Policy-as-code files

**Pros**:
- Policy rules are version-controlled alongside source code
- Familiar pattern for teams already using OPA or similar tools

**Cons**:
- Requires a file-watching or sync mechanism to keep SQLite state consistent with files
- Adds a second source of truth for project state, conflicting with `spex`'s SQLite-first model
- Increases operator burden for teams that only need simple per-spec rules

## Consequences

### Positive
- Agents can no longer silently mark work done without registering evidence on approved specs
- Operators can configure per-spec or per-task policies via CLI (`spex policy`) or MCP tools without touching code
- All policy evaluations and approval decisions are emitted as domain events and are auditable from the event log
- Policy enforcement is fail-closed for approved specs: if policy resolution fails, the operation is denied rather than allowed through
- The approval workflow provides a structured human-in-the-loop gate for risky operations

### Negative
- Adds friction for agents working on approved specs: evidence must be registered before `complete_task` or `complete_spec` will succeed under an enforced policy
- Operators must explicitly configure policies; there is no default enforcement for new specs

### Risks
- Agents may register low-quality or fabricated evidence — Mitigation: evidence bundles record the submitting agent and timestamp; human reviewers can inspect bundle contents before approving
- Policy misconfiguration could block legitimate completions — Mitigation: policies default to `advisory` mode; operators must explicitly set `enforced` to activate blocking behavior
- Approval expiry could leave specs stuck if operators do not act — Mitigation: expired approvals surface in `spex policy approvals list`; operators can re-request or override

## Implementation Notes

### Rollout scope (v1)

Policy enforcement applies only to specs in `approved`, `in_progress`, `paused`, or `done` status. Draft and backlog specs are not enforced, reducing friction during exploration and planning. This is implemented in `policy_rollout_applies` in `src/sdd/policy.rs`.

### Risky operations (v1 list)

The following operations are classified as risky and subject to policy evaluation:

| Operation | Default disposition (enforced spec) |
|---|---|
| `destructive_command` | `require_approval` |
| `write_outside_allowed_scope` | `deny` |
| `schema_change` | `require_approval` |
| `global_config_change` | `require_approval` |
| `complete_task` | `deny` (unless evidence requirements are met) |
| `complete_spec` | `deny` (unless evidence requirements are met) |

Dispositions can be overridden per-spec or per-task via `rules_json` in the policy config.

### Evidence bundle requirements

Evidence bundles are registered against a task or spec and contain one or more `EvidenceItem` entries. Each item carries:

- `kind`: one of `test_run`, `lint`, `build`, `review`, `manual`
- `label`: human-readable description
- `artifact_path`: optional path to the supporting artifact
- `validation_run_id`: optional link to a recorded validation run

The `CompletionPolicy` for a task or spec under an enforced policy defaults to:

- `require_evidence_bundle: true`
- `require_rationale: true`
- `require_validation: primary` (tasks) / `full` (specs)
- `require_approval: false`

These defaults can be relaxed or tightened via policy config overlays.

### Approval workflow

Approval requests follow a single-level state machine: `pending → approved | rejected | cancelled | expired`. Multi-approver workflows are deferred to v2. Key constraints:

- Only humans (or operator-designated agents) may call `decide_approval`; the requesting agent cannot approve its own request
- A rejected or expired approval does not block re-requesting; a new approval record is created
- Approval state is checked at evaluation time; an `approved` record allows the operation to proceed

### Audit trail

Every policy evaluation that results in `denied` or `approval_required` emits a domain event. Approval state transitions (`ApprovalRequested`, `ApprovalDecided`) are also emitted. Operators can query the full audit trail via `spex events`.

### Operator workflow summary

1. Configure a policy: `spex policy set --scope spec --ref SPEC-NNN --rules '{"require_evidence_bundle": true}'`
2. Agent registers evidence before completing: `spex evidence register --task T-NNN --kind test_run --label "cargo test passed"`
3. If approval is required, agent requests it: `spex policy approval request --task T-NNN --operation complete_task`
4. Operator reviews and decides: `spex policy approval decide <id> --approve`
5. Agent completes the task: `spex task complete T-NNN`

## References

- ADR-001 — Core architecture and local-first system constraints
- ADR-002 — Hardened control plane; `src/sdd/workflow.rs` is the authority for lifecycle transitions
- `src/sdd/policy.rs` — Policy config model, risky operation evaluation, approval workflow
- `src/sdd/evidence.rs` — Evidence bundle model and evidence item kinds
- `src/sdd/workflow.rs` — Gate enforcement in `complete_task` and `complete_spec`
- `src/cli/policy.rs` — CLI surface for policy and approval management
- `src/mcp/tools/policy.rs` — MCP tool surface for policy and approval management
- `migrations/20260422113000_policy_engine.sql` — Schema for `policy_configs`, `approvals`, and evidence tables
