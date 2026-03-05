# Improvement Backlog — `spex`

This document is the prioritised improvement backlog for the `spex` CLI tool. Items are ordered by priority (P0 → P3) and then by estimated effort. Use this as the authoritative source when triaging issues or planning sprint work.

## Summary

| ID | Title | Priority | Effort | Location |
|----|-------|----------|--------|----------|
| IMP-001 | `law freeze` is a no-op | P0 | S | `src/cli/law.rs` |
| IMP-002 | `--yes` flag silently ignored in `spex new` | P0 | S | `src/scaffold/mod.rs` |
| IMP-003 | No automated tests | P1 | L | project-wide |
| IMP-004 | No CI/CD pipeline | P1 | M | `.github/workflows/` |
| IMP-005 | `doctor --fix` is a stub | P1 | M | `src/doctor/mod.rs` |
| IMP-006 | No CHANGELOG / CONTRIBUTING | P1 | S | project root |
| IMP-007 | `memory_get_all` ignores `spec` scope | P1 | S | `src/mcp/server.rs` |
| IMP-008 | MCP tool proliferation (27 aliases) | P2 | M | `src/mcp/server.rs` |
| IMP-009 | No pagination on list commands | P2 | M | `src/sdd/*.rs` |
| IMP-010 | Spec lifecycle transitions not enforced | P2 | M | `src/sdd/spec.rs` |
| IMP-011 | `spex pulse` has no time-range filter | P2 | S | `src/cli/pulse.rs` |
| IMP-012 | No structured logging / tracing | P2 | M | project-wide |
| IMP-013 | Binary size not optimised | P3 | S | `Cargo.toml` |
| IMP-014 | `spex doctor` checks are hard-coded strings | P3 | S | `src/doctor/mod.rs` |
| IMP-015 | No shell completion generation | P3 | S | `src/main.rs` |
| IMP-016 | SQLite WAL mode not enabled | P3 | S | `src/sdd/db.rs` |

---

## IMP-001 — `law freeze` is a no-op (Priority: P0)

**Problem:** `spex law freeze` sets the constitution status to `"frozen"` in the DB, but `cmd_law_edit` never checks this flag — it always overwrites the constitution regardless of frozen state. This means the freeze command provides a false sense of immutability.

**Location:** `src/cli/law.rs` — `cmd_law_edit` function

**Proposed Solution:** At the start of `cmd_law_edit`, query the DB for the current constitution status. If `status == "frozen"`, print an error `"Constitution is frozen. Use 'spex law unfreeze' to make changes."` and return early with a non-zero exit code.

**Effort:** S

---

## IMP-002 — `--yes` flag silently ignored in `spex new` (Priority: P0)

**Problem:** The `spex new --yes` flag (meant to skip interactive prompts) is accepted by the CLI parser but the parameter is named `_yes` and never read. Users who pass `--yes` in scripts still get interactive prompts, breaking automation.

**Location:** `src/scaffold/mod.rs` — `cmd_new` function signature uses `_yes: bool`

**Proposed Solution:** Rename `_yes` to `yes`, and branch on its value: if `yes == true`, skip all `Confirm::new(...).interact()` calls and apply defaults directly.

**Effort:** S

---

## IMP-003 — No automated tests (Priority: P1)

**Problem:** The project has zero unit, integration, or doc tests. Only `tempfile` is listed as a dev-dependency but is never used. This means regressions can ship silently.

**Location:** `project-wide`

**Proposed Solution:** Add unit tests for each SDD domain module (`spec.rs`, `task.rs`, `memory.rs`, `artifact.rs`), using an in-memory SQLite DB (`sqlite::memory:`). Add integration tests in `tests/` that spin up `spex new` in a temp dir and assert CLI output. Target ≥ 60% line coverage.

**Effort:** L

---

## IMP-004 — No CI/CD pipeline (Priority: P1)

**Problem:** There is no `.github/workflows/` directory. No automated build, test, lint, or release pipeline exists. Contributions can break the build without detection.

**Location:** `.github/workflows/` (does not exist)

**Proposed Solution:** Create `.github/workflows/ci.yml` with jobs: `fmt` (`cargo fmt --check`), `clippy` (`cargo clippy -- -D warnings`), `test` (`cargo test`), `build` (matrix: ubuntu/macos/windows). Add a `release.yml` that publishes to crates.io on tag push.

**Effort:** M

---

## IMP-005 — `doctor --fix` is a stub (Priority: P1)

**Problem:** `spex doctor --fix` prints `"Auto-fix not yet implemented"` and exits. The flag is advertised in help text but does nothing, eroding user trust.

**Location:** `src/doctor/mod.rs` — `cmd_doctor` function

**Proposed Solution:** Implement auto-fix for at least the automatable checks: (1) create missing `.spex/` directory, (2) run `spex init` if no DB found, (3) run `spex skills install` if agent skills are out of date. Checks that require human judgment (e.g. "MCP configured?") should print actionable instructions instead of silently passing.

**Effort:** M

---

## IMP-006 — No CHANGELOG / CONTRIBUTING (Priority: P1)

**Problem:** The project has no `CHANGELOG.md` or `CONTRIBUTING.md`. Contributors have no guidance on how to contribute, and users cannot track what changed between releases.

**Location:** `project root`

**Proposed Solution:** Create `CHANGELOG.md` following Keep a Changelog format (https://keepachangelog.com). Create `CONTRIBUTING.md` covering: fork-and-PR workflow, commit message convention (Conventional Commits), running tests, and the agent skill development guide.

**Effort:** S

---

## IMP-007 — `memory_get_all` ignores `spec` scope (Priority: P1)

**Problem:** When an MCP client calls `state_memory_get` without a `key` (intending to list all memory for a spec), the server's handler drops the `spec` parameter from the SQL query. This returns memory entries across all specs, causing cross-spec contamination and leaking data between agent sessions.

**Location:** `src/mcp/server.rs` — `memory_get` handler, `all_entries` branch

**Proposed Solution:** In the `all_entries` branch, pass the `spec` parameter into the SQL query: `SELECT * FROM memory WHERE agent = ? AND (spec = ? OR spec IS NULL)`. Add a test asserting spec isolation.

**Effort:** S

---

## IMP-008 — MCP tool proliferation (27 aliases) (Priority: P2)

**Problem:** `tools/list` exposes 27 entries because the server registers three prefix aliases for each tool (`spec_*`, `slice_*`, `state_*`). This inflates the tool list shown to LLM agents, increases token usage, and causes confusion about canonical names.

**Location:** `src/mcp/server.rs` — tool registration

**Proposed Solution:** Keep only the `state_*` canonical names. Move `spec_*` and `slice_*` aliases behind a `--legacy-aliases` flag (default: off). Update the README and skill files to reference canonical names only. Emit a deprecation warning when a legacy alias is used.

**Effort:** M

---

## IMP-009 — No pagination on list commands (Priority: P2)

**Problem:** `spex spec list`, `spex task list`, `spex trace`, and `spex pulse` fetch all rows from SQLite without any LIMIT/OFFSET. In projects with hundreds of specs or thousands of events, this will produce unusable terminal output and slow queries.

**Location:** `src/sdd/spec.rs`, `src/sdd/task.rs`, `src/sdd/event.rs`

**Proposed Solution:** Add `--limit N` (default: 50) and `--offset N` (default: 0) flags to all list commands. Thread these through the SDD layer as SQL `LIMIT ? OFFSET ?` parameters. Add `--all` flag to bypass pagination.

**Effort:** M

---

## IMP-010 — Spec lifecycle transitions not enforced (Priority: P2)

**Problem:** `spex spec update --status` accepts any string value and writes it directly to the DB. There is no validation that transitions follow the defined lifecycle (`draft → approved → in_progress ⇄ paused → done`). An agent can set a spec from `draft` directly to `done`, skipping required gates.

**Location:** `src/sdd/spec.rs` — `update_spec` function

**Proposed Solution:** Define a `SpecStatus` enum with valid transition rules. In `update_spec`, reject invalid transitions with a descriptive error (e.g. `"Cannot transition from 'draft' to 'done'; must pass through 'approved' and 'in_progress' first"`). Expose an `--force` flag for human overrides.

**Effort:** M

---

## IMP-011 — `spex pulse` has no time-range filter (Priority: P2)

**Problem:** `spex pulse` shows recent events but provides no way to filter by time range. Users cannot narrow output to "events from the last hour" or "events since yesterday".

**Location:** `src/cli/pulse.rs`

**Proposed Solution:** Add `--since <datetime|duration>` and `--until <datetime>` flags. Support ISO 8601 timestamps and human durations like `1h`, `2d`. Thread into the SQL query as `WHERE timestamp >= ?`.

**Effort:** S

---

## IMP-012 — No structured logging / tracing (Priority: P2)

**Problem:** The application uses `eprintln!` and `println!` for all output. There is no structured logging, no log levels, and no way to enable debug output for troubleshooting. This makes diagnosing MCP server issues nearly impossible.

**Location:** `project-wide`

**Proposed Solution:** Add `tracing` and `tracing-subscriber` crates. Replace `eprintln!` debug output with `tracing::debug!` / `tracing::error!`. Respect `RUST_LOG` env var. In MCP stdio mode, ensure logs go to stderr only (not stdout, which is reserved for JSON-RPC).

**Effort:** M

---

## IMP-013 — Binary size not optimised (Priority: P3)

**Problem:** `Cargo.toml` has no release profile tuning. The binary includes debug symbols and is not stripped, resulting in a larger-than-necessary binary (estimated 15–25 MB unstripped).

**Location:** `Cargo.toml`

**Proposed Solution:** Add to `Cargo.toml`:

```toml
[profile.release]
opt-level = "z"
lto = true
codegen-units = 1
strip = true
panic = "abort"
```

**Effort:** S

---

## IMP-014 — `spex doctor` checks are hard-coded strings (Priority: P3)

**Problem:** The 7 doctor checks are implemented as a hard-coded `Vec<(&str, bool, &str)>` of `(name, passed, message)` tuples. Adding a new check requires editing the core function rather than registering a new check, making the system brittle.

**Location:** `src/doctor/mod.rs`

**Proposed Solution:** Define a `DoctorCheck` trait with `name()`, `run() -> CheckResult`, and `fix() -> Result<()>` methods. Register checks in a `Vec<Box<dyn DoctorCheck>>`. This enables `--fix` to be implemented per-check and makes the system extensible.

**Effort:** S

---

## IMP-015 — No shell completion generation (Priority: P3)

**Problem:** `spex` has no shell completion scripts for bash, zsh, or fish. Power users must type full subcommand names without tab-completion assistance.

**Location:** `src/main.rs` / `build.rs`

**Proposed Solution:** Use `clap_complete` crate to generate completion scripts at build time (in `build.rs`) and install them via `spex skills install` or as a separate `spex completions <shell>` subcommand.

**Effort:** S

---

## IMP-016 — SQLite WAL mode not enabled (Priority: P3)

**Problem:** SQLite defaults to DELETE journal mode. In WAL (Write-Ahead Logging) mode, reads don't block writes and writes don't block reads, which is important for the MCP server which may receive concurrent tool calls.

**Location:** `src/sdd/db.rs` — `init_db` function

**Proposed Solution:** After opening the connection pool, execute `PRAGMA journal_mode=WAL; PRAGMA synchronous=NORMAL;`. This is a one-line change with measurable concurrency benefits.

**Effort:** S

---

## Legend

| Priority | Meaning |
|----------|---------|
| P0 | Correctness bug — fix immediately |
| P1 | Significant gap — fix in next release |
| P2 | Enhancement — plan for upcoming sprint |
| P3 | Polish — address when convenient |

| Effort | Meaning |
|--------|---------|
| S | Small: < 2 hours |
| M | Medium: 2–8 hours |
| L | Large: > 1 day |
