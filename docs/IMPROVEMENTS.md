# Improvement Backlog — `spex`

This document is the prioritised improvement backlog for the `spex` CLI tool. Items are ordered by priority (P0 → P3) and then by estimated effort. Use this as the authoritative source when triaging issues or planning sprint work.

## Summary

| ID | Title | Priority | Effort | Location |
|----|-------|----------|--------|----------|
| IMP-001 | Historical: `law freeze` issue from earlier CLI shape | P0 | S | obsolete (`src/cli/law.rs` removed) |
| IMP-002 | Completed: `--yes` now works in `spex new` | P0 | S | `src/scaffold/mod.rs` |
| IMP-003 | Automated tests exist, but coverage is still thin | P1 | L | project-wide |
| IMP-004 | Historical: CI/release workflows now exist | P1 | M | `.github/workflows/` |
| IMP-005 | Completed: `doctor --fix` now performs basic auto-fixes | P1 | M | `src/doctor/mod.rs` |
| IMP-006 | Completed: CONTRIBUTING guide exists | P1 | S | `CONTRIBUTING.md` |
| IMP-007 | Completed: `memory_get_all` respects `spec` scope | P1 | S | `src/sdd/memory.rs` |
| IMP-008 | MCP tool proliferation (27 aliases) | P2 | M | `src/mcp/server.rs` |
| IMP-009 | Pagination is still incomplete across CLI list views | P2 | M | `src/sdd/*.rs` |
| IMP-011 | Completed: `spex pulse` supports time-range filters | P2 | S | `src/cli/pulse.rs` |
| IMP-012 | Structured logging exists, but adoption is partial | P2 | M | project-wide |
| IMP-013 | Completed: release profile is tuned for smaller binaries | P3 | S | `Cargo.toml` |
| IMP-014 | `spex doctor` checks are still hard-coded in one module | P3 | S | `src/doctor/mod.rs` |
| IMP-015 | No shell completion generation | P3 | S | `src/main.rs` |

---

## IMP-001 — Historical: `law freeze` issue from earlier CLI shape (Priority: P0)

**Status:** Obsolete. The current CLI no longer has `src/cli/law.rs` or a `spex law freeze` command in this form, so this backlog item no longer reflects the current repository state.

**Location:** historical reference only

**Note:** Keep this ID only as a historical note in case constitution freezing returns in a future CLI redesign.

**Effort:** S

---

## IMP-002 — Completed: `--yes` now works in `spex new` (Priority: P0)

**Status:** Completed. The CLI now uses `yes: bool`, and `src/scaffold/mod.rs` skips the confirmation prompt when `yes` is set.

**Location:** `src/scaffold/mod.rs`

**Note:** Kept here as a closed item because it was previously a real automation bug.

**Effort:** S

---

## IMP-003 — Automated tests exist, but coverage is still thin (Priority: P1)

**Problem:** The repository now has automated CLI tests in `tests/cli_tests.rs`, so the project no longer has zero tests. However, coverage is still light and remains concentrated in a few end-to-end paths.

**Location:** `project-wide`

**Proposed Solution:** Expand beyond the current CLI smoke tests with focused unit and integration coverage for the SDD and MCP layers, especially around state queries, filtering, and regression-prone command behavior.

**Effort:** L

---

## IMP-004 — Historical: CI/release workflows now exist (Priority: P1)

**Status:** Obsolete as written. The repository now has `.github/workflows/ci.yml` and `.github/workflows/release.yml`.

**Location:** `.github/workflows/`

**Note:** If pipeline coverage needs further work later, create a fresh item that describes the remaining gap rather than preserving this now-inaccurate claim.

**Effort:** M

---

## IMP-005 — Completed: `doctor --fix` now performs basic auto-fixes (Priority: P1)

**Status:** Completed. `spex doctor --fix` now attempts several concrete remediations, including creating missing project files and installing bundled skills when appropriate.

**Location:** `src/doctor/mod.rs` — `cmd_doctor` function

**Note:** Keep this as a closed historical item rather than an active backlog entry.

**Effort:** M

---

## IMP-006 — Completed: CONTRIBUTING guide exists (Priority: P1)

**Status:** Completed. `CONTRIBUTING.md` exists at the repository root and covers workflow, commit conventions, tests, and development setup.

**Location:** `CONTRIBUTING.md`

**Note:** Kept for historical traceability only.

**Effort:** S

---

## IMP-007 — Completed: `memory_get_all` respects `spec` scope (Priority: P1)

**Status:** Completed. `memory_get_all` now branches on `spec` and applies the scoped SQL query when a spec is provided.

**Location:** `src/sdd/memory.rs`

**Note:** Keep as a closed item because it was a real correctness issue.

**Effort:** S

---

## IMP-008 — MCP tool proliferation (27 aliases) (Priority: P2)

**Problem:** `tools/list` exposes 27 entries because the server registers three prefix aliases for each tool (`spec_*`, `slice_*`, `state_*`). This inflates the tool list shown to LLM agents, increases token usage, and causes confusion about canonical names.

**Location:** `src/mcp/server.rs` — tool registration

**Proposed Solution:** Keep only the `state_*` canonical names. Move `spec_*` and `slice_*` aliases behind a `--legacy-aliases` flag (default: off). Update the README and skill files to reference canonical names only. Emit a deprecation warning when a legacy alias is used.

**Effort:** M

---

## IMP-009 — Pagination is still incomplete across CLI list views (Priority: P2)

**Problem:** This is no longer universally true: the SDD/event layers support `LIMIT/OFFSET`, `spex trace` already has a `--limit`, and `spex pulse` supports time filtering. The remaining gap is that `spex spec list` and `spex task list` still do not expose pagination controls in the CLI.

**Location:** `src/sdd/spec.rs`, `src/sdd/task.rs`, `src/sdd/event.rs`

**Proposed Solution:** Add `--limit N` (default: 50) and `--offset N` (default: 0) flags to all list commands. Thread these through the SDD layer as SQL `LIMIT ? OFFSET ?` parameters. Add `--all` flag to bypass pagination.

**Effort:** M

---

## IMP-011 — Completed: `spex pulse` supports time-range filters (Priority: P2)

**Status:** Completed. The CLI now exposes `--since` and `--until` on `spex pulse`, and the event query layer supports both filters.

**Location:** `src/cli/pulse.rs`

**Note:** Kept as a closed item for historical context.

**Effort:** S

---

## IMP-012 — Structured logging exists, but adoption is partial (Priority: P2)

**Problem:** The project now initializes `tracing_subscriber` and uses `tracing` in the MCP server, so the repository no longer lacks structured logging entirely. The remaining gap is inconsistent adoption across the rest of the codebase.

**Location:** `project-wide`

**Proposed Solution:** Continue migrating internal diagnostics to `tracing` where structured logs add value, while keeping normal user-facing CLI output on stdout/stderr as appropriate.

**Effort:** M

---

## IMP-013 — Completed: release profile is tuned for smaller binaries (Priority: P3)

**Status:** Completed. `Cargo.toml` now defines a release profile with `opt-level = "z"`, `lto = true`, `codegen-units = 1`, `strip = true`, and `panic = "abort"`.

**Location:** `Cargo.toml`

**Note:** Keep as a closed historical item only.

**Effort:** S

---

## IMP-014 — `spex doctor` checks are still hard-coded in one module (Priority: P3)

**Problem:** The exact tuple-based implementation is gone, but the checks are still hard-coded as a fixed set of functions inside `src/doctor/mod.rs`. Adding or extending checks still requires editing the core module directly.

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
