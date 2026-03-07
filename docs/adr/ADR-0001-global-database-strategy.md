# ADR-0001: Global Shared Database Strategy — Always-Global, project_dir Partition

**Date:** 2026-03-07  
**Status:** Accepted  
**Deciders:** product owner, spex-architect  
**Relates to:** SLICE-005

---

## Context and Problem Statement

`spex` was originally designed with a per-project storage model: every project root
contains a `.spex/state.db` SQLite file, and the MCP server discovers it by walking
up the directory tree (the same pattern as `git`). This worked for single-project
usage, but created a significant friction point for developers who work on multiple
repositories simultaneously:

- Every new project requires `spex init` to create a `.spex/state.db`.
- The global OpenCode `~/.config/opencode/config.json` can only point the MCP server
  at a fixed binary; there is no built-in way to make it resolve "the project I am
  currently working in" at tool-call time.
- Running `spex mcp setup` per project means maintaining multiple per-project
  `opencode.json` override files, defeating the goal of zero-friction setup.

The core question: **where should spex store its mutable state, and how should
project isolation be achieved?**

---

## Decision Drivers

- **Zero-friction multi-project workflow** — configure once in `~/.config/opencode/config.json`;
  the active project is determined automatically from the working directory at server
  startup.
- **Strong project isolation** — rows belonging to `/project/A` must never be visible
  to an MCP session running in `/project/B`.
- **Simple migration path** — existing per-project databases must be importable without
  data loss.
- **No per-project config ceremony** — `spex init` / `spex new` should only create
  the `.spex/` directory marker; no DB creation required.
- **Clean MCP entry JSON** — no environment variables needed in the generated config
  beyond the binary path.

---

## Alternatives Considered

### Alternative A — Per-Project DB (status quo)

Each project root contains `.spex/state.db`. The MCP server walks up from CWD to find
the DB (same walk-up pattern as `git`).

**Pros:**
- Complete data isolation by filesystem boundary.
- DB travels with the repo and can be committed to version control.
- No coordination needed between projects.

**Cons:**
- Requires per-project `spex init` to create the DB.
- Global OpenCode config cannot resolve the active project; each project needs its own
  `opencode.json` override file.
- Walk-up logic fails silently when CWD is above the project root (e.g. in a monorepo
  working on a sub-package from a parent shell).
- Cannot provide a single `spex mcp setup --global` that works for all projects.

### Alternative B — Global DB with `project_dir` Partition Key (chosen)

A single SQLite database lives at `~/.local/share/spex/global-state.db` (resolved via
`dirs::data_dir()`). Every project-scoped table gains a `project_dir TEXT NOT NULL`
column. The MCP server resolves the active project from the `SPEX_PROJECT_DIR`
environment variable (or `std::env::current_dir()` as fallback) at startup and
injects it as a filter into every SQL query.

**Pros:**
- Single global MCP config; no per-project setup ceremony.
- Strong isolation: `AND project_dir = ?` in every query ensures cross-project
  visibility is impossible at the SQL layer.
- `spex init` / `spex new` only create `.spex/` as a project-root marker (for
  walk-up detection); no DB creation.
- One-time migration path: `spex db migrate-to-global` imports existing per-project
  DBs.
- Clean MCP entry JSON: no `SPEX_GLOBAL_DB` env var; no `--global-db` flag needed.

**Cons:**
- Breaking change for existing users: `.spex/state.db` is no longer the live DB.
  Users must run `spex db migrate-to-global` once.
- `project_dir` must be correct at MCP server startup; an incorrect CWD (e.g. wrong
  shell session) would scope queries to the wrong project. Mitigated by
  `SPEX_PROJECT_DIR` env var support and `spex doctor` warnings.
- All 14 project-scoped tables require a schema migration; 11 of them need the full
  `CREATE TABLE new + INSERT-SELECT + DROP + RENAME` pattern because SQLite does not
  support `ALTER PRIMARY KEY`.

### Alternative C — Per-Project DB with Global Symlink

Maintain per-project DBs but create a stable symlink at a fixed path (e.g.
`~/.local/share/spex/active.db → /current/project/.spex/state.db`) that the global
MCP config points at.

**Pros:**
- No schema migration needed.
- Project isolation maintained by filesystem.

**Cons:**
- Symlink must be updated whenever the developer switches projects — requires a
  `spex switch` command or a shell hook.
- Breaks on concurrent multi-project sessions (two shells, two projects).
- OS-specific behaviour on Windows (symlinks require elevated privileges or Developer
  Mode).
- Fragile: the symlink becomes stale if `.spex/state.db` is deleted or the project
  is moved.
- Does not compose with OpenCode's remote/container environments.

---

## Decision

**Alternative B — Global DB with `project_dir` partition key, always-on (no
per-project fallback).**

Rationale:
- The project isolation guarantee is stronger with a SQL filter than with a symlink.
- Eliminating the per-project DB entirely removes the entire class of "wrong DB"
  configuration errors.
- The breaking-change cost is one-time and recoverable (migration command + doctor
  warning); the ergonomic gain is permanent.
- A single global config file is the standard expectation for developer tools (cf.
  `gh`, `aws cli`, `kubectl`).

Per-project DB mode is **removed entirely**. There is no `--per-project` flag, no
`SPEX_GLOBAL_DB` env var toggle, and no runtime fallback. The global DB is the only
mode.

---

## Consequences

### Positive

- **Single MCP config for all projects** — `spex mcp setup` configures OpenCode once;
  `SPEX_PROJECT_DIR` (or CWD) selects the active project at server start time.
- **No DB setup for new projects** — `spex init` / `spex new` only create `.spex/`
  (the project-root marker); the global DB is created automatically on first
  `spex mcp serve`.
- **Full data isolation at the SQL layer** — `AND project_dir = ?` ensures cross-
  project leakage is impossible regardless of how the tool is invoked.
- **`spex mcp setup` entry JSON is clean** — no `env` key, no `SPEX_GLOBAL_DB`
  variable needed.

### Negative

- **Breaking change** — existing `.spex/state.db` users must run
  `spex db migrate-to-global` once to import their data. Old `.spex/state.db` files
  are not auto-discovered at runtime; they become inert until migrated.
- **`project_dir` must be correct at startup** — if an agent session starts with an
  incorrect CWD and `SPEX_PROJECT_DIR` is not set, queries will scope to the wrong
  project. Mitigated by: (1) `SPEX_PROJECT_DIR` env var support, (2) `spex doctor`
  warning when `SPEX_PROJECT_DIR` is not set, (3) server startup log printing both
  the global DB path and the active `project_dir`.
- **Schema migration complexity** — 11 tables need the `CREATE TABLE new + INSERT-
  SELECT + DROP + RENAME` pattern (SQLite limitation); `task_leases` gets a composite
  `(project_dir, task_id)` PK. Only `events`, `constitution`, and `meta` can use
  simple `ALTER TABLE ADD COLUMN`.
- **`memory` table is intentionally excluded** — the `memory` table is already scoped
  by `(agent, spec, key)` and cross-project agent memory sharing is the correct
  behaviour (e.g. `spex-architect` recalling a pattern from another project). No
  `project_dir` column is added to `memory`.

---

## Implementation References

| Task | File | Summary |
|------|------|---------|
| T05-01 | `docs/adr/ADR-0001-global-database-strategy.md` | This document |
| T05-02 | `docs/PRD.md` | Remove per-project non-goal; update Architecture Principle 1 |
| T05-03 | `migrations/20260308000000_global_project_dir.sql` | Schema migration |
| T05-04 | `src/sdd/db.rs` | `open_global_db()`, `global_db_path()`, remove per-project fns |
| T05-05 | `src/sdd/*.rs` | Add `project_dir: &str` to all query functions |
| T05-06 | `src/mcp/server.rs` | Thread `project_dir` through dispatch |
| T05-07 | `src/cli/mcp_cmd.rs` | Always-global serve; `SPEX_PROJECT_DIR` resolution |
| T05-08 | `src/tool_target/mod.rs` | Clean MCP entry JSON |
| T05-09 | `src/cli/db_cmd.rs` | `spex db migrate-to-global` command |
| T05-10 | `src/doctor/mod.rs` | Global DB checks; stale per-project DB warning |
| T05-11 | `src/scaffold/mod.rs` | No `.spex/state.db` creation |
| T05-12 | `src/main.rs` | Wire `spex db` subcommand group |
| T05-13 | `tests/` | Integration tests (4 scenarios) |
| T05-14 | QA gate | Verify all 19 ACs; cargo clippy + test + build --release |

---

## Global DB Path

```
~/.local/share/spex/global-state.db
```

Resolved via `dirs::data_dir()` (cross-platform). On Linux: `$XDG_DATA_HOME/spex/` or
`~/.local/share/spex/`. On macOS: `~/Library/Application Support/spex/`. On Windows:
`%APPDATA%\spex\`.

---

## Project Resolution Order

At `spex mcp serve` startup:

1. If `SPEX_PROJECT_DIR` is set and resolves to a valid directory → use canonicalized
   path as `project_dir`.
2. Otherwise → use `std::env::current_dir()`, canonicalized.
3. If neither resolves → print error to stderr, exit non-zero.

The resolved `project_dir` is logged to stderr at startup:
```
[spex] global DB: /home/user/.local/share/spex/global-state.db
[spex] project:   /home/user/projects/my-repo
```
