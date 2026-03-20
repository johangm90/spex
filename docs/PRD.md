# Spex — Product Requirements Document

> **Status:** active  
> **Version:** 1.0  
> **Date:** 2026-03-06  
> **Authors:** spex-orchestrate synthesis

---

## Vision

Spex is a **Spec-Driven Development (SDD) CLI** that gives a team of autonomous AI coding agents a shared, persistent, structured picture of what needs to be built, who is building it, and what the current state of the work is.

The problem it solves: when multiple LLM-based agents collaborate on a software project, there is no authoritative shared state — each session starts blind, agents duplicate work, and the human developer loses traceability. Spex provides that shared brain: a single Rust binary that runs a local MCP server, persists project state in SQLite, bundles 6 specialised agent skills, and enforces a human-gated spec lifecycle.

**Who benefits:**
- Developers using AI-assisted coding (primarily OpenCode users) who want structured, traceable, multi-agent workflows from the terminal.
- Any developer who wants to apply a spec/slice methodology to their projects with AI agent coordination.

---

## Goals

1. **Zero-friction agent coordination** — Any specialised AI agent (`spex-backend`, `spex-qa`, `spex-orchestrate`, etc.) can read and write shared project state in under 100ms via the MCP server, with no network dependency and no external service.

2. **Human remains the gate** — Every spec must pass through human approval (`draft → approved`) before any agent executes work, and the CLI enforces the state machine transitions — no agent can skip stages.

3. **One binary, works anywhere** — `spex` is a self-contained Rust binary: it embeds all 6 bundled agent skills, bundles the MCP server, includes migrations, and installs via a single shell command. No Node.js, no Python, no Docker required.

---

## Non-Goals

- **Not a web UI tool** — Spex is terminal-first. No browser dashboard, no SaaS backend, no remote sync (unlike Engram's Git sync).
- **Not a general-purpose memory system** — The memory layer is an agent coordination scratchpad, not a long-term personal knowledge base. (Engram handles that use-case better.)
- **Not multi-project / multi-repo** — Each `.spex/state.db` is scoped to one project root. Cross-project federation is out of scope.
- **Not an agent runtime** — Spex does not execute agents. It provides state; OpenCode executes agents.
- **Not locked to OpenCode** — The MCP protocol is standard JSON-RPC 2.0 over stdio, usable by any MCP-compatible agent host. The bundled skills happen to target OpenCode's format.
- **Not a CI/CD system** — Spex tracks spec status and gates; it does not trigger builds, run pipelines, or deploy code.

---

## Users

### Primary Persona — "The AI-Augmented Developer"
A software engineer who uses OpenCode (or compatible tools) as their primary development environment. They work on medium-to-large features decomposed into specs and delegate implementation to specialised AI agents. They want:
- Visibility into what every agent is doing (the `pulse` dashboard)
- An audit trail of all decisions and events (the `trace` log)
- Confidence that agents won't clobber each other or skip human review

### Secondary Persona — "The AI Team Lead"
A developer building or running multi-agent pipelines (e.g. `spex-orchestrate` coordinating `spex-backend`, `spex-qa`, `spex-gitops`). They need the orchestration primitives: tasks, artifact tracking, event emission, and memory scoped to individual specs and agents.

---

## Tech Stack

| Layer | Technology | Notes |
|---|---|---|
| **Language** | Rust (stable, edition 2021) | Single binary target; release profile: `opt-level="z"`, LTO, strip |
| **CLI** | `clap` 4 with derive | Color, env, and cargo features enabled |
| **Async runtime** | `tokio` (full features) | Required for SQLx async and MCP stdio loop |
| **Database** | SQLite via `sqlx` 0.8 | WAL mode, foreign keys, compile-time migrations |
| **Serialization** | `serde` + `serde_json` | All API payloads are JSON |
| **Date/time** | `chrono` 0.4 with serde | RFC3339 timestamps throughout |
| **Error handling** | `anyhow` + `thiserror` | Propagate up to CLI boundary; display user-friendly messages |
| **Terminal UI** | `colored` + `inquire` | Color output; interactive prompts for `plan build` |
| **Asset embedding** | `include_dir` | All 11 bundled agent skills compiled into the binary |
| **Platform dirs** | `dirs` | Resolve `~/.config/opencode/` cross-platform |
| **MCP transport** | JSON-RPC 2.0 over stdio | No HTTP server; OpenCode spawns `spex mcp serve` as subprocess |
| **CI/CD** | GitHub Actions | `ci.yml` (test + build), `release.yml` (cross-compile + publish) |

---

## Architecture Principles

1. **Project-local state only** — All mutable state lives in `.spex/state.db` at the project root. The binary walks up the directory tree to find it (same pattern as `git`). No global mutable state.

2. **Human approval is a hard gate** — The spec state machine (`draft → approved → in_progress ⇄ paused → done`) is enforced in Rust, not just by convention. Agents cannot transition a spec to `in_progress` without human `spex spec approve`.

3. **Append-only event log** — The `events` table is never modified after insert. All state changes produce an event. This provides a complete audit trail.

4. **Agents use MCP, humans use CLI** — The CLI is for human operators. The MCP server is for agents. They share the same SQLite database but through different interfaces.

5. **Memory is a per-agent, per-spec scratchpad** — The `memory` table scopes KV entries to `(agent, spec, key)`. An agent's session context must not leak into another agent's or spec's namespace.

6. **No external services at runtime** — The binary must work fully offline. SQLite is embedded. Skills are compiled in. No network calls during normal operation.

7. **Canonical tool names** — All MCP tools use the `state_` prefix convention. Tool names are stable; renaming requires a deprecation cycle.

8. **Single responsibility per module** — `sdd/` holds only domain logic (no I/O). `cli/` holds only output formatting and event emission. `mcp/` holds only protocol handling. Cross-cutting concerns (DB path, timestamps) live in `sdd/db.rs` and `chrono`.

---

## Acceptance Standards

A spec is **done** when all of the following are true:

1. **All tasks pass** — Every task in the spec reaches `done` status (no `failed` or `in_progress` tasks remain).
2. **Project-appropriate validation passes** — The repo's standard validation gate exits 0. For this Rust codebase, the equivalent gate is `cargo test`, `cargo clippy -- -D warnings`, and `cargo build --release`.
3. **No regressions** — `spex doctor` reports no failures (warnings are acceptable).
4. **State machine respected** — Spec reached `done` only via `in_progress → done`; no direct jumps.
5. **Event trail complete** — The event log contains at minimum: `SpecApproved`, `SpecStarted`, relevant `TaskCompleted` events, and `SpecCompleted`.
6. **No dead code introduced** — New public items must not require `#[allow(dead_code)]` annotations.
7. **Tests added for new SDD ops** — Any new function in `src/sdd/` must have at least one `#[tokio::test]` test in a `tests/` file or inline `#[cfg(test)]` module.

---

## Feature Areas

### Core Spec Lifecycle
The central feature. `spec add`, `spec approve`, `spec start`, `spec done` with state machine enforcement. `plan build` for interactive task decomposition. `pulse` dashboard. `trace` event log.

### MCP State Server
The agent-facing API. JSON-RPC 2.0 over stdio. 18 tool operations covering specs, tasks, events, memory, artifacts, and PRD reading. Started via `spex mcp serve`.

### Skills & Agent Bundle
6 specialised agent skill files (`SKILL.md`) embedded in the binary at compile time. Installed to `~/.config/opencode/skills/` via `spex setup` or `spex skill install --all`.

### Project Scaffolding
`spex new <NAME>` and `spex init` for bootstrapping. Generates `PRD.md`, `opencode.json`, `.gitignore`, and `.spex/state.db` with auto-migrations.

### Doctor & Auto-fix
7 health checks covering DB, PRD, skills, MCP config, git repo, and stuck specs. `spex doctor --fix` auto-corrects fixable issues.

### Memory System (KV Scratchpad)
Per-agent, per-spec KV store. Currently: `memory_set` / `memory_get`. Planned enhancements: FTS5 search, typed entries, topic-key upserts, soft-delete, `memory_context`, `memory_stats`.

---

## Open Questions

1. ~~**`constitution` table** — The `constitution` DB table is vestigial (created in migration but never written). Should a future migration drop it, or repurpose it for structured metadata?~~ **Resolved:** Dropped via migration `20260319100000_drop_vestigial_tables.sql`.

2. ~~**`SpecStatus` enum** — Currently has `#[allow(dead_code)]`. Should it replace the raw string matching in `update_spec_status`, or is it purely for documentation?~~ **Resolved:** `SpecStatus` and `TaskStatus` enums now validate at the MCP boundary; `#[allow(dead_code)]` removed.

3. ~~**`meta` table** — Created in schema but never used. Reserved for project-level metadata (e.g. `project_name`, `spex_version`)? Define or drop.~~ **Resolved:** Dropped via migration `20260319100000_drop_vestigial_tables.sql`.

4. ~~**Dead dependencies** — `minijinja`, `toml`, `indicatif` appear unused in current source. Should they be removed in the next maintenance pass?~~ **Resolved:** Removed from `Cargo.toml`.

5. ~~**Test strategy** — IMP-003 (zero tests) is the highest-risk open item. Should tests live in `tests/integration/` (using `tempfile` for isolated DBs) or inline `#[cfg(test)]` modules? Integration or unit-first?~~ **Resolved:** Inline `#[cfg(test)]` modules with in-memory SQLite pools (`make_pool()`). 69 tests across spec, task, event, artifact, memory, MCP dispatch, and GC.

6. ~~**Memory system evolution** — How deeply should the memory layer evolve toward Engram-style features (FTS5, typed observations, topic keys)? Is this a minor enhancement or a separate slice?~~ **Resolved:** Memory Evolution pass shipped: `memory_list` with filtering/pagination, `memory_gc` with FTS rebuild, `spex memory` CLI, QueryBuilder refactor. 19 MCP tools.

7. ~~**MCP tool alias cleanup** — IMP-008: 27 tool registrations for 14 operations. Should aliases be hidden behind a `--legacy` flag, or kept first-class forever for backward compatibility?~~ **Resolved:** Codebase has 18 tools with no aliases. The "27 registrations" claim was stale.

8. ~~**Pagination** — IMP-009: `spec list`, `task list`, and `trace` have no limit. At what threshold does this become a real problem, and what pagination style fits a stdio MCP API?~~ **Resolved:** `list_specs`, `list_tasks`, and `query_events` all accept `limit`/`offset` pagination. MCP tools `state_slice_get`, `state_task_get`, and `state_event_query` expose these params.
