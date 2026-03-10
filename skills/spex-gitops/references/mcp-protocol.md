# MCP State Protocol — spex-gitops

These are the canonical MCP tool call patterns for `spex-gitops`. Use these snippets verbatim; adjust values for the current slice/task.

---

## On Startup — Restore Session Context

```
memory_get(agent="spex-gitops", key="session_context")
```

If a value is returned, display:

> _"Resuming: last worked on [last_branch / last_pr] — [summary]."_

If no value is found, proceed without a message.

---

## On Task Completion — Save Session Context

```
memory_set(
  agent  = "spex-gitops",
  key    = "session_context",
  type   = "config",
  value  = {
    slice:       "SLICE-NNN",
    last_branch: "slice/NNN-<slug>",
    last_pr:     "<PR URL or number>",
    summary:     "one sentence describing what was done",
    timestamp:   "<ISO-8601 datetime>"
  }
)
```

---

## On Branch Creation — Record Decision

```
memory_set(
  agent  = "spex-gitops",
  key    = "branch_SLICE-NNN",
  spec   = "SLICE-NNN",
  type   = "decision",
  value  = {
    branch:    "slice/NNN-<slug>",
    base:      "main",
    created:   "<ISO-8601 datetime>",
    rationale: "human-requested for SLICE-NNN implementation"
  }
)
```

---

## On PR Creation — Record PR

```
memory_set(
  agent  = "spex-gitops",
  key    = "pr_SLICE-NNN",
  spec   = "SLICE-NNN",
  type   = "config",
  value  = {
    pr_url:    "<GitHub PR URL>",
    pr_number: <number>,
    branch:    "slice/NNN-<slug>",
    title:     "feat: SLICE-NNN — <title>",
    opened:    "<ISO-8601 datetime>"
  }
)
```

---

## Emit Event — Branch Created

```
event_emit(
  type    = "branch.created",
  spec    = "SLICE-NNN",
  agent   = "spex-gitops",
  payload = {
    branch: "slice/NNN-<slug>",
    base:   "main"
  }
)
```

---

## Emit Event — PR Opened

```
event_emit(
  type    = "pr.opened",
  spec    = "SLICE-NNN",
  agent   = "spex-gitops",
  payload = {
    pr_url:    "<GitHub PR URL>",
    pr_number: <number>,
    branch:    "slice/NNN-<slug>"
  }
)
```

---

## Update Slice Status (after PR open)

```
state_slice_update(
  id         = "SLICE-NNN",
  status     = "in-review",
  updated_by = "spex-gitops"
)
```

---

## Look Up a Prior Branch or PR

```
memory_get(agent="spex-gitops", key="branch_SLICE-NNN", spec="SLICE-NNN")
memory_get(agent="spex-gitops", key="pr_SLICE-NNN",     spec="SLICE-NNN")
```

---

## MCP Availability Check (mandatory at startup)

Before any other action, verify shared persistent memory is reachable:

1. Call `state_snapshot` via the `spex-state` MCP tools.
2. Confirm `project_dir` in the response matches the current working directory.
3. If the call **succeeds** → proceed normally.
4. If the call **fails**:
   - Inform the human: _"The `spex-state` MCP server is not available. This is required for shared memory. May I run `spex mcp setup` to configure it?"_
   - **Wait** for explicit human approval before running the setup command.
   - After approval, run `spex mcp setup` then retry `state_snapshot`.
