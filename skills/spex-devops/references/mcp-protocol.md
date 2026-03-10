# MCP Protocol — spex-devops

## MCP State Check (mandatory at startup)

Before any other action, verify shared persistent memory is available:

1. Call `state_snapshot` via the `spex-state` MCP tools.
2. Verify `project_dir` in the response matches the current project directory.
3. If the call **succeeds** → proceed normally.
4. If the call **fails** (tool unavailable or error):
   - Inform the human: _"The `spex-state` MCP server is not available. This is required for shared memory. May I run `spex mcp setup` to configure it?"_
   - **Wait** for explicit human approval before running setup.
   - After approval, run `spex mcp setup` then retry `state_snapshot`.

---

## State Protocol

### On startup

After the MCP availability check, restore the last known context:

```
memory_get(agent="spex-devops", key="session_context")
```

If found, display: _"Resuming: last worked on [task] — [summary]."_

### On task completion

```
memory_set(
  agent="spex-devops",
  key="session_context",
  type="config",
  value={
    slice: "SLICE-NNN",
    task: "T0NN-N",
    files_changed: ["path/to/config.yaml"],
    summary: "one sentence describing what was done",
    timestamp: "<ISO-8601>"
  }
)
```

### On architecture decision

```
memory_set(
  agent="spex-devops",
  key="decision_<slug>",
  type="decision",
  spec="SLICE-NNN",
  value="<rationale and chosen approach>"
)
```

---

## Runbook MCP Storage Pattern

Runbooks are stored in MCP only — **never committed to the repository**.

### Step 1 — Register the artifact

```
artifact_register(
  id="PROJ-OPS-NNN",
  spec="SLICE-NNN",
  task="T0NN-N",
  agent="spex-devops",
  type="runbook",
  path="mcp:ops/PROJ-OPS-NNN",
  description="<one-line description of the runbook>"
)
```

### Step 2 — Store the content

```
memory_set(
  agent="spex-devops",
  key="artifact_PROJ-OPS-NNN",
  type="architecture",
  spec="SLICE-NNN",
  value="<full runbook markdown content>"
)
```

### Retrieving a runbook

```
memory_get(agent="spex-devops", key="artifact_PROJ-OPS-NNN")
```

### Querying all registered runbooks

```
artifact_query(agent="spex-devops", type="runbook")
```
