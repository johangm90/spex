# Contributing to spex

Thank you for your interest in contributing! This guide covers the development workflow, code conventions, and how bundled agents differ from custom generated skills.

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
git clone https://github.com/johangm90/spex
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
- In MCP server code (`src/mcp/server.rs`), follow the current canonical MCP tool surface: state operations use `state_*` names and memory operations use `memory_*` names.
- All MCP state must be stored via the MCP tools — never write orchestration files to the repository.

---

## Agent Skill Development

This repository currently uses both bundled agents and custom generated skills:

- Bundled agent files live in `agents/*.md`, are embedded into the binary via `include_dir!`, and install to `~/.config/opencode/agents` when you run `spex setup` or `spex skill install --all`.
- Custom project skills are separate `SKILL.md` files under `~/.config/opencode/skills/<slug>/`, typically scaffolded by `skill-builder` for a specific stack or codebase.

**To add or update a bundled agent:**

1. Edit or create `agents/<agent-name>.md`.
2. Run `spex skill install --all` to copy the updated bundled agents into your local OpenCode config.
3. Run `spex skill list` to confirm the markdown files are installed.
4. Test by opening OpenCode and invoking the agent.

**Bundled agent file format:** Plain Markdown. Keep instructions explicit and implementation-backed; avoid documenting behavior the CLI or MCP server does not currently implement.

---

## Reporting Issues

Please include:
- `spex --version` output
- OS and architecture (`uname -a`)
- `spex doctor` output
- Steps to reproduce

For MCP server issues, capture stderr from `spex mcp serve` and include the command or client flow that triggered the problem.
