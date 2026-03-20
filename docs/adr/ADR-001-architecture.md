# ADR-001: Core Architecture of spex

Date: 2026-03-05
Status: Accepted
Deciders: core team

---

## Context and Problem Statement

`spex` is a command-line tool for spec-driven coordination in AI-assisted software delivery. It serves two primary audiences:

1. **Human engineers** — who use the CLI to manage project specs, tasks, events, and agent memory across a team.
2. **LLM agents** (running inside OpenCode) — which call `spex` as an MCP server to read and mutate shared project state without leaving their execution environment.

The tool must work completely offline, install as a single binary with no external dependencies, and integrate seamlessly with OpenCode's MCP client protocol. This document records the significant architectural decisions made during the early product design phase.

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

The current schema comprises five working tables: `specs`, `tasks`, `events`, `memory`, and `artifacts`. Earlier `constitution` and `meta` tables were removed by migration `20260319100000_drop_vestigial_tables.sql`. Migrations are applied automatically at startup via `sqlx::migrate!`.

#### Consequences

- ✅ Works completely offline; zero external infrastructure.
- ✅ State travels with the repo — a `git clone` restores history.
- ✅ SQL queries are compile-time checked (`DATABASE_URL` must be set during `cargo build`).
- ⚠️ Concurrent write throughput is still limited by SQLite's single-writer model, even with WAL mode enabled.

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

The MCP server dispatches a canonical tool set for snapshot, spec, task, event, memory, artifact, and PRD access. The current source exposes 20 canonical tools split across `state_*` and `memory_*` names in `build_tools_list()`, and this document should treat that source as authoritative for tool counts and names.

#### Consequences

- ✅ Zero port conflicts; no firewall rules needed.
- ✅ No authentication surface — the process is owned by the same user.
- ✅ Works out of the box with OpenCode's default MCP configuration.
- ⚠️ A single MCP session is single-threaded by the stdio pipe; parallel agent calls are serialised.
- ⚠️ Tool counts are implementation-derived facts and should be re-verified against `src/mcp/server.rs` when the MCP surface changes.

---

### 3. Agent Skills Distribution — Embedded Binary Assets

#### Options Considered

| Option | Description |
|--------|-------------|
| **`include_dir!` at build time (chosen)** | Skills compiled into the binary; `spex setup` extracts them |
| Downloaded at install time | Installer script fetches skills from a URL at `spex init` |
| Git submodule | Skills live in a submodule; user must init submodules after clone |
| Separate package | Skills distributed as an independent npm/crate/pip package |

#### Decision Outcome

**`include_dir!` macro** (from the `include_dir` crate), evaluated at compile time.

Six bundled agent markdown files from `agents/` are embedded directly into the `spex` binary. `build.rs` watches `agents/` for rebuilds, and the current installer writes bundled files to `~/.config/opencode/agents/` when the user runs `spex setup` or `spex skill install --all`.

#### Consequences

- ✅ Offline installation — no internet access required after `cargo install spex`.
- ✅ Bundled agents are version-locked to the binary; no drift between tool behaviour and shipped prompts.
- ✅ Single binary distribution; no sidecar files.
- ⚠️ Binary size increases with every new bundled agent file.
- ⚠️ Updating a bundled agent requires a new binary release; there is no hot-reload path.

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
| **Defined state machine (chosen)** | Fixed status values with transition rules |
| Free-form status strings | Any string accepted; no enforcement |
| Event-sourced lifecycle | Status derived from event log; no `status` column |

#### Decision Outcome

Specs follow a defined lifecycle:

```
draft → approved → in_progress ⇄ paused → done
```

Status values are stored in the `specs.status` column, and transitions are validated against the defined lifecycle before updates are persisted.

#### Consequences

- ✅ Status values are well-defined and visible to agents via `state_snapshot`.
- ✅ The lifecycle model provides a shared vocabulary for orchestrator agents.
- ✅ Invalid transitions are rejected before they reach persistent state.
- ⚠️ Any future lifecycle expansion still requires coordinated updates across CLI, MCP schemas, and validation logic.

---

### 7. MCP Tool Naming — Canonical State and Memory Names

#### Options Considered

| Option | Description |
|--------|-------------|
| **Canonical state + memory names (current)** | `state_*` and `memory_*` names map directly to the supported MCP operations |
| Single `state_*` prefix only | One prefix for all tools, including memory operations |
| Versioned tool names | `state_spec_get_v2`; explicit version in name |

#### Decision Outcome

The current implementation exposes 20 canonical MCP tools:

- 12 `state_*` tools for snapshot, spec, task, event, artifact, and PRD operations
- 8 `memory_*` tools for memory storage, search, deletion, statistics, and relationship lookup

This tool surface is what `build_tools_list()` returns today, so it is the authoritative source for names and counts used by bundled agents and docs.

Additionally, MCP `memory` entries include an optional `spec` scope field. Scoping is stored but not yet enforced as a read isolation boundary. See **IMP-007** in `docs/IMPROVEMENTS.md`.

#### Consequences

- ✅ Tool listings match the current MCP surface directly; docs can refer to canonical names without alias indirection.
- ✅ Memory capabilities are discoverable as first-class tools rather than implicit state sub-operations.
- ⚠️ Published tool counts still require periodic verification because they can drift when new tools are added.
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
| Terminal tables | Plain CLI formatting | — |
| Terminal colour | ANSI | `colored` |
| Interactive prompts | inquire | `inquire` |
| Asset embedding | compile-time | `include_dir` |

### Directory Conventions

```
.spex/
  state.db          # SQLite database (project-local)
~/.config/opencode/
  agents/           # Installed bundled agent markdown files
  skills/<slug>/    # Generated custom project skills (`SKILL.md`)
```

### Build-Time Requirements

- `DATABASE_URL=sqlite:.spex/state.db` must be set (or use `cargo sqlx prepare` offline cache).
- The embedded bundled-agent directory must exist at the path referenced in `build.rs`.

### Cross-References to Improvement Backlog

| ID | Summary |
|----|---------|
| IMP-007 | Memory scope isolation — enforce `spec` field as a read boundary |

Open improvement items are tracked in `docs/IMPROVEMENTS.md`.
