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
| IMP-008 | Historical: MCP tool alias proliferation claim is obsolete | P2 | S | `src/mcp/server.rs` |
| IMP-009 | Historical: pagination gap claim is obsolete as written | P2 | S | `src/main.rs` |
| IMP-011 | Completed: `spex pulse` supports time-range filters | P2 | S | `src/cli/pulse.rs` |
| IMP-012 | Historical: structured logging adoption claim is obsolete | P2 | S | project-wide |
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

**Problem:** The repository no longer has zero tests, but coverage still relies mainly on inline `#[cfg(test)]` modules in `src/` rather than a broader mix of integration-style coverage. The current codebase contains many Rust unit and async tests, yet the remaining gap is still depth around cross-command and regression-prone paths.

**Location:** `project-wide`

**Proposed Solution:** Keep the current inline test coverage and add targeted higher-level tests where regressions are most likely, especially around CLI behavior, MCP tool wiring, and state-query edge cases.

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

## IMP-008 — Historical: MCP tool alias proliferation claim is obsolete (Priority: P2)

**Status:** Obsolete as written. The current `build_tools_list()` implementation exposes 20 canonical MCP tools split across `state_*` and `memory_*` names; the earlier "27 aliases" claim no longer matches the source.

**Location:** `src/mcp/server.rs` — tool registration

**Note:** If tool proliferation becomes a problem again, create a new backlog item based on the current MCP surface instead of preserving the retired alias description.

**Effort:** S

---

## IMP-009 — Historical: pagination gap claim is obsolete as written (Priority: P2)

**Status:** Obsolete as written. The current CLI already exposes `--limit` and `--offset` for `spex spec list`, `spex task list`, and `spex trace`, so the broad "pagination is still incomplete" claim is no longer accurate.

**Location:** `src/main.rs`

**Note:** If future pagination UX gaps remain, track them with a narrower item that names the exact command and missing behavior.

**Effort:** S

---

## IMP-011 — Completed: `spex pulse` supports time-range filters (Priority: P2)

**Status:** Completed. The CLI now exposes `--since` and `--until` on `spex pulse`, and the event query layer supports both filters.

**Location:** `src/cli/pulse.rs`

**Note:** Kept as a closed item for historical context.

**Effort:** S

---

## IMP-012 — Historical: structured logging adoption claim is obsolete (Priority: P2)

**Status:** Obsolete as written. No current `tracing_subscriber` or `tracing` usage was verified in `src/` or `Cargo.toml`, so this item describes a state the repository does not presently implement.

**Location:** `project-wide`

**Note:** If structured logging is introduced later, open a fresh backlog item describing the actual implementation gap at that time.

**Effort:** S

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
