# Contributing to spex

Thank you for your interest in contributing! This guide covers the development workflow, code conventions, and the agent skill development process.

## Table of Contents

- [Development Setup](#development-setup)
- [Workflow](#workflow)
- [Commit Convention](#commit-convention)
- [Running Tests](#running-tests)
- [Code Style](#code-style)
- [Agent Skill Development](#agent-skill-development)
- [Reporting Issues](#reporting-issues)

---

## Development Setup

**Prerequisites:** Rust 1.75+, `cargo`, `git`.

```sh
git clone https://github.com/OWNER/spex   # replace OWNER with actual handle
cd spex
cargo build
cargo run -- --help
```

For cross-compilation (Linux ARM64 release builds):

```sh
cargo install cross
cross build --target aarch64-unknown-linux-gnu --release
```

---

## Workflow

1. Fork the repository and create a feature branch:
   ```sh
   git checkout -b feat/my-feature
   ```
2. Make your changes. Keep commits small and focused.
3. Run the full check suite before pushing:
   ```sh
   cargo fmt --check
   cargo clippy -- -D warnings
   cargo test
   cargo build --release
   ```
4. Open a pull request against `main`. Fill in the PR template.

---

## Commit Convention

We follow [Conventional Commits](https://www.conventionalcommits.org/):

```
<type>(<scope>): <subject>

[optional body]

[optional footer]
```

**Types:** `feat`, `fix`, `docs`, `refactor`, `test`, `chore`, `ci`

**Examples:**
```
feat(cli): add --since/--until filter to spex pulse
fix(scaffold): honour --yes flag in spex new
docs: rewrite README with architecture diagram
chore(deps): bump sqlx to 0.8
```

Breaking changes: append `!` after the type or add `BREAKING CHANGE:` in the footer.

---

## Running Tests

```sh
cargo test                    # all tests
cargo test -- --nocapture     # with stdout
cargo test sdd::               # filter to sdd module
```

Tests use an in-memory SQLite database (`sqlite::memory:`) — no setup required.

---

## Code Style

- `cargo fmt` is enforced in CI. Run it before committing.
- `cargo clippy -- -D warnings` is enforced in CI. Fix all warnings.
- Keep functions short; prefer named helper functions over long `match` chains.
- In MCP server code (`src/mcp/server.rs`), only use `state_*` canonical tool names in new code. Legacy aliases (`spec_*`, `slice_*`) exist for backwards compatibility only.
- All MCP state must be stored via the MCP tools — never write orchestration files to the repository.

---

## Agent Skill Development

Bundled agent skills live in `skills/` (e.g. `skills/spex-orchestrate/SKILL.md`). They are embedded into the binary via `include_dir!` and installed to `~/.config/opencode/skills/` by `spex setup` / `spex skill install --all`.

**To add or update a skill:**

1. Edit or create `skills/<skill-name>/SKILL.md`.
2. If adding a new skill, add it to the `BUNDLED_SKILLS` list in `src/skills_mgr.rs`.
3. Run `spex skill install --all` to push the updated skill to your local OpenCode config.
4. Test by opening OpenCode and invoking the skill.

**Skill file format:** Plain Markdown. No special syntax required. The first `# Heading` is treated as the skill title. Skills are read-only from the agent's perspective — agents cannot modify their own skill files.

---

## Reporting Issues

Please include:
- `spex --version` output
- OS and architecture (`uname -a`)
- `spex doctor` output
- Steps to reproduce

For MCP server issues, run with `RUST_LOG=debug` (once structured logging is implemented) or capture stderr from `spex mcp serve`.
