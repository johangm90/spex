# MCP Protocol Reference

## Startup State Check (mandatory)

Before any other action, verify the shared persistent memory is available:

1. Call `state_snapshot` via the `spex-state` MCP tools.
2. If the call **succeeds** → proceed normally.
3. If the call **fails** (tool unavailable or error):
   - Inform the human: _"The `spex-state` MCP server is not available. This is required for shared memory. May I run `spex mcp setup` to configure it?"_
   - **Wait** for explicit human approval before running the setup.
   - After approval, run `spex mcp setup` then retry `state_snapshot`.

---

## Session Context — memory_set / memory_get

### Restore on startup
```
memory_get(agent="spex-architect", key="session_context")
```
If found, display: _"Resuming: last worked on [slice/ADR] — [summary]."_

### Save on task completion
```
memory_set(
  agent="spex-architect",
  key="session_context",
  type="config",
  value=JSON.stringify({
    slice: "SLICE-NNN",
    task: "T0NN-N or ADR-XXXX",
    summary: "one sentence describing what was last done",
    timestamp: new Date().toISOString()
  })
)
```

---

## Slice Spec Storage

Slice specs live in MCP only — no `docs/slices/` files.

### Store a slice spec
```
memory_set(
  agent="spex-architect",
  key="slice_SLICE-NNN",
  type="architecture",
  value=JSON.stringify({ /* full slice spec object */ })
)
```

### Retrieve a slice spec (by other agents)
```
memory_get(agent="spex-architect", key="slice_SLICE-NNN")
```

### Update slice status
```
state_slice_update(
  id="SLICE-NNN",
  status="approved",        // draft | approved | in-progress | done
  updated_by="spex-architect"
)
```

> Both `state_slice_update` **and** `memory_set(key="slice_SLICE-NNN")` must be called on approval — one updates metadata, the other stores the full spec content.

---

## artifact_register Patterns

Register every produced ADR, slice spec snapshot, or architecture document:

```
artifact_register(
  id="A0NN-N",
  spec="SLICE-NNN",
  task="T0NN-N",
  agent="spex-architect",
  type="adr",               // adr | doc | slice_spec
  path="docs/adr/ADR-NNNN.md",
  description="Short description of what this artifact contains"
)
```

### Type reference

| type | Used for |
|------|----------|
| `adr` | Architecture Decision Records |
| `doc` | PRD, architecture overviews |
| `slice_spec` | Snapshot of an approved slice spec |
