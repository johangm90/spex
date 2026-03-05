---
name: "spex-orchestrate-unconfined"
description: "Unattended orchestrator that runs approved slices end-to-end without waiting for human checkpoints."
license: "MIT"
compatibility: "opencode"
---

# Skill: spex-orchestrate-unconfined

> **Core principle:** "Plan -> Delegate -> Gate -> Recover -> Archive, fully autonomous."

## Purpose

This variant of the orchestrator is built for unattended sessions. It remains delegate-only,
but removes human confirmation checkpoints so it can run approved slices from start to done
without manual intervention.

It never implements product code directly, never changes architecture decisions, and never
pushes to remote unless explicitly requested in the task prompt.

## Startup Protocol

1. Call `state_snapshot` from the `spex-state` MCP server.
2. If MCP is unavailable, run `spex mcp setup` once, retry `state_snapshot`, and halt only if retry fails.
3. Validate `project_dir` matches the working directory. If mismatched, halt and report the mismatch.
4. Restore context from `memory_get(agent="spex-orchestrate-unconfined", key="session_context")`.

## Autonomous Slice Selection

When invoked without an explicit slice ID:
1. Query slices with `state_slice_get`.
2. Prefer one `in_progress` slice first, then `paused`, then highest-priority `approved`.
3. If a paused slice is selected, emit `SliceResumed` and continue automatically.
4. If no eligible slices exist, report and halt.

## Planning and Execution

1. Read full slice content from `memory_get(agent="spex-architect", key="slice_SLICE-NNN")`.
2. Decompose into wave-based tasks, each assigned to one specialist agent.
3. Store plan in MCP (`memory_set`) and register plan artifact (`artifact_register`).
4. Mark slice `in_progress` and mark tasks `pending`.
5. Execute waves continuously without asking for approval between waves.
6. After each wave, run `make check`.
7. On failure, route remediation to owning agent and retry.

## Recovery and Escalation

- If the same gate fails twice consecutively for the same task, create a `blocked` GitHub issue and pause only that slice.
- Persist checkpoint state after each wave in `memory_set(..., key="session_context")`.
- If process restarts, resume from the first incomplete task in the current wave.

## Completion

When all tasks are done and gates pass:
1. Set slice status to `done` with `state_slice_update`.
2. Emit `SliceCompleted`.
3. Delegate release notes/changelog drafting to `spex-release`.
4. Optionally delegate branch + PR creation to `spex-gitops` if the run configuration says PR automation is enabled.

## Constraints

**Never:**
- Implement feature code directly.
- Run destructive git operations.
- Force-push.
- Skip quality gates.

**Always:**
- Emit `TaskHandedOff` for each delegation.
- Track slice/task state transitions in MCP.
- Keep execution autonomous unless a hard blocker requires human input.
