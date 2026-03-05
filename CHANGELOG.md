# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

### Added
- `spex setup` one-time global command: installs all bundled agent skills and writes MCP config
- `PRD.md` support: `spex new` and `spex init` create a structured PRD template in the project root
- MCP tool `state_prd_get`: reads `PRD.md` from disk, returns content + `is_template` flag
- `spex doctor --fix`: automatically creates missing `.spex/`, `PRD.md`, `opencode.json`, and installs skills
- `spex pulse --since` / `--until`: filter recent activity by time range
- `[profile.release]` in `Cargo.toml`: optimised binary size (`opt-level = "z"`, LTO, strip, `panic = "abort"`)
- CI workflow (`.github/workflows/ci.yml`): fmt, clippy, build, test on Ubuntu and macOS
- Release workflow (`.github/workflows/release.yml`): 4-target matrix (x86_64/aarch64 × linux/macOS), GitHub Release with SHA-256 checksums
- `install.sh`: platform-detecting shell installer with SHA-256 verification and `--prefix` support
- WAL mode + `PRAGMA synchronous=NORMAL` enabled on SQLite pool open
- `.gitignore` at project root

### Changed
- `spex new` / `spex init`: no longer create `docs/specs/` or `docs/adr/` directories
- `opencode.json` MCP entry format updated to array-command style: `"command": ["spex", "mcp", "serve"]`
- `memory_get_all` now scopes SQL query to the provided `spec` parameter, preventing cross-spec memory contamination
- `spex doctor` checks updated: `check_constitution()` replaced with `check_prd()` (checks for `PRD.md` and template detection)

### Removed
- `spex law` command and all constitution concepts removed; superseded by `PRD.md`
- `constitution_get` MCP tool removed; replaced by `state_prd_get` (legacy alias `constitution_get` retained for backwards compatibility)
- Explicit `BEGIN TRANSACTION` / `COMMIT` from migration `20260305000000_memory_scope.sql` (conflicted with sqlx's own transaction wrapping)

### Fixed
- `spex new --yes` flag was silently ignored (`_yes` parameter never read); now correctly skips interactive prompts
- `spex law freeze` was a no-op (removed along with the `law` command)

## [0.1.0] - 2026-03-05

### Added
- Initial release of `spex` CLI
- Spec lifecycle management: `draft → approved → in_progress ⇄ paused → done`
- MCP JSON-RPC server over stdio (`spex mcp serve`) for shared agent state
- SQLite state database at `.spex/state.db` with automatic migrations
- 16 bundled `spex-*` agent skills embedded via `include_dir!`
- `spex new` / `spex init` project scaffolding
- `spex spec`, `spex task`, `spex plan` CRUD commands
- `spex pulse` project dashboard
- `spex trace` domain event log viewer
- `spex doctor` health checks
- `spex skill install` / `spex skill list` skill management
