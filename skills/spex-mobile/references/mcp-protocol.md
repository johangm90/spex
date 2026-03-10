# MCP State Protocol — spex-mobile

## 1. MCP State Check (mandatory at startup)

Before any other action, verify the shared persistent memory is available:

**Step 1** — Call `state_snapshot` via the `spex-state` MCP tools.

**Step 2** — Verify `project_dir` in the response matches the current project directory.

**Step 3** — If the call **succeeds** → proceed normally.

**Step 4** — If the call **fails** (tool unavailable or error):
- Inform the human: _"The `spex-state` MCP server is not available. This is required for shared memory. May I run `spex mcp setup` to configure it?"_
- **Wait** for explicit human approval before running the setup.
- After approval, run `spex mcp setup` then retry `state_snapshot`.

---

## 2. Session Context (startup restore)

After the MCP availability check, restore last task/file context:

```
memory_get(agent="spex-mobile", key="session_context")
```

If found, display: _"Resuming: last worked on [task] — [summary]."_

---

## 3. State Protocol Snippets

### On task completion

```js
memory_set(agent="spex-mobile", key="session_context", value=JSON.stringify({
  slice: "SLICE-NNN",
  task: "T0NN-N",
  files_changed: ["path/to/Screen.tsx"],
  summary: "one sentence",
  timestamp: new Date().toISOString()
}))
```

### On artifact production

```js
artifact_register(
  id="A0NN-N",
  slice="SLICE-NNN",
  task="T0NN-N",
  agent="spex-mobile",
  type="code",
  path="src/...",
  description="..."
)
```

---

## 4. Input — memory_get pattern

| Input | MCP call |
|-------|----------|
| Slice spec | `memory_get(agent="spex-architect", key="slice_SLICE-NNN")` |
| API contract | `memory_get(key="artifact_PROJ-API-NNN")` from `spex-backend` |
| Frontend component spec | `memory_get(agent="spex-frontend", key="artifact_A0NN-N")` |
