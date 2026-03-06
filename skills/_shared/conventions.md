# Agent Framework Conventions

Shared contract for all `spex-*` skills. Every artifact produced by an agent
must conform to these rules.

---

## Shared Persistent Memory (MCP State)

All agents **must** use the shared persistent SQLite state exposed via the
`spex-state` MCP server. **No agent writes state files, artifact documents,
or spec files to the project repository.** All state — slices, tasks, events,
artifacts, agent memory — lives exclusively in the MCP SQLite database.

> **Repository hygiene rule:** Do **not** create `ai/`, `docs/slices/`,
> `docs/orchestration/`, `docs/db/`, `docs/api/`, `docs/exploration/`,
> `docs/security/`, `docs/ops/`, `docs/releases/`, or any other state/artifact
> directory in the target project repository.
>
> **The only documents committed to the repo are:**
> - `docs/PRD.md` — product requirements (human-readable intent)
> - `docs/adr/ADR-NNNN.md` — Architecture Decision Records (durable decisions)
> - Source code and configuration files
>
> **Everything else — slice specs, task plans, DB designs, API contracts,
> exploration reports, security reviews, runbooks, test plans, release notes —
> lives in the MCP SQLite database only.**

### Available MCP Tools

| Tool | Purpose |
|------|---------|
| `state_snapshot` | Full project snapshot: slice counts, recent events, progress |
| `state_slice_get` | Read one slice (`id`) or all slices (no args) |
| `state_slice_update` | Update slice `status`, `ac_passed`, `agents`, `updated_by` |
| `state_task_get` | Read tasks, optionally filtered by `slice` or `id` |
| `state_task_update` | Update task `status` or `output_artifact` |
| `state_event_emit` | Append a domain event to the persistent event log |
| `state_event_query` | Query the event log with filters (`slice`, `type`, `agent`, `since`) |
| `memory_set` | Upsert a key/value pair for an agent into persistent `kv_store` |
| `memory_get` | Retrieve a previously stored key/value pair for an agent |
| `artifact_register` | Register a produced artifact (id, slice, task, agent, type, path, description) |
| `artifact_query` | Query registered artifacts by slice, task, agent, or type |

### state_snapshot — project identity fields

The snapshot response includes:
- `"project_dir"`: absolute path of the project whose DB is being served
- `"config_source"`: `"local-opencode.json"` | `"global-opencode.json"` | `"env"` | `"unknown"`
- `"isolation_warning"`: present only when `config_source` is `"global-opencode.json"`

**Agents MUST verify `project_dir` at startup** before reading any slice or task data.

### MCP Availability Check (mandatory for every agent at startup)

Before using any MCP tool, verify the server is reachable by calling
`state_snapshot`. If the call fails or the tool is not available:

1. **Inform the human:** _"The `spex-state` MCP server is not available. This
   is required for shared persistent memory. May I run `spex mcp setup` to
   configure it?"_
2. **Wait** for explicit human approval before executing the setup command.
3. Once the human approves, run `spex mcp setup` and retry `state_snapshot`.
4. If it still fails, ask the human to check their OpenCode MCP configuration
   and halt until resolved.

> **Never skip this check.** Operating without the MCP server means agents work
> in isolation with no shared state, producing inconsistent results.

### Mapping: Old file references → MCP tools

| Old reference | Use instead |
|---------------|-------------|
| `ai/state.json` (read) | `state_snapshot` or `state_slice_get` / `state_task_get` |
| `ai/state.json` (write) | `state_slice_update` / `state_task_update` |
| `ai/events.jsonl` (append) | `state_event_emit` |
| `ai/events.jsonl` (read) | `state_event_query` |
| `docs/slices/SLICE-NNN.md` (read/write) | `state_slice_get` / `state_slice_update` + `memory_get/set` |
| `docs/orchestration/SLICE-NNN-plan.md` | `memory_set(agent="spex-orchestrate", key="plan_SLICE-NNN", value=<content>)` |
| `docs/db/PROJ-DB-NNN.md` | `artifact_register` + `memory_set(key="artifact_PROJ-DB-NNN", value=<content>)` |
| `docs/api/PROJ-API-NNN.md` | `artifact_register` + `memory_set(key="artifact_PROJ-API-NNN", value=<content>)` |
| `docs/exploration/PROJ-EXP-NNN.md` | `artifact_register` + `memory_set(key="artifact_PROJ-EXP-NNN", value=<content>)` |
| `docs/security/PROJ-SEC-NNN.md` | `artifact_register` + `memory_set(key="artifact_PROJ-SEC-NNN", value=<content>)` |
| `docs/ops/PROJ-OPS-NNN.md` | `artifact_register` + `memory_set(key="artifact_PROJ-OPS-NNN", value=<content>)` |
| `docs/releases/PROJ-REL-NNN.md` | `artifact_register` + `memory_set(key="artifact_PROJ-REL-NNN", value=<content>)` |
| `spex spec start SLICE-NNN` | `state_slice_update` with `status: "in_progress"` |
| `spex task done TASK-NNN` | `state_task_update` with `status: "done"` |
| `spex spec done SLICE-NNN` | `state_slice_update` with `status: "done"` |
| `spex mcp serve -> spec_update(status="paused")` | `state_slice_update` with `status: "paused"` |
| `spex spec start SLICE-NNN` | `state_slice_update` with `status: "in_progress"` |

---

### Agent Memory Protocol

Agents MUST proactively read and write their own persistent memory to maintain
continuity across sessions. This is not optional — operating without memory means
every session starts blind, producing duplicated or inconsistent work.

#### On startup (every session)

After the MCP availability check, every agent MUST:

1. Call `memory_get` with `agent=<your-role>` and `key="session_context"` to restore
   previous context (last task, last slice, last known state).
2. If a value is found, display a one-line summary to the human:
   _"Resuming: last worked on SLICE-NNN / T0NN-N — <brief description>."_
3. If no value is found, proceed normally (first session).

#### On task completion (before ending a session)

After completing any task, every agent MUST call `memory_set` with:
- `agent=<your-role>`
- `key="session_context"`
- `value` = JSON string: `{"slice":"SLICE-NNN","task":"T0NN-N","summary":"<one sentence>","timestamp":"<ISO-8601>"}`

#### On artifact production

Whenever an agent produces a named artifact (code file, ADR, schema, contract,
test plan, exploration report, etc.), it MUST:

1. Call `artifact_register` with all required fields.
2. For artifacts with significant content (schemas, contracts, specs, reports),
   also store the full content:
   ```
   memory_set(agent="<your-role>", key="artifact_<ARTIFACT-ID>", value=<content as JSON string>)
   ```

Do not rely on the orchestrator to register artifacts; each agent registers its own outputs.

---

## Artifact Envelope

Every named artifact produced by an agent — whether its content is stored in MCP
memory or in the repository (ADRs only) — **must** begin with a YAML front-matter
block delimited by `---`.

```yaml
---
id: "<PREFIX>-<NNN>"            # Unique artifact ID, e.g. PROJ-DB-001
type: "<type>"                  # See type registry below
owner_agent: "<role>"           # Agent skill responsible, e.g. spex-db
status: "draft|review|validated|deprecated"
depends_on:                     # List of artifact IDs this one builds on
  - "<id>"
created_at: "YYYY-MM-DD"
updated_at: "YYYY-MM-DD"
outputs:                        # Concrete deliverables this artifact produces
  - "<description>"
risks:                          # Known risks or open questions
  - "<description>"
acceptance_criteria:            # Must be non-empty for status >= review
  - "<criterion>"
---
```

## Type Registry

| type              | Description                                 | Typical owner  |
|-------------------|---------------------------------------------|----------------|
| `vision`          | High-level system architecture summary      | spex-architect  |
| `slice_spec`      | Vertical slice definition                   | spex-architect  |
| `task`            | Single implementation task                  | spex-orchestrate|
| `adr`             | Architecture Decision Record                | spex-architect  |
| `db_design`       | Database schema / entity model              | spex-db         |
| `api_contract`    | OpenAPI / REST / event contract             | spex-backend    |
| `runbook`         | Operational runbook                         | spex-devops     |
| `test_plan`       | Test strategy & test cases                  | spex-qa         |
| `security_review` | Security & compliance assessment            | spex-qa         |
| `release_note`    | Release summary & changelog entry           | spex-gitops     |
| `exploration`     | Codebase / domain exploration notes         | *(any agent)*   |

Projects may extend this registry with domain-specific types (e.g. `fiscal_spec`).

## Artifact Rules

1. **No envelope = rejected.** Gate scripts fail any artifact without a valid envelope.
2. **ID uniqueness.** IDs must be globally unique across all artifacts. Recommended prefix pattern:
   - `PROJ-ARCH-NNN` — architecture / vision
   - `PROJ-DB-NNN` — database designs
   - `PROJ-API-NNN` — API contracts
   - `PROJ-SEC-NNN` — security reviews
   - `PROJ-OPS-NNN` — runbooks / ops
   - `PROJ-TEST-NNN` — test plans
   - `PROJ-EXP-NNN` — exploration reports
   - `ADR-NNNN` — architecture decisions
   - `SLICE-NNN` — slice specs
   - `TASK-NNN` — tasks
   Projects replace `PROJ` with their own prefix.
3. **`acceptance_criteria` must not be empty** for any artifact with status `review` or `validated`.
4. **`depends_on`** must reference existing artifact IDs. Dangling references are gate failures.
5. **`updated_at`** must be updated on every revision.
6. **`status` progression:** `draft` → `review` → `validated`. Skipping is forbidden.
   A status may move to `deprecated` from any state.
   > **Vocabulary note:** `validated` means "the artifact has been fully verified and
   > gate-passed". The word `approved` is reserved exclusively for the slice lifecycle
   > (meaning "the human has authorised implementation of this slice spec"). Never use
   > `approved` as an artifact status.
7. **No TODO in mandatory fields.** Mandatory fields: `id`, `type`, `owner_agent`,
   `status`, `created_at`, `updated_at`, `outputs`, `acceptance_criteria`.

---

## Repository Rules

### Default Mode: Dev Flow (commits directly to current branch)

By default all agents commit directly to the **current branch** (typically `main`
for solo-dev workflows). No branches are created unless the human explicitly
requests it.

**Dev flow commit protocol for all implementation agents:**

```
git add <source files only — never docs/ artifact files>
git commit -m "<type>(<scope>): <description> — Refs: SLICE-NNN / TASK-NNN"
```

**Never commit:**
- `ai/` directory or any MCP state files
- `docs/slices/`, `docs/orchestration/`, `docs/db/`, `docs/api/`,
  `docs/exploration/`, `docs/security/`, `docs/ops/`, `docs/releases/`
- Any artifact whose content is managed by the MCP server

### Optional: Branching and Pull Requests (via `spex-gitops`)

When the human explicitly requests a branch + PR workflow, the orchestrator
delegates fully to `spex-gitops`:

1. `spex-orchestrate` asks the human after first gate pass: _"All gates are green.
   Would you like me to create a feature branch and open a PR for this slice?
   I'll delegate that to @spex-gitops."_
2. If the human confirms, `spex-orchestrate` delegates to `spex-gitops` with the
   slice ID, commit range, and context.
3. `spex-gitops` creates the branch, ensures all commits are on it, and opens the
   PR using `gh pr create`.
4. `spex-release` is **not** invoked for merge in dev flow — the human merges when ready.

### Branch Conventions (when branching is requested)

| Branch          | Purpose                                  |
|-----------------|------------------------------------------|
| `main`          | Stable, gates-passing state              |
| `slice/NNN-*`   | All work for a given slice               |
| `feat/NNN-*`    | Single feature within a slice            |
| `fix/NNN-*`     | Bug fixes                                |
| `docs/NNN-*`    | Documentation-only changes               |
| `chore/NNN-*`   | Tooling, scripts, CI config              |

Branch names must be lowercase, hyphen-separated, and reference a SLICE or TASK ID.

### Commit Conventions (Conventional Commits)

```
<type>(<scope>): <short description>

[optional body]

Refs: SLICE-NNN / TASK-NNN / ADR-NNNN
```

**Types:** `feat`, `fix`, `docs`, `test`, `refactor`, `chore`, `ci`, `perf`

**Rules:**
- Subject line ≤ 72 characters
- Body explains *why*, not *what*
- Every commit refs at least one SLICE/TASK/ADR ID
- No "WIP" commits on `main`

### Pull Request Checklist (when PRs are used)

- [ ] `make check` exits 0
- [ ] No `TODO` in mandatory envelope fields
- [ ] Slice status in MCP confirms `in_progress` or `done`
- [ ] If an architectural decision was made: ADR created or updated in `docs/adr/`
- [ ] At least one reviewer approved before merging

### Forbidden on `main`

- Force-push
- Commits that skip gate checks

---

## Output Envelope (Agent Handoff)

When handing off a completed artifact to the Orchestrator, agents must confirm:

```
AGENT: <spex-role>
ARTIFACT: <ID>  type=<type>  status=<status>
GATE: make check [PASS|FAIL]
SUMMARY: <1-2 sentence summary of what was produced>
OPEN QUESTIONS: <list or "none">
```

---

## Typed Handoff Events

All inter-agent handoffs are recorded as typed events via `state_event_emit`
MCP tool. **Do not write to `ai/events.jsonl` or any file.**

### TaskHandedOff

Emitted by `spex-orchestrate` when delegating a task to a specialist agent.

```json
{
  "type": "TaskHandedOff",
  "task": "<task-id>",
  "from_agent": "spex-orchestrate",
  "to_agent": "<agent-name>",
  "artifact_id": "<artifact-id>",
  "timestamp": "<ISO-8601>"
}
```

### QASignOff

Emitted by `spex-qa` when all acceptance criteria for a slice have been verified
and all gates pass.

```json
{
  "type": "QASignOff",
  "slice": "<slice-id>",
  "passed_criteria": "<integer>",
  "total_criteria": "<integer>",
  "timestamp": "<ISO-8601>"
}
```

### SliceCompleted

Emitted by `spex-gitops` (dev flow or branch flow) after a slice reaches `done`
status. `spex-orchestrate` emits this directly if `spex-gitops` release finalisation
is not invoked.

```json
{
  "type": "SliceCompleted",
  "slice": "<slice-id>",
  "timestamp": "<ISO-8601>"
}
```

### SlicePaused

Emitted by `spex-orchestrate` when a slice is suspended by human request or
preempted by higher-priority work.

```json
{
  "type": "SlicePaused",
  "slice": "<slice-id>",
  "agent": "spex-orchestrate",
  "payload": {
    "paused_at_wave": "<integer>",
    "pending_tasks": ["<task-id>", "..."],
    "reason": "<human-provided reason or 'human-requested'>"
  }
}
```

### SliceResumed

Emitted by `spex-orchestrate` when a previously paused slice is restarted.

```json
{
  "type": "SliceResumed",
  "slice": "<slice-id>",
  "agent": "spex-orchestrate",
  "payload": {
    "resuming_at_wave": "<integer>",
    "next_task": "<task-id>"
  }
}
```

### ReleaseGatePass

Emitted by `spex-gitops` after a successful merge in the branch + PR flow.

```json
{
  "type": "ReleaseGatePass",
  "branch": "feat/SLICE-NNN",
  "slice": "<slice-id>",
  "agent": "spex-gitops",
  "timestamp": "<ISO-8601>"
}
```

> **Note:** `ReleaseGatePass` is only emitted when the optional full
> branching + PR + merge release flow is requested by the human and executed
> by `spex-gitops`.

---

## Git Protocol per Agent

> **Global rules (apply to all agents):**
> 1. No agent executes `git push`. The human decides when and what to publish.
> 2. **Default mode is dev flow**: commits go directly to the current branch.
>    No branches are created automatically.
> 3. Branching and PR creation are **opt-in** features delegated to `spex-gitops`
>    only when the human explicitly requests them.
> 4. **Never commit artifact documents** (DB designs, API contracts, test plans,
>    security reviews, runbooks, exploration reports, release notes, slice specs,
>    orchestration plans). These live in MCP only.

### `spex-architect`

| Moment | Git action |
|--------|-----------|
| Human approves a slice | Updates MCP only — `state_slice_update(status: "approved")`. No git commit. |
| Creates an ADR | `git add docs/adr/ADR-NNNN.md && git commit -m "docs(adr): add ADR-NNNN — <decision title>"` |
| Creates / updates PRD | `git add docs/PRD.md && git commit -m "docs(prd): <summary>"` |

### `spex-orchestrate`

| Moment | Git action |
|--------|-----------|
| Decomposes tasks | Stores plan in MCP via `memory_set(agent="spex-orchestrate", key="plan_SLICE-NNN")`. No git commit. |
| Human requests branch+PR | Delegates entirely to `spex-gitops` — does **not** run git itself |

> `spex-orchestrate` does **not** commit files, create branches, or open PRs.
> It is **wave-gated** by default: asks the human for confirmation before starting
> each new wave and never chains waves autonomously.
> In **unconfined mode** (activated by telling the orchestrator "run unconfined"),
> wave checkpoints are skipped but double gate failures still halt execution.

### `spex-db` / `spex-backend` / `spex-frontend` / `spex-mobile` / `spex-ai-eng` / `spex-devops`

| Moment | Git action |
|--------|-----------|
| Finishes an assigned task | `git add <own source files only> && git commit -m "feat(<domain>): <description> — Refs: TASK-NNN"` |

Where `<domain>` maps to: `db`, `api`, `ui`, `mobile`, `ai`, `infra` respectively.
Agents commit only **source files** (code, migrations, config). Artifact documents
(schemas, contracts, specs, reports) are stored in MCP — never committed to git.

### `spex-qa`

| Moment | Git action |
|--------|-----------|
| Completes slice sign-off | `git add <test source files> && git commit -m "test(<scope>): QA sign-off SLICE-NNN — <N>/<total> criteria passed — Refs: SLICE-NNN"` |

### `spex-gitops`

`spex-gitops` is the **only** agent that creates branches and PRs, and only when
the human explicitly requests them via `spex-orchestrate`.

| Moment | Git action |
|--------|-----------|
| Human requests a feature branch | `git checkout -b slice/NNN-<slug>` (slug = title in kebab-case, max 40 chars) |
| Opening a PR | `gh pr create --title "feat: SLICE-NNN — <title>" --base main --head slice/NNN-<slug> --body "..."` |
| Correcting a commit message | Executes `git commit --amend` or stages and commits with the corrected message |
| Updating CHANGELOG | `git add CHANGELOG.md && git commit -m "docs(changelog): ..."` |

### `spex-gitops` (release finalisation)

When the human requests CHANGELOG + semver tagging, or when the branch + PR flow is
active and a merge is ready, `spex-gitops` also handles release finalisation:

| Moment | Git action |
|--------|-----------|
| CHANGELOG update | `git add CHANGELOG.md && git commit -m "docs(changelog): SLICE-NNN — <title> — Refs: SLICE-NNN"` |
| Semver tag (if requested) | `git tag -a vX.Y.Z -m "SLICE-NNN — <title>"` |
| Merge to main (branch flow only) | `git checkout main && git merge --no-ff slice/NNN-<slug>` — if conflicts, STOP and escalate |

> In **dev flow** (default), `spex-gitops` only handles CHANGELOG + semver tagging
> when the human requests it — no merge operation needed.

### Conflict Policy (branch flow only)

1. Execute `git merge --no-ff --no-commit slice/NNN-<slug>`
2. If `git status` shows conflicts (`both modified`, `deleted by us`, etc.):
   - Abort: `git merge --abort`
   - Report to human: exact list of conflicting files, conflicting branch name
   - **Do not attempt auto-resolution**
3. If no conflicts: `git commit` with the release message and continue

---

## Agent Responsibility Matrix

| Agent | Mode | Owns | Must never |
|-------|------|------|------------|
| `spex-architect` | `primary` | PRD (file), ADRs (files), slice specs (MCP only), bounded contexts, product discovery | Write application code; self-approve slices; write slice specs to repo |
| `spex-orchestrate` | `primary` | Decompose, delegate, gate, MCP state tracking; unconfined autonomous mode | Write/edit code; make arch decisions; create branches/PRs; commit files |
| `spex-backend` | `subagent` | Server-side code, API contracts (MCP), business logic | Write frontend/mobile code; commit artifact docs |
| `spex-frontend` | `subagent` | Web UI components, client-side state, web E2E tests; wireframes & design tokens (Design Mode) | Write mobile code; write backend business logic; commit artifact docs |
| `spex-mobile` | `subagent` | Mobile screens, platform APIs, native modules, app-store configs | Write web UI; write backend business logic; commit artifact docs |
| `spex-ai-eng` | `subagent` | LLM integration, RAG pipelines, vector DBs, prompt engineering | Make product decisions; deploy infrastructure; commit artifact docs |
| `spex-db` | `subagent` | Schema design (MCP), migration source files | Deploy databases; write application queries; commit schema docs |
| `spex-devops` | `subagent` | CI/CD, containers, infra-as-code (source files), runbooks (MCP) | Write application business logic; commit runbook docs |
| `spex-gitops` | `subagent` | Branch creation, PR creation (gh), commit validation, CHANGELOG (file), release finalisation (CHANGELOG + semver tag + merge) | Merge PRs unilaterally; push to remote; write application code; act without human request |
| `spex-qa` | `subagent` | Test plans (MCP), test source files, gate sign-off, security review & threat modelling (Security Review Mode) | Write production application code; commit test plan or security review docs |

---

## SKILL.md and Agent Config Duality

Each bundled agent ships **two paired files**:

| File | Location (installed) | Purpose |
|------|---------------------|---------|
| `SKILL.md` | `~/.config/opencode/skills/<name>/SKILL.md` | Behavioural instructions loaded on-demand via the `skill` tool |
| `<name>.md` | `~/.config/opencode/agents/<name>.md` | OpenCode agent config: mode, tools, permissions, temperature, system prompt |

### SKILL.md Frontmatter (normative)

OpenCode recognises **only** these fields in a `SKILL.md`:

```yaml
---
name: "spex-xxx"              # required; must match directory name
description: "..."           # required; 1–1024 chars
license: "MIT"               # optional
compatibility: "opencode"    # optional
metadata:                    # optional; string-to-string map
  key: value
---
```

All other fields (e.g. `version`, `compatible_with`) are **silently ignored** by OpenCode.
Do not add them.

### Agent Config Frontmatter (normative)

```yaml
---
description: "..."           # required; shown in @ autocomplete
mode: primary|subagent|all   # default: all
temperature: 0.1             # optional
tools:
  write: false               # disable specific tools
  edit: false
  bash: false
permission:
  edit: deny                 # allow | deny | ask
  bash:
    "*": ask                 # wildcard glob; last match wins
    "git status": allow
  task:
    "*": allow               # which subagents this agent may invoke
---
Brief prompt instructing the agent to load its skill before acting.
```

### Installation

Use `spex skill install --all` to install bundled skills and agent
configs in a single step. Bundled source files live in:

- `skills/<name>/SKILL.md`
- `agents/<name>.md`
