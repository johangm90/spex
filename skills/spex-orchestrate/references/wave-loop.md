# Wave Loop Reference

Full wave loop procedure, task prompt format, escalation rules, and gate checkpoint protocol.

---

## Wave Loop (Step 6 of Process)

A **wave** is a set of tasks that can be executed in parallel because they have
no mutual dependencies. Execute waves sequentially; tasks within a wave may be
delegated concurrently.

### For each wave:

#### a. Gate Checkpoint (before starting the wave)

After completing the previous wave and confirming `make check` passes, ask:

> _"Wave N complete for SLICE-NNN — gates green ✅. Ready for Wave N+1: [task list].
> Proceed, or would you like to pause?"_

- **Wait for explicit human confirmation** before delegating.
- If the human requests pause → follow the Pause flow in SKILL.md.
- If the human confirms → continue.

> **Rule:** Never chain waves autonomously. Each wave requires a human go-ahead.

#### b. Assign

Post a task prompt (see format below) to each target agent.
Emit one `TaskHandedOff` event per delegation via `state_event_emit`.
Update each task: `state_task_update(id="T0NN-N", status="in_progress")`.

#### c. Collect

When an agent reports back:

1. Validate the output contains a valid **artifact envelope** (see task prompt format).
2. If envelope is missing or malformed → reject and re-delegate to the same agent with a correction note.
3. If valid → `state_task_update(id="T0NN-N", status="done", output_artifact="<id>")`.

#### d. Gate

Once all tasks in the wave are `done`:

1. Run `make check`.
2. If **green** → proceed to gate checkpoint for next wave (or archive if last wave).
3. If **red** → identify the failing task/agent and re-delegate with the failure output.
   - If the **same gate fails twice consecutively** → escalate (see Escalation below).

---

## Task Prompt Format

Use this exact structure when delegating to a specialist agent:

```
ORCHESTRATOR → [AGT-ROLE]
TASK: [task-id]
SLICE: [slice-id]
INPUTS: [artifact-id list — retrieve via artifact_query or memory_get]
EXPECTED OUTPUT: [artifact-id] type=[type]
DEADLINE GATE: make check must pass
---
[task description: clear, scoped to this agent's skill; no implementation details
 that belong to another agent; reference the slice spec section that applies]
```

**Tips:**
- `INPUTS` should list MCP artifact IDs, not file paths where possible.
- `EXPECTED OUTPUT` must match what `artifact_register` will record.
- Keep the task description to 3–5 sentences; link to spec sections for detail.

---

## Escalation Rules

| Condition | Action |
|-----------|--------|
| Agent output missing artifact envelope | Reject and re-delegate once with correction note |
| Same gate fails twice consecutively | Open a GitHub issue labelled `blocked`; halt delegation on that task; notify human |
| Agent explicitly reports a blocker | Surface to human immediately; do not attempt workarounds |
| Human unreachable and gate blocked | Emit `SlicePaused` with `reason: "blocked-gate"`; halt |

**Blocked issue format:**

```
Title: [BLOCKED] SLICE-NNN / TASK-ID — gate failure: <short description>
Body:
  Slice: SLICE-NNN
  Task:  T0NN-N
  Agent: <agent-name>
  Gate:  make check — <failing check name>
  Attempts: 2
  Last output: <paste gate failure>
  Action needed: human review
Labels: blocked
```

---

## Gate Checkpoint Protocol (Summary)

| Checkpoint | Trigger | Action |
|------------|---------|--------|
| Pre-wave | Previous wave complete + `make check` green | Ask human to proceed or pause |
| Post-delegation | All tasks in wave collected | Run `make check` |
| Post-all-waves | Last wave gates green | Offer branching opt-in; then archive |
| Escalation | Double gate failure | Open `blocked` issue; halt; notify human |
