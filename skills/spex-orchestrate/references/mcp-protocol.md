# MCP Protocol Reference

Full procedures for MCP state check, PRD check, State Protocol snippets, and event payloads.

---

## MCP State Check (mandatory at startup)

Before any other action, verify the shared persistent memory is available and
scoped to this project:

1. Call `state_snapshot` via the `spex-state` MCP tools.
2. If the call **fails** (tool unavailable or error):
   - Inform the human: _"The `spex-state` MCP server is not available. This is required for shared memory. May I run `spex mcp setup` to configure it?"_
   - **Wait** for explicit human approval.
   - After approval, run `spex mcp setup` then retry `state_snapshot`.
   - If it still fails, halt and ask the human to check their OpenCode MCP configuration.
3. If the call **succeeds**, verify `project_dir`:
   - The response includes a `"project_dir"` field with the absolute path being served.
   - **Compare `project_dir` to the project you are working in.**
   - If `project_dir` does NOT match → **halt immediately** and inform the human:
     _"⚠️ MCP is serving state for `{project_dir}` but we are working in `{current project}`. Run `spex mcp setup` in this project directory first, then restart OpenCode."_
   - If `config_source` is `"global-opencode.json"` → add caution even if dirs match:
     _"ℹ️ MCP is configured globally. Consider running `spex mcp setup` for per-project isolation."_
4. If `project_dir` matches → proceed to PRD check.

---

## PRD Check (mandatory after MCP check)

1. Call `state_prd_get` (or `state_constitution_get`) via MCP.
2. Evaluate the response:
   - `exists: false` → `docs/PRD.md` does not exist.
   - `is_template: true` → `docs/PRD.md` exists but contains only placeholder text.
   - `is_template: false` → PRD is filled; read `content` silently as context.
3. **If the PRD is missing or template-only**, stop and delegate to `spex-architect`:
   - Inform the human: _"📋 `docs/PRD.md` hasn't been filled out yet. I need it before I can orchestrate anything. Please ask `@spex-architect` to create it — it will walk you through the process interactively."_
   - Do **not** attempt to collect PRD content or write files yourself.
   - **Wait** for the human to confirm the PRD is ready before proceeding.
4. **If the PRD is filled** → acknowledge briefly: _"📋 PRD loaded. [one-sentence summary of the project vision]."_ Then proceed to the normal startup flow.

> **Rule:** Never start orchestrating slices without a filled `docs/PRD.md`.
> Writing `docs/PRD.md` is `spex-architect`'s responsibility — `spex-orchestrate` only reads it.

---

## State Protocol

### On startup

```js
memory_get(agent="spex-orchestrate", key="session_context")
// If found, display: "Resuming: orchestrating [slice] — last wave/task [context]."
```

### On plan decomposition

Store the full task plan in MCP — do **not** write a file to the repository:

```js
memory_set(agent="spex-orchestrate", key="plan_SLICE-NNN", value=JSON.stringify({
  slice: "SLICE-NNN",
  title: "<slice title>",
  waves: [...],
  tasks: [...],
  created_at: new Date().toISOString()
}))

artifact_register(
  id="PLAN-SLICE-NNN",
  slice="SLICE-NNN",
  task="orchestration",
  agent="spex-orchestrate",
  type="plan",
  path="mcp://plan_SLICE-NNN",
  description="Task decomposition plan for SLICE-NNN"
)
```

### On session end

```js
memory_set(agent="spex-orchestrate", key="session_context", value=JSON.stringify({
  slice: "SLICE-NNN",
  last_wave: N,
  last_task: "T0NN-N",
  pending_tasks: ["T0NN-N", "..."],
  timestamp: new Date().toISOString()
}))
```

---

## Event Payloads

### TaskHandedOff

Emitted once per task delegation via `state_event_emit`:

```json
{
  "type": "TaskHandedOff",
  "task": "<task-id>",
  "slice": "<slice-id>",
  "agent": "spex-orchestrate",
  "payload": {
    "to_agent": "<agent-name>",
    "artifact_id": "<artifact-id>"
  }
}
```

### SlicePaused

Emitted when the slice transitions to `paused` status:

```json
{
  "type": "SlicePaused",
  "slice": "<slice-id>",
  "agent": "spex-orchestrate",
  "payload": {
    "paused_at_wave": "<N>",
    "pending_tasks": ["<task-id>", "..."],
    "reason": "<human-provided reason or 'human-requested'>"
  }
}
```

### SliceResumed

Emitted when the slice transitions back to `in_progress`:

```json
{
  "type": "SliceResumed",
  "slice": "<slice-id>",
  "agent": "spex-orchestrate",
  "payload": {
    "resuming_at_wave": "<N>",
    "next_task": "<task-id>"
  }
}
```

### SliceCompleted

Emitted when all gates pass and the slice is archived:

```json
{
  "type": "SliceCompleted",
  "slice": "<slice-id>",
  "agent": "spex-orchestrate",
  "payload": {
    "total_waves": "<N>",
    "tasks_completed": ["<task-id>", "..."]
  }
}
```
