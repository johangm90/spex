# spex

> **Spec-Driven Development for AI-assisted teams.**  
> Define specs, coordinate agents, track progress — all from your terminal.

[![Build](https://img.shields.io/badge/build-passing-brightgreen?style=flat-square)](https://github.com/johangm90/spex)
[![Version](https://img.shields.io/badge/version-0.1.0-blue?style=flat-square)](Cargo.toml)
[![License](https://img.shields.io/badge/license-MIT-lightgrey?style=flat-square)](LICENSE)
[![Rust](https://img.shields.io/badge/rust-stable-orange?style=flat-square)](https://www.rust-lang.org)

---

## Table of Contents

- [What is spex?](#what-is-spex)
- [Prerequisites](#prerequisites)
- [Installation](#installation)
- [Quick Start](#quick-start)
- [Spec Lifecycle](#spec-lifecycle)
- [Commands Reference](#commands-reference)
  - [Project Bootstrap](#project-bootstrap)
  - [Constitution (Law)](#constitution-law)
  - [Specs](#specs)
  - [Plans](#plans)
  - [Tasks](#tasks)
  - [Pulse](#pulse)
  - [Trace](#trace)
  - [MCP Server](#mcp-server)
  - [Skills](#skills)
  - [Doctor](#doctor)
- [Architecture](#architecture)
- [Project Structure](#project-structure)
- [MCP Tools Reference](#mcp-tools-reference)
  - [Memory Tools](#memory-tools)
- [Bundled Agent Skills](#bundled-agent-skills)
- [Database Schema](#database-schema)
- [Contributing / Development](#contributing--development)
- [License](#license)

---

## What is spex?

`spex` is a **Spec-Driven Development (SDD) CLI** written in Rust. It gives human developers and AI agents a shared, persistent state store for coordinated feature delivery.

The core ideas:

1. **Specs are the unit of work.** Every feature is a *spec* — a named slice with a defined lifecycle (`draft → approved → in_progress ⇄ paused → done`). Human approval is a first-class gate before any agent begins work.

2. **Agents share state via MCP.** `spex` runs an embedded **MCP (Model Context Protocol)** JSON-RPC server over stdio. OpenCode agents call its tools to read and write specs, tasks, events, memory, and artifacts — all stored in a local SQLite database at `.spex/state.db`.

3. **Skills are bundled and installed.** `spex` ships 10 specialised AI agent skill files (`SKILL.md`) and agent prompt files for the full `spex-*` agent team. One command installs them into `~/.config/opencode/`.

### The workflow at a glance

```
You write a Constitution (project PRD)
  └─► You add Specs and approve them
        └─► spex-orchestrate decomposes each Spec into Tasks
              └─► Specialist agents (backend, frontend, qa, …) execute Tasks
                    └─► Agents write Events, Memory, Artifacts to spex MCP state
                          └─► You review Pulse and promote Specs to done
```

---

## Prerequisites

| Requirement | Version | Notes |
|---|---|---|
| [Rust](https://rustup.rs) | stable (≥ 1.75) | `rustup update stable` |
| [OpenCode](https://opencode.ai) | latest | AI coding assistant that hosts agents |
| SQLite | 3.x | Bundled via `sqlx`; no separate install needed |
| `$EDITOR` | any | Used by `spex law edit`; defaults to `nano` |

---

## Installation

### From source (recommended)

```bash
git clone https://github.com/johangm90/spex.git
cd spex
cargo install --path .
```

Verify:

```bash
spex --version
```

### Pre-built binary *(coming soon)*

Pre-built binaries for Linux x86\_64, macOS arm64, and macOS x86\_64 will be available on the [Releases](https://github.com/johangm90/spex/releases) page.

---

## Quick Start

### Step 1 — Create a new project

```bash
spex new my-project
cd my-project
```

This scaffolds:
- `README.md`
- `.gitignore` (with `.spex/state.db` excluded)
- `docs/specs/` and `docs/adr/`
- `opencode.json` (MCP config pre-wired to `spex mcp serve`)
- `.spex/state.db` (SQLite database, auto-migrated)
- `.spex/constitution.md` (editable template)

> **Existing project?** Run `spex init` instead — it merges into your existing `opencode.json` and never overwrites existing files.

### Step 2 — Write your Constitution

The Constitution is your project's living PRD. It constrains every agent.

```bash
spex law edit        # opens $EDITOR with .spex/constitution.md
spex law show        # preview the stored constitution
```

The template provides sections for Vision, Goals, Non-Goals, Tech Stack, Architecture Principles, and Acceptance Standards.

### Step 3 — Install agent skills

```bash
spex skill install --all
```

This writes 16 `spex-*` skill files to `~/.config/opencode/skills/` and matching agent prompts to `~/.config/opencode/agents/`. All files are embedded in the binary — no network access required.

### Step 4 — Add and approve your first spec

```bash
spex spec add SPEC-001 "User authentication" -p P0
spex spec approve SPEC-001          # human gate — required before agents can start
spex plan build SPEC-001            # interactive: enter tasks one-by-one
```

### Step 5 — Open OpenCode and monitor the pulse

```bash
spex pulse          # project status dashboard
```

Open OpenCode in the project directory. The `spex-state` MCP server starts automatically via `opencode.json`. Ask `spex-orchestrate` to begin work on `SPEC-001`.

---

## Spec Lifecycle

```
           ┌──────────┐
  create   │          │
 ─────────►│  draft   │
           │          │
           └────┬─────┘
                │ spex spec approve  (human gate)
                ▼
           ┌──────────┐
           │          │
           │ approved │
           │          │
           └────┬─────┘
                │ spex spec start
                ▼
           ┌────────────┐   pause    ┌────────┐
           │            │───────────►│        │
           │ in_progress│            │ paused │
           │            │◄───────────│        │
           └────┬───────┘   resume  └────────┘
                │ spex spec done
                ▼
           ┌──────────┐
           │          │
           │   done   │
           │          │
           └──────────┘
```

Valid transitions enforced by the state machine:

| From | To | Command / Agent action |
|---|---|---|
| `draft` | `approved` | `spex spec approve <ID>` |
| `approved` | `in_progress` | `spex spec start <ID>` or agent via MCP |
| `in_progress` | `done` | `spex spec done <ID>` or agent via MCP |
| `in_progress` | `paused` | agent via MCP `state_spec_update` |
| `paused` | `in_progress` | agent via MCP `state_spec_update` |

All other transitions return an error. There is no back-transition from `done`.

---

## Commands Reference

### Project Bootstrap

#### `spex new <NAME>`

Bootstrap a brand-new `spex` project in a new subdirectory.

```bash
spex new my-app
```

| Flag | Description |
|---|---|
| `--yes` / `-y` | Skip confirmation prompts *(stub — see [IMP-002](docs/IMPROVEMENTS.md))* |

Creates: project directory, `.spex/`, `README.md`, `.gitignore`, `docs/specs/`, `docs/adr/`, `opencode.json`, `.spex/state.db`, `.spex/constitution.md`.

#### `spex init`

Initialise `spex` in the **current** directory (for existing projects).

```bash
cd existing-project
spex init
```

Safe to run on an existing repository — never overwrites existing files. Appends `spex` entries to `.gitignore` and merges MCP entries into `opencode.json`.

---

### Constitution (Law)

The Constitution is a Markdown document (`.spex/constitution.md`) that is synced to `.spex/state.db`. It represents the project PRD — vision, goals, tech stack, and acceptance standards. All agents read it via `state_constitution_get` on startup.

Constitution statuses: `draft` → `active` → `frozen`

#### `spex law init`

Create the constitution template file and initialise the database record.

```bash
spex law init
```

#### `spex law edit`

Open the constitution in `$EDITOR` (defaults to `nano`) and sync the saved content to the database.

```bash
spex law edit
```

> ⚠ **Known issue:** A frozen constitution can currently still be edited. This is tracked as [IMP-001](docs/IMPROVEMENTS.md) and will be fixed in the next release.

#### `spex law show`

Print the full constitution to stdout with status and version metadata.

```bash
spex law show
```

#### `spex law freeze`

Permanently lock the constitution. Requires interactive confirmation. Cannot be undone.

```bash
spex law freeze
# ⚠  This will permanently freeze the Constitution.
#    No further edits will be allowed after freezing.
# Are you sure you want to freeze the Constitution? (y/N)
```

---

### Specs

Specs are the primary unit of work — named feature slices that progress through a defined lifecycle.

#### `spex spec add <ID> <TITLE> [-p PRIORITY]`

Create a new spec in `draft` status.

```bash
spex spec add SPEC-001 "User authentication"
spex spec add SPEC-002 "Payment flow" -p P0
```

| Argument | Description |
|---|---|
| `ID` | Unique spec identifier (e.g. `SPEC-001`) |
| `TITLE` | Human-readable description |
| `-p` / `--priority` | `P0` `P1` *(default)* `P2` `P3` |

#### `spex spec approve <ID>`

Approve a spec — the **human gate** that enables agent work. Emits a `SpecApproved` event.

```bash
spex spec approve SPEC-001
```

#### `spex spec start <ID>`

Transition from `approved` to `in_progress`. Emits `SpecStarted`.

```bash
spex spec start SPEC-001
```

#### `spex spec done <ID>`

Mark a spec complete. Emits `SpecCompleted`.

```bash
spex spec done SPEC-001
```

#### `spex spec list [--json]`

List all specs in a table, sorted by ID.

```bash
spex spec list
spex spec list --json
```

Columns: `ID`, `Title`, `Status`, `Priority`, `AC` (acceptance criteria `passed/total`).

#### `spex spec show <ID>`

Show full spec details including status, priority, AC progress, agents, dependencies, and all tasks.

```bash
spex spec show SPEC-001
```

---

### Plans

Plans decompose a spec into an ordered list of tasks, each assigned to a specific agent.

#### `spex plan build <SPEC_ID>`

Interactively enter tasks for a spec. Prompts for task ID, title, agent, inputs, and output artifact for each task. Leave ID blank to finish.

```bash
spex plan build SPEC-001
```

> **Tip:** For non-interactive / bulk task creation, use `spex task add` in a script, or have an agent call `state_task_create` via MCP.

#### `spex plan show <SPEC_ID>`

Display the current task list for a spec.

```bash
spex plan show SPEC-001
```

---

### Tasks

Individual units of work within a spec, each owned by a named agent.

#### `spex task add <SPEC_ID> <TASK_ID> <TITLE> --agent <AGENT>`

Add a single task non-interactively.

```bash
spex task add SPEC-001 T001-1 "Design data model" --agent spex-db
spex task add SPEC-001 T001-2 "Implement REST API" --agent spex-backend \
  --inputs T001-1 \
  --output-artifact SPEC-001-API-SPEC
```

| Flag | Description |
|---|---|
| `--agent` | Agent name (required; e.g. `spex-backend`) |
| `--inputs` | Input artifact IDs (repeatable) |
| `--output-artifact` | Output artifact ID this task will produce |

#### `spex task start <ID>`

Mark a task as `in_progress`.

```bash
spex task start T001-1
```

#### `spex task done <ID>`

Mark a task as `done`.

```bash
spex task done T001-1
```

#### `spex task fail <ID>`

Mark a task as `failed`.

```bash
spex task fail T001-1
```

#### `spex task list [SPEC_ID] [--json]`

List tasks, optionally filtered by spec.

```bash
spex task list                  # all tasks
spex task list SPEC-001         # tasks for SPEC-001 only
spex task list --json           # JSON output
spex task list SPEC-001 --json
```

Task statuses: `pending` → `in_progress` → `done` | `failed`

---

### Pulse

#### `spex pulse`

Display a rich project status dashboard showing:
- Constitution status and version
- Spec counts by status (draft / approved / in\_progress / done / paused)
- Per-spec ASCII progress bar with task completion ratio
- Last 5 domain events

```bash
spex pulse
```

```
╔══════════════════════════════════════════════╗
║              spex — project pulse            ║
╚══════════════════════════════════════════════╝

  ⚖ Constitution — active v3

  ■ Specs

  1 draft  2 approved  1 in_progress  3 done  0 paused

  SPEC-001    [████████████████░░░░] in_progress  16/20 tasks
  SPEC-002    [████████████████████] done          8/8 tasks
  SPEC-003    [░░░░░░░░░░░░░░░░░░░░] approved      0/0 tasks

  ▶ Recent Activity

  2026-03-05 14:22  TaskCompleted    spex-backend  [SPEC-001]
  2026-03-05 14:20  TaskStarted      spex-qa       [SPEC-001]
```

---

### Trace

#### `spex trace [--spec X] [--agent Y] [--limit N]`

Show the domain event log — an append-only audit trail of all project activity.

```bash
spex trace                              # last 50 events
spex trace --spec SPEC-001              # events for SPEC-001
spex trace --agent spex-backend         # events from spex-backend
spex trace --limit 20                   # last 20 events
spex trace --spec SPEC-001 --limit 10
```

Output columns: `Timestamp`, `Type`, `Spec`, `Agent`, `Payload`

---

### MCP Server

The MCP server is the communication bridge between OpenCode agents and `spex`'s shared state. It implements [JSON-RPC 2.0](https://www.jsonrpc.org/specification) over stdio.

#### `spex mcp serve`

Start the MCP stdio server. Invoked automatically by OpenCode via `opencode.json`; rarely needed to run manually.

```bash
spex mcp serve
```

The server advertises itself as `spex-state` version `0.1.0` and supports MCP protocol version `2024-11-05`.

#### `spex mcp setup [--global]`

Write or merge the MCP config into `opencode.json` (local) or `~/.config/opencode/config.json` (global).

```bash
spex mcp setup              # writes to ./opencode.json
spex mcp setup --global     # writes to ~/.config/opencode/config.json
```

Generated `opencode.json` entry:

```json
{
  "mcp": {
    "spex-state": {
      "command": "spex",
      "args": ["mcp", "serve"]
    }
  }
}
```

---

### Skills

#### `spex skill install [--all]`

Install bundled agent skills and agent prompt files to the OpenCode config directory.

```bash
spex skill install --all
```

Installs to:
- `~/.config/opencode/skills/spex-*/SKILL.md` — skill instruction files (10 agents)
- `~/.config/opencode/agents/spex-*.md` — agent prompt files (10 agents)
- `~/.config/opencode/skills/_shared/conventions.md` — shared agent conventions

All files are **embedded in the `spex` binary** at compile time via `include_dir!` — no internet access required.

#### `spex skill list`

List all installed `spex-*` skills.

```bash
spex skill list
# Installed skills:
#   • spex-architect
#   • spex-backend
#   • spex-orchestrate
#   ...
```

---

### Doctor

#### `spex doctor [--fix]`

Run 7 health checks and report the project's configuration status.

```bash
spex doctor
spex doctor --fix       # (--fix not yet implemented — see IMP-005)
```

| # | Check | Pass condition |
|---|---|---|
| 1 | **State DB** | `.spex/state.db` exists and is readable |
| 2 | **Constitution** | Constitution record exists and is not an empty draft |
| 3 | **Skills dir** | `~/.config/opencode/skills/` directory exists |
| 4 | **Skills installed** | At least one `spex-*` skill is installed |
| 5 | **opencode.json** | File exists and contains a `spex-state` MCP entry |
| 6 | **Git repo** | A `.git` directory is found anywhere in the tree |
| 7 | **Stuck specs** | No specs are stuck in `in_progress` |

Exits with code `1` if any check fails.

---

## Architecture

```
┌──────────────────────────────────────────────────────────────────────┐
│  Terminal / Human                                                      │
│                                                                        │
│  spex pulse / spex spec add / spex law edit / spex trace / ...        │
└───────────────────────────┬────────────────────────────────────────────┘
                            │ Rust CLI (clap 4)
                            ▼
┌──────────────────────────────────────────────────────────────────────┐
│  src/cli/                                                              │
│  law.rs · spec.rs · plan.rs · task.rs · pulse.rs · trace.rs           │
│  mcp_cmd.rs · skill_cmd.rs · doctor.rs                                │
└───────────────────────────┬────────────────────────────────────────────┘
                            │
                            ▼
┌──────────────────────────────────────────────────────────────────────┐
│  src/sdd/  (domain models + SQLite CRUD, sqlx 0.8)                    │
│                                                                        │
│  spec.rs · task.rs · constitution.rs · event.rs · memory.rs           │
│  artifact.rs · db.rs                                                   │
└───────────────────────────┬────────────────────────────────────────────┘
                            │ sqlx async queries
                            ▼
┌──────────────────────────────────────────────────────────────────────┐
│  .spex/state.db  (SQLite — auto-migrated on startup)                  │
│                                                                        │
│  constitution · specs · tasks · events · memory · artifacts · meta    │
└──────────────────────────────────────────────────────────────────────┘
                            ▲
                            │ JSON-RPC 2.0 over stdio (MCP protocol)
┌──────────────────────────────────────────────────────────────────────┐
│  src/mcp/server.rs                                                     │
│                                                                        │
│  initialize · tools/list · tools/call                                  │
│  → dispatch_tool() → sdd domain functions                             │
└───────────────────────────┬────────────────────────────────────────────┘
                            │ opencode.json: "spex mcp serve"
                            ▼
┌──────────────────────────────────────────────────────────────────────┐
│  OpenCode  (AI coding assistant, runs agent sessions)                 │
│                                                                        │
│  spex-orchestrate  ──► decomposes specs into tasks                    │
│  spex-architect    ──► defines ADRs and bounded contexts              │
│  spex-backend      ──► implements server-side code                    │
│  spex-frontend     ──► implements web UI and design specs             │
│  spex-qa           ──► writes tests and verifies acceptance criteria  │
│  spex-db           ──► designs schemas and migrations                 │
│  spex-devops       ──► manages CI/CD and infrastructure               │
│  spex-gitops       ──► manages commits, branches, and PRs             │
│  spex-mobile       ──► builds mobile apps                             │
│  spex-ai-eng       ──► integrates LLMs and AI features                │
└──────────────────────────────────────────────────────────────────────┘
```

### Key data flows

| Flow | Description |
|---|---|
| Human → CLI → SQLite | Direct CRUD: `spex spec add`, `spex task done`, etc. |
| Agent → MCP → SQLite | Agents call `state_*` tools via JSON-RPC to read/write state |
| CLI → `~/.config/opencode/` | `spex skill install` writes embedded skill files to disk |
| `opencode.json` → MCP | OpenCode reads config and spawns `spex mcp serve` as a subprocess |

---

## Project Structure

```
spex/
├── src/
│   ├── main.rs                     # CLI entry point, clap command tree
│   ├── cli/                        # Command handler functions
│   │   ├── mod.rs
│   │   ├── law.rs                  # spex law {init,edit,show,freeze}
│   │   ├── spec.rs                 # spex spec {add,approve,start,done,list,show}
│   │   ├── plan.rs                 # spex plan {build,show}
│   │   ├── task.rs                 # spex task {add,start,done,fail,list}
│   │   ├── pulse.rs                # spex pulse
│   │   ├── trace.rs                # spex trace
│   │   ├── mcp_cmd.rs              # spex mcp {serve,setup}
│   │   ├── skill_cmd.rs            # spex skill {install,list}
│   │   └── doctor.rs               # spex doctor
│   ├── sdd/                        # Domain models + SQLite CRUD
│   │   ├── mod.rs
│   │   ├── db.rs                   # DB open/init, project root discovery
│   │   ├── spec.rs                 # Spec model, validate_transition, CRUD
│   │   ├── task.rs                 # Task model, CRUD
│   │   ├── constitution.rs         # Constitution model, freeze logic
│   │   ├── event.rs                # Append-only event log
│   │   ├── memory.rs               # Per-agent KV scratchpad
│   │   └── artifact.rs             # Output artifact registry
│   ├── mcp/
│   │   ├── mod.rs
│   │   └── server.rs               # JSON-RPC 2.0 stdio server + tool dispatch
│   ├── scaffold/
│   │   └── mod.rs                  # spex new / spex init logic
│   ├── doctor/
│   │   └── mod.rs                  # 7 health checks
│   └── skills_mgr/
│       └── mod.rs                  # include_dir! embed + install logic
├── skills/                         # Bundled SKILL.md files (embedded in binary)
│   ├── _shared/
│   │   └── conventions.md
│   └── spex-{architect,backend,frontend,mobile,db,devops,
│              qa,gitops,ai-eng,orchestrate}/
│       └── SKILL.md
├── agents/                         # Bundled agent prompt .md files
│   └── spex-*.md
├── migrations/                     # SQLite schema migrations (sqlx migrate!)
│   ├── 20240101000000_initial.sql
│   └── 20260305000000_memory_scope.sql
├── docs/
│   ├── specs/                      # Spec documents (Markdown, tracked in git)
│   ├── adr/                        # Architecture Decision Records
│   └── IMPROVEMENTS.md             # Prioritised improvement plan
├── .spex/                          # Runtime state (gitignored)
│   ├── state.db                    # SQLite database
│   └── constitution.md             # Editable constitution file
├── Cargo.toml
├── Cargo.lock
├── build.rs
├── opencode.json                   # MCP config (spex-state → spex mcp serve)
└── .gitignore                      # Excludes .spex/state.db
```

---

## MCP Tools Reference

All tools are invoked via `tools/call` in the MCP JSON-RPC protocol. The `state_*` names are canonical; legacy aliases (`spec_*`, `slice_*`, `task_*`, `event_*`) are preserved for backwards compatibility but not recommended for new skill files.

| Tool (canonical) | Description | Required | Optional |
|---|---|---|---|
| `state_snapshot` | Full project overview: constitution, all specs, all tasks, 10 recent events | — | — |
| `state_spec_get` | Get a spec by ID, or list all specs | — | `id` |
| `state_spec_create` | Create a new spec in `draft` status | `id`, `title` | `priority`, `depends_on[]`, `agents[]` |
| `state_spec_update` | Update spec status, AC counts, or agents | `id` | `status`, `ac_total`, `ac_passed`, `agents[]`, `updated_by` |
| `state_task_get` | Get a task by ID, or list tasks | — | `id`, `spec` |
| `state_task_create` | Create a new task within a spec | `id`, `spec`, `title`, `agent` | `inputs[]`, `output_artifact` |
| `state_task_update` | Update task status or output artifact | `id` | `status`, `output_artifact` |
| `state_event_emit` | Emit a domain event to the append-only log | `type` | `spec`, `agent`, `payload` |
| `state_event_query` | Query the event log with filters | — | `type`, `spec`, `agent`, `limit`, `since` |
| `memory_set` | Store or update a value in agent memory | `agent`, `key`, `value` | `spec`, `type`, `ttl_seconds` |
| `memory_get` | Get a value or all entries for an agent | `agent` | `key`, `spec` |
| `memory_search` | Full-text search across memory entries (FTS5) | `agent`, `query` | `spec`, `type`, `limit` |
| `memory_delete` | Soft-delete a memory entry | `agent`, `key` | `spec` |
| `memory_context` | Return most recently accessed entries for session recovery | `agent` | `spec`, `limit` |
| `memory_stats` | Return aggregate statistics for agent memory | `agent` | `spec` |
| `artifact_register` | Register an output artifact | `id`, `spec`, `agent`, `type` | `task`, `path`, `description` |
| `artifact_query` | Query registered artifacts | — | `spec`, `task`, `agent`, `type` |
| `state_constitution_get` | Get the project constitution | — | — |

**Legacy aliases** accepted by `dispatch_tool` (but not recommended for new skill files):  
`spec_get`, `spec_create`, `spec_update`, `slice_get`, `slice_create`, `slice_update`, `state_slice_get`, `state_slice_create`, `state_slice_update`, `task_get`, `task_create`, `task_update`, `event_emit`, `event_query`, `constitution_get`

---

## Memory Tools

The six `memory_*` tools give agents a persistent, scoped, searchable scratchpad backed by the `memory` table in `.spex/state.db`. Entries are unique on `(agent, spec, key)` — upserts are idempotent. All reads filter out soft-deleted and TTL-expired entries automatically.

### `memory_set`

Store or update a value in agent memory.

| Parameter | Type | Required | Description |
|-----------|------|----------|-------------|
| `agent` | string | ✅ | Agent identifier (e.g. `spex-backend`) |
| `key` | string | ✅ | Memory key — unique within `(agent, spec)` |
| `value` | string | ✅ | Value to store (any string; JSON recommended for structured data) |
| `spec` | string | | Scope entry to a specific spec ID; omit for global agent memory |
| `type` | string | | Entry type: `decision` \| `architecture` \| `bugfix` \| `pattern` \| `config` \| `discovery` \| `learning` |
| `ttl_seconds` | integer | | If set, entry is automatically hidden after this many seconds |

Behaviour: if an entry with the same `(agent, spec, key)` already exists it is updated in place; `revision_count` is incremented on every update.

---

### `memory_get`

Get a single value or list all entries for an agent.

| Parameter | Type | Required | Description |
|-----------|------|----------|-------------|
| `agent` | string | ✅ | Agent identifier |
| `key` | string | | Key to retrieve; omit to return all entries for the agent |
| `spec` | string | | Scope to a specific spec ID |

Returns: the stored value string when `key` is given; a key→value map when `key` is omitted. Expired and deleted entries are excluded. Each successful `key` lookup bumps `access_count` and `last_accessed_at`.

---

### `memory_search`

Full-text search across memory entries using SQLite FTS5.

| Parameter | Type | Required | Description |
|-----------|------|----------|-------------|
| `agent` | string | ✅ | Agent identifier |
| `query` | string | ✅ | FTS5 query string (e.g. `"sqlite persistence"`) |
| `spec` | string | | Restrict search to a specific spec scope |
| `type` | string | | Restrict search to a specific entry type |
| `limit` | integer | | Maximum number of results (default: 10) |

Returns: list of matching `Memory` objects sorted by FTS5 relevance rank. Deleted and expired entries are excluded.

---

### `memory_delete`

Soft-delete a memory entry.  The row is marked with a `deleted_at` timestamp and is immediately invisible to all read operations; it is not physically removed.

| Parameter | Type | Required | Description |
|-----------|------|----------|-------------|
| `agent` | string | ✅ | Agent identifier |
| `key` | string | ✅ | Key to delete |
| `spec` | string | | Scope to a specific spec ID (must match the scope used at insert time) |

Returns: `true` if a row was affected; `false` if no matching active entry was found.

---

### `memory_context`

Return the most recently accessed memory entries for session recovery.  Ordered by `last_accessed_at DESC` then `access_count DESC`.

| Parameter | Type | Required | Description |
|-----------|------|----------|-------------|
| `agent` | string | ✅ | Agent identifier |
| `spec` | string | | Restrict to a specific spec scope |
| `limit` | integer | | Maximum entries to return (default: 10) |

Returns: list of `Memory` objects representing the agent's most recently touched entries.  Use this on startup to quickly restore context without a full scan.

---

### `memory_stats`

Return aggregate statistics for an agent's memory store.

| Parameter | Type | Required | Description |
|-----------|------|----------|-------------|
| `agent` | string | ✅ | Agent identifier |
| `spec` | string | | Scope statistics to a specific spec |

Returns a JSON object with:

| Field | Type | Description |
|-------|------|-------------|
| `total` | integer | Total number of active (non-deleted, non-expired) entries |
| `by_type` | object | Count per entry type; entries with no type appear under `"untyped"` |
| `most_accessed_key` | string \| null | Key with the highest `access_count` |
| `last_written_at` | string \| null | ISO-8601 timestamp of the most recently updated entry |

---

## Bundled Agent Skills

All 10 skills are `spex-*` prefixed, embedded in the binary, and installed via `spex skill install --all`.

| Agent | Specialisation |
|---|---|
| `spex-architect` | Bounded contexts, slice specs, Architecture Decision Records; includes Product Discovery mode |
| `spex-orchestrate` | Spec decomposition, task delegation, agent team coordination |
| `spex-backend` | Server-side implementation, APIs, business logic |
| `spex-frontend` | Web UI implementation (React, Vue, etc.); includes design tokens and wireframes |
| `spex-mobile` | React Native / Flutter apps, native modules, push notifications |
| `spex-db` | Schema design, ERDs, migration strategies |
| `spex-devops` | Containers, CI/CD pipelines, infrastructure runbooks |
| `spex-qa` | Test plans, verification checklists, spec promotion gates; includes security review |
| `spex-gitops` | Conventional commits, branch policy, PRs, CHANGELOG entries; includes release finalisation |
| `spex-ai-eng` | LLM integration, RAG pipelines, vector DBs, prompt engineering |

Each skill reads the project Constitution via `state_constitution_get` and calls `state_snapshot` on startup to orient itself within the current project context.

---

## Database Schema

`spex` stores all persistent state in `.spex/state.db` (SQLite). The database is created automatically on `spex new` or `spex init` and is excluded from version control via `.gitignore`. Schema migrations are applied automatically via `sqlx::migrate!`.

| Table | Purpose |
|---|---|
| `constitution` | Project PRD — one record per project; `status`: `draft` / `active` / `frozen`; versioned |
| `specs` | Feature slices — `status` state machine, `priority` (P0–P3), JSON `depends_on` and `agents` arrays, AC counters |
| `tasks` | Tasks within a spec — `status`: `pending` / `in_progress` / `done` / `failed`; JSON `inputs` array |
| `events` | Append-only domain event log — indexed by `type`, `spec`, `agent`, `timestamp` |
| `memory` | Per-agent KV scratchpad — unique on `(agent, spec, key)`; last-write-wins; spec-scoped |
| `artifacts` | Registered output artifacts — linked to `spec`, `task`, and `agent`; indexed by `type` |
| `meta` | Project-level KV metadata |

---

## Contributing / Development

### Setup

```bash
git clone https://github.com/johangm90/spex.git
cd spex
cargo build
cargo test
```

### Code quality

```bash
cargo clippy -- -D warnings    # lint
cargo fmt                      # format
cargo fmt --check              # format check (CI)
```

### Run the CLI locally during development

```bash
cargo run -- pulse
cargo run -- spec list
cargo run -- doctor
cargo run -- mcp serve         # MCP server reads JSON-RPC from stdin
```

### Adding a new CLI command

1. Add the variant to the relevant `*Cmd` enum in `src/main.rs`
2. Add a `cmd_<name>` handler function in `src/cli/<group>.rs`
3. Wire the dispatch in the corresponding `match` arm in `src/main.rs`

### Adding a new SDD domain operation

1. Implement the DB function in `src/sdd/<entity>.rs`
2. Re-export it from `src/sdd/mod.rs` if consumed outside the module
3. Add a `tools/call` handler in `dispatch_tool()` in `src/mcp/server.rs`
4. Add the tool schema entry in `build_tools_list()` in `src/mcp/server.rs`

### Adding or updating a bundled skill

1. Edit `skills/spex-<name>/SKILL.md` or `agents/spex-<name>.md`
2. Rebuild (`cargo build`) — files are embedded at compile time via `include_dir!`
3. Re-run `spex skill install --all` in your test project to pick up the new files

### Known issues and improvement plan

See [`docs/IMPROVEMENTS.md`](docs/IMPROVEMENTS.md) for the full prioritised backlog including P0 bugs.

---

## License

This project is licensed under the [MIT License](LICENSE).

---

*Built with Rust · sqlx · clap · tokio · MCP*
