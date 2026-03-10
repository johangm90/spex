# MCP Protocol — spex-db

Reference snippets for state management, artifact registration, and session recovery.

---

## On Startup — Restore Session Context

```
memory_get(agent="spex-db", key="session_context")
```

If found, display:
> _"Resuming: last worked on [task] — [summary]."_

---

## On Task Completion — Save Session Context

```
memory_set(
  agent="spex-db",
  key="session_context",
  type="config",
  value={
    slice: "SLICE-NNN",
    task: "T0NN-N",
    artifact: "PROJ-DB-NNN",
    summary: "one sentence describing what was modeled",
    timestamp: "<ISO-8601>"
  }
)
```

---

## On Artifact Production — Register db_design

```
artifact_register(
  id="PROJ-DB-NNN",
  spec="SLICE-NNN",
  task="T0NN-N",
  agent="spex-db",
  type="db_design",
  path="mcp:db/PROJ-DB-NNN",
  description="Schema design for <feature> — <N> entities, additive migration"
)

memory_set(
  agent="spex-db",
  key="artifact_PROJ-DB-NNN",
  type="architecture",
  value=<full artifact content as string>
)
```

---

## db_design Artifact Front-Matter

Every `db_design` artifact stored in MCP must begin with this front-matter block:

```yaml
---
id: "PROJ-DB-NNN"
type: db_design
owner_agent: spex-db
slice: "SLICE-NNN"
task: "T0NN-N"
status: draft        # draft | review | approved
---
```

---

## Querying Artifacts

To retrieve a previously registered db_design:

```
artifact_query(agent="spex-db", type="db_design", spec="SLICE-NNN")
```

To read the artifact content:

```
memory_get(agent="spex-db", key="artifact_PROJ-DB-NNN")
```

---

## Task Status Update

```
state_task_update(
  id="T0NN-N",
  status="done",
  output_artifact="PROJ-DB-NNN"
)
```
