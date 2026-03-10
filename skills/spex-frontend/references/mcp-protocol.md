# MCP State Protocol — spex-frontend

Reference snippets for session recovery, artifact registration, and reading inputs.

---

## 1. Startup — Restore Session Context

Call this at the start of every task to recover the last known working state:

```js
memory_get({
  agent: "spex-frontend",
  key: "session_context"
})
```

If the key exists, display:

> _"Resuming: last worked on [task] — [summary]."_

If the key is absent, proceed with fresh context from the slice spec.

---

## 2. Read Inputs from Memory

### Slice spec (written by spex-architect)
```js
memory_get({
  agent: "spex-architect",
  key: "slice_SLICE-NNN"   // e.g. "slice_AUTH-001"
})
```

### API contract artifact
```js
memory_get({
  agent: "spex-backend",            // or "spex-architect"
  key: "artifact_PROJ-API-NNN"      // exact key from the architect's register call
})
```

### Own previous artifact (e.g. wireframe or prior component)
```js
memory_get({
  agent: "spex-frontend",
  key: "artifact_A0NN-N"
})
```

---

## 3. Task Completion — Persist Session Context

Call this when a task reaches `status: "done"`:

```js
memory_set({
  agent: "spex-frontend",
  key: "session_context",
  type: "config",
  value: {
    slice: "SLICE-NNN",
    task: "T0NN-N",
    files_changed: [
      "src/components/FeatureName.tsx",
      "src/services/featureApi.ts",
      "src/store/featureSlice.ts"
    ],
    summary: "One sentence describing what was implemented.",
    timestamp: "<ISO-8601 timestamp>"
  }
})
```

Then update the task record:

```js
state_task_update({
  id: "T0NN-N",
  status: "done",
  output_artifact: "A0NN-N"   // omit if no artifact was registered
})
```

---

## 4. Register a Code Artifact

Call this after producing a significant code deliverable (component library, service module, offline queue, etc.) that other agents may consume:

```js
artifact_register({
  id: "A0NN-N",                          // unique artifact ID
  spec: "SLICE-NNN",                     // owning slice
  task: "T0NN-N",                        // producing task
  agent: "spex-frontend",
  type: "code",
  path: "src/components/FeatureName/",   // repo-relative path
  description: "Brief description of what the artifact contains and its public interface."
})
```

### Type values for frontend artifacts

| `type` | When to use |
|--------|-------------|
| `code` | Component, hook, service, store module |
| `test` | E2E test suite or unit test file |
| `config` | Build config, environment manifest |

---

## 5. Emit Domain Events (optional but recommended)

Signal task milestones to the event log so `spex-orchestrate` can track progress:

```js
// When starting work
state_event_emit({
  type: "task.started",
  spec: "SLICE-NNN",
  agent: "spex-frontend",
  payload: { task: "T0NN-N" }
})

// When done
state_event_emit({
  type: "task.completed",
  spec: "SLICE-NNN",
  agent: "spex-frontend",
  payload: { task: "T0NN-N", gate: "PASS" }
})
```
