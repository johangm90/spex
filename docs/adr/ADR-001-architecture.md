# ADR-001: Core Architecture of spex

Date: 2026-03-05
Status: Accepted
Deciders: core team

---

## Context and Problem Statement

`spex` is a Spec-Driven Development (SDD) command-line tool. It serves two primary audiences:

1. **Human engineers** — who use the CLI to manage project specs, tasks, events, and agent memory across a team.
2. **LLM agents** (running inside OpenCode) — which call `spex` as an MCP server to read and mutate shared project state without leaving their execution environment.

The tool must work completely offline, install as a single binary with no external dependencies, and integrate seamlessly with OpenCode's MCP client protocol. This document records the significant architectural decisions made during the v0.1.0 design phase.

---

## Decision Drivers

- **Zero infrastructure** — a developer must be able to clone a repo and run `spex` with no Docker, no databases, no cloud accounts.
- **Version-locked skills** — embedded agent skill files must stay in sync with the binary that ships them; they must not drift via independent updates.
- **LLM-agent compatibility** — the MCP interface must be callable by an LLM agent via the stdio transport that OpenCode already supports.
- **Developer ergonomics** — SQL errors should surface at build time, not in production; CLI commands should be self-documenting.
- **Auditability** — all agent state changes must be traceable; spec lifecycle transitions must follow a defined sequence.

---

## Considered Options and Decision Outcomes

### 1. Persistence Layer — SQLite over a Remote Database

#### Options Considered

| Option | Description |
|--------|-------------|
| **SQLite (chosen)** | Embedded file-based RDBMS; stored at `.spex/state.db` |
| PostgreSQL | Remote relational DB; requires a running server or cloud account |
| JSON flat files | Plain JSON files in `.spex/`; no query capability |
| LMDB / RocksDB | Embedded key-value stores; good performance, poor query ergonomics |

#### Decision Outcome

**SQLite** via the `sqlx` crate (async, compile-time checked).

The `.spex/state.db` file lives inside the project directory alongside source code. This means state is local-first and can optionally be committed to version control. There is no network dependency and no server to provision.

The schema currently comprises seven tables: `constitution`, `specs`, `tasks`, `events`, `memory`, `artifacts`, and `meta`. Migrations are applied automatically at startup via `sqlx::migrate!`.

#### Consequences

- ✅ Works completely offline; zero external infrastructure.
- ✅ State travels with the repo — a `git clone` restores history.
- ✅ SQL queries are compile-time checked (`DATABASE_URL` must be set during `cargo build`).
- ⚠️ Concurrent write throughput is limited by SQLite's single-writer model.
- ⚠️ WAL mode is not yet enabled; high-frequency agent writes may cause lock contention. See **IMP-016** in `docs/IMPROVEMENTS.md`.

---

### 2. MCP Transport — Stdio (JSON-RPC 2.0 over stdin/stdout)

#### Options Considered

| Option | Description |
|--------|-------------|
| **Stdio JSON-RPC (chosen)** | OpenCode spawns `spex mcp serve`; communicates over stdin/stdout |
| HTTP server | `spex` binds a port and serves HTTP; OpenCode connects as an HTTP MCP client |
| Unix domain socket | IPC via a local socket path; avoids port conflicts |
| gRPC | Binary protocol; excellent for performance but requires protobuf and extra tooling |

#### Decision Outcome

**Stdio JSON-RPC 2.0** (`spex mcp serve`).

OpenCode's MCP client natively supports the stdio transport. Launching `spex mcp serve` as a child process avoids port-allocation conflicts, removes the need for TLS or authentication, and keeps the tool stateless between MCP sessions (state lives in SQLite, not in process memory).

The MCP server dispatches **14 canonical tools** (`state_snapshot`, `spec_get`, `spec_create`, `spec_update`, `task_get`, `task_create`, `task_update`, `event_emit`, `event_query`, `memory_set`, `memory_get`, `artifact_register`, `artifact_query`, `constitution_get`), plus **13 legacy alias entries** bringing the total registered count to 27. Aliases allow agents that were trained on earlier `slice_*` or `spec_*` prefixes to continue working without changes. See **IMP-008** in `docs/IMPROVEMENTS.md` for the technical debt note on eventual alias retirement.

#### Consequences

- ✅ Zero port conflicts; no firewall rules needed.
- ✅ No authentication surface — the process is owned by the same user.
- ✅ Works out of the box with OpenCode's default MCP configuration.
- ⚠️ A single MCP session is single-threaded by the stdio pipe; parallel agent calls are serialised.
- ⚠️ Legacy aliases (IMP-008) represent technical debt: 13 duplicate registrations that must be maintained until all agents are retrained.

---

### 3. Agent Skills Distribution — Embedded Binary Assets

#### Options Considered

| Option | Description |
|--------|-------------|
| **`include_dir!` at build time (chosen)** | Skills compiled into the binary; `spex skills install` extracts them |
| Downloaded at install time | Installer script fetches skills from a URL at `spex init` |
| Git submodule | Skills live in a submodule; user must init submodules after clone |
| Separate package | Skills distributed as an independent npm/crate/pip package |

#### Decision Outcome

**`include_dir!` macro** (from the `include_dir` crate), evaluated at compile time.

Sixteen agent skill directories plus shared resources are embedded directly into the `spex` binary. The `build.rs` build script verifies the assets directory exists at compile time so a broken asset path fails the build rather than producing a silent runtime error. When the user runs `spex skills install`, the embedded tree is extracted to `~/.config/opencode/skills/`.

#### Consequences

- ✅ Offline installation — no internet access required after `cargo install spex`.
- ✅ Skills are always version-locked to the binary; no drift between tool behaviour and agent prompts.
- ✅ Single binary distribution; no sidecar files.
- ⚠️ Binary size increases with every new skill (currently ~16 skill directories).
- ⚠️ Updating a skill requires a new binary release; there is no hot-reload path.

---

### 4. CLI Framework — Clap v4 with Derive Macros

#### Options Considered

| Option | Description |
|--------|-------------|
| **Clap v4 derive (chosen)** | Declarative structs/enums; subcommands map to types |
| Clap v4 builder API | Imperative builder pattern; more verbose |
| `argh` | Lightweight derive-based parser; smaller feature set |
| `structopt` | Predecessor to clap derive; effectively superseded |

#### Decision Outcome

**Clap v4 with derive macros** (`#[derive(Parser, Subcommand, Args)]`).

Each subcommand is a Rust enum variant holding an `Args` struct. Handler functions receive typed, validated arguments. `--help` text is generated automatically from doc-comments. The derive approach eliminates boilerplate and keeps command definitions co-located with their argument types.

#### Consequences

- ✅ Single source of truth for argument parsing and help text.
- ✅ Compile-time validation of argument constraints.
- ✅ Adding a new subcommand requires only a new enum variant and handler function.
- ⚠️ Clap v4 is a heavy dependency; compile times are non-trivial.

---

### 5. SQL Safety — sqlx Compile-Time Query Checking

#### Options Considered

| Option | Description |
|--------|-------------|
| **sqlx with `query!` macros (chosen)** | Queries checked against DB schema at build time |
| sqlx with runtime `query()` | Queries are strings; errors surface at runtime |
| Diesel ORM | Strong typing; requires schema codegen; heavier setup |
| Raw `rusqlite` | Minimal dependency; no async; no compile-time checking |

#### Decision Outcome

**sqlx with compile-time query macros** (`query!`, `query_as!`).

During `cargo build`, sqlx connects to the database identified by `DATABASE_URL`, introspects the schema, and verifies every SQL query for syntax and column-type correctness. This surfaces schema mismatches as build errors rather than panics at runtime.

The `sqlx::migrate!` macro also runs pending migrations at application startup, keeping the schema in sync automatically.

#### Consequences

- ✅ SQL errors caught at build time — no silent runtime failures from typos or missing columns.
- ✅ Automatic migration application removes manual `sqlite3` invocations during development.
- ⚠️ `DATABASE_URL` must point to a valid, migrated database for `cargo build` to succeed.
- ⚠️ CI pipelines must provision a local SQLite file before running `cargo build` (or use `sqlx`'s offline query cache via `cargo sqlx prepare`).

---

### 6. Spec Lifecycle — State Machine with Defined Transitions

#### Options Considered

| Option | Description |
|--------|-------------|
| **Defined state machine (chosen, partially implemented)** | Fixed status values with transition rules |
| Free-form status strings | Any string accepted; no enforcement |
| Event-sourced lifecycle | Status derived from event log; no `status` column |

#### Decision Outcome

Specs follow a defined lifecycle:

```
draft → approved → in_progress ⇄ paused → done
```

Status values are stored in the `specs.status` column. The CLI and MCP tools accept status strings and write them to the database.

**Current state:** transition rules are documented but **not yet enforced in code** — invalid transitions (e.g. `done → in_progress`) are not rejected by the API layer. This is tracked as **IMP-010** in `docs/IMPROVEMENTS.md`. Enforcement will be added in a future release via a guard function in the `spec_update` handler.

#### Consequences

- ✅ Status values are well-defined and visible to agents via `state_snapshot`.
- ✅ The lifecycle model provides a shared vocabulary for orchestrator agents.
- ⚠️ Without enforcement (IMP-010), agents can transition specs to any status string; data integrity relies on agent discipline.
- ⚠️ The `paused` ⇄ `in_progress` bi-directional edge is not yet tested.

---

### 7. MCP Tool Naming — Multi-Prefix Alias Strategy

#### Options Considered

| Option | Description |
|--------|-------------|
| **Multi-prefix aliases (chosen, flagged as debt)** | `state_*`, `spec_*`, `slice_*` all map to the same handlers |
| Single canonical prefix only | One prefix (`state_*`); agents must be retrained |
| Versioned tool names | `state_spec_get_v2`; explicit version in name |

#### Decision Outcome

**Three prefix families** are registered for backward compatibility during early adoption:

- `state_*` — canonical (14 tools)
- `spec_*` / `task_*` / `event_*` / `memory_*` / `artifact_*` / `constitution_*` — direct-domain aliases
- `slice_*` — legacy aliases matching an earlier naming scheme

All 27 registrations dispatch to the same 14 handler functions. This allows agents trained on any prefix to interoperate without prompt changes.

This is explicitly flagged as technical debt in **IMP-008** (`docs/IMPROVEMENTS.md`). Once all agent skills have been updated to use `state_*` exclusively, the alias registrations will be removed.

Additionally, MCP `memory` entries include an optional `spec` scope field. Scoping is stored but not yet enforced as a read isolation boundary. See **IMP-007** in `docs/IMPROVEMENTS.md`.

#### Consequences

- ✅ Zero-friction adoption: agents trained on any prefix work without modification.
- ✅ Smooth migration path: canonical prefix can be enforced at a later date.
- ⚠️ 27-tool registration makes MCP tool listings verbose; LLM context windows include redundant entries.
- ⚠️ Any breaking change to a handler signature must be reflected across all alias registrations.
- ⚠️ Memory entries are not yet scope-isolated by spec (IMP-007); agents reading memory may see entries from unrelated specs.

---

## Implementation Notes

### Technology Stack Summary

| Concern | Choice | Crate(s) |
|---------|--------|----------|
| Language | Rust, edition 2021 | — |
| Async runtime | Tokio multi-thread | `tokio` |
| CLI framework | Clap v4 derive | `clap` |
| Database | SQLite | `sqlx` |
| Serialization | JSON | `serde`, `serde_json` |
| Terminal tables | comfy-table | `comfy-table` |
| Terminal colour | ANSI | `colored` |
| Interactive prompts | dialoguer | `dialoguer` |
| Asset embedding | compile-time | `include_dir` |

### Directory Conventions

```
.spex/
  state.db          # SQLite database (project-local)
~/.config/opencode/
  skills/           # Extracted agent skill files
```

### Build-Time Requirements

- `DATABASE_URL=sqlite:.spex/state.db` must be set (or use `cargo sqlx prepare` offline cache).
- The embedded skills assets directory must exist at the path referenced in `build.rs`.

### Cross-References to Improvement Backlog

| ID | Summary |
|----|---------|
| IMP-007 | Memory scope isolation — enforce `spec` field as a read boundary |
| IMP-008 | Retire legacy MCP tool alias prefixes after agent retraining |
| IMP-010 | Enforce spec lifecycle state machine transitions in `spec_update` handler |
| IMP-016 | Enable SQLite WAL mode to reduce write-lock contention |

All items are tracked in `docs/IMPROVEMENTS.md`.
