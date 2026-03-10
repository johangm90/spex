# MCP Protocol — spex-qa Reference

State Protocol snippets, QASignOff event JSON, and artifact storage patterns used by `spex-qa`.

---

## Session Context (Startup)

On startup, restore last test task context:

```js
memory_get(agent="spex-qa", key="session_context")
// If found, display: "Resuming: last worked on [task] — [summary]."
```

---

## Session Context (Completion)

After completing a task, persist context for next session:

```js
memory_set(agent="spex-qa", key="session_context", value=JSON.stringify({
  slice: "SLICE-NNN",
  task: "T0NN-N",
  test_files: ["path/to/test.ts"],
  passed: N,
  total: N,
  summary: "one sentence",
  timestamp: new Date().toISOString()
}))
```

---

## QASignOff Event

Emit via `state_event_emit` when all gates pass:

```json
{
  "type": "QASignOff",
  "slice": "<slice-id>",
  "agent": "spex-qa",
  "payload": {
    "passed_criteria": "<integer>",
    "total_criteria": "<integer>"
  }
}
```

**Rule:** `QASignOff` must be emitted before reporting completion to `spex-orchestrate`. It is the gate that allows the `in_progress` → `done` transition.

---

## Artifact Storage Pattern

Test plans are stored in MCP only (no file written to the repo):

```js
// 1. Register artifact metadata
artifact_register(
  id="PROJ-TEST-NNN",
  slice="SLICE-NNN",
  task="T0NN-N",
  agent="spex-qa",
  type="test_plan",
  path="mcp:test_plans/PROJ-TEST-NNN",
  description="Test plan for SLICE-NNN — N cases"
)

// 2. Store full artifact content
memory_set(
  agent="spex-qa",
  key="artifact_PROJ-TEST-NNN",
  value=<test plan content>
)
```

---

## Querying Artifacts

To retrieve a previously registered test plan:

```js
// Fetch by key
memory_get(agent="spex-qa", key="artifact_PROJ-TEST-NNN")

// List all test plan artifacts for a slice
artifact_query(agent="spex-qa", spec="SLICE-NNN", type="test_plan")
```
