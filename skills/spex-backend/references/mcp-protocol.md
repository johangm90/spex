# MCP State Protocol — spex-backend

Reference snippets for the spex-backend skill's MCP interactions.

---

## On Startup — Restore Session Context

```js
memory_get(agent="spex-backend", key="session_context")
```

If an entry is found, display:

> *"Resuming: last worked on [task] — [summary]."*

---

## On Task Completion — Save Session Context

```js
memory_set(
  agent="spex-backend",
  key="session_context",
  type="config",
  value={
    slice: "SLICE-NNN",
    task: "T0NN-N",
    files_changed: ["src/path/to/file.ts"],
    summary: "One sentence describing what was implemented.",
    timestamp: "<ISO-8601>"
  }
)
```

---

## On Artifact Production — Register Code Artifact

```js
artifact_register(
  id="A0NN-N",
  spec="SLICE-NNN",
  task="T0NN-N",
  agent="spex-backend",
  type="code",
  path="src/...",
  description="Brief description of what this file implements."
)
```

---

## On API Contract Production — Register + Store

```js
artifact_register(
  id="PROJ-API-NNN",
  spec="SLICE-NNN",
  task="T0NN-N",
  agent="spex-backend",
  type="api_contract",
  path="mcp:api/PROJ-API-NNN",
  description="OpenAPI contract for <feature name>."
)

memory_set(
  agent="spex-backend",
  key="artifact_PROJ-API-NNN",
  type="architecture",
  value="<full OpenAPI spec content>"
)
```

---

## Reading Input Artifacts

### Slice spec (from architect)

```js
memory_get(agent="spex-architect", key="slice_SLICE-NNN")
```

### DB design artifact

```js
memory_get(agent="spex-db", key="artifact_PROJ-DB-NNN")
```

### Existing API contract

```js
memory_get(agent="spex-backend", key="artifact_PROJ-API-NNN")
```
