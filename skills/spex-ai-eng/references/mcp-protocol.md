# MCP Protocol Reference — spex-ai-eng

## MCP State Check (Full 4-Step Procedure)

Perform this check **before any other action** at the start of every session.

**Step 1 — Call `state_snapshot`**
```
state_snapshot()
```
Use the `spex-state` MCP tool. This returns project metadata, active slices, recent events, and the `project_dir` field.

**Step 2 — Verify `project_dir`**
Confirm that `project_dir` in the response matches the repository root you are currently operating in. If it does not match, stop and inform the human before proceeding.

**Step 3 — Success path**
If the call succeeds and `project_dir` matches → proceed normally with the task.

**Step 4 — Failure path**
If the call fails (tool unavailable, MCP server not running, or error response):
1. Inform the human:
   > "The `spex-state` MCP server is not available. This is required for shared memory. May I run `spex mcp setup` to configure it?"
2. **Wait** for explicit human approval — do not run any setup commands automatically.
3. After approval, run:
   ```
   spex mcp setup
   ```
4. Retry `state_snapshot`. If it still fails, escalate to the human and pause the task.

---

## State Protocol Snippets

### Session Context — Restore on Startup

After the MCP availability check, restore the last known AI feature context:

```typescript
// Input pattern
memory_get({
  agent: "spex-ai-eng",
  key: "session_context"
})

// On success, display to human:
// "Resuming: last worked on [task] — [summary]."
// On miss (key not found), start fresh.
```

### Session Context — Persist on Task Completion

```typescript
memory_set({
  agent: "spex-ai-eng",
  key: "session_context",
  type: "decision",
  value: {
    slice: "SLICE-NNN",
    task: "T0NN-N",
    last_ai_feature: "brief description of the AI capability implemented",
    files_changed: ["src/ai/feature/handler.ts", "prompts/v1/feature.md"],
    summary: "one sentence describing what was done and why",
    timestamp: new Date().toISOString()
  }
})
```

### Artifact Registration

Call `artifact_register` immediately after producing any output artifact (code, prompt file, eval suite, config):

```typescript
artifact_register({
  id: "A0NN-N",
  spec: "SLICE-NNN",
  task: "T0NN-N",
  agent: "spex-ai-eng",
  type: "code",           // "code" | "doc" | "config" | "eval"
  path: "src/ai/feature/handler.ts",
  description: "LLM integration handler for <feature> — wraps OpenAI chat completions with retry and fallback"
})
```

---

## `memory_get` Input Patterns

### Read a slice spec authored by spex-architect
```typescript
memory_get({
  agent: "spex-architect",
  key: "slice_SLICE-NNN"
})
```

### Read an API contract artifact
```typescript
memory_get({
  agent: "spex-backend",
  key: "artifact_PROJ-API-NNN"
})
// or query by spec:
artifact_query({ spec: "SLICE-NNN", type: "api_contract" })
```

### Read own last session context
```typescript
memory_get({
  agent: "spex-ai-eng",
  key: "session_context"
})
```

### Read a specific decision or architecture note
```typescript
memory_get({
  agent: "spex-ai-eng",
  key: "model_selection_SLICE-NNN",
  spec: "SLICE-NNN"
})
```
