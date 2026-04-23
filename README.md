# spex

> **Spec-driven coordination for AI-assisted software delivery.**  
> Define work, share state, coordinate specialist agents, and track progress from your terminal.

[![CI](https://github.com/johangm90/spex/actions/workflows/ci.yml/badge.svg)](https://github.com/johangm90/spex/actions/workflows/ci.yml)
[![Release](https://img.shields.io/github/v/release/johangm90/spex?style=flat-square)](https://github.com/johangm90/spex/releases/latest)
[![License](https://img.shields.io/badge/license-MIT-lightgrey?style=flat-square)](LICENSE)
[![Rust](https://img.shields.io/badge/rust-stable-orange?style=flat-square)](https://www.rust-lang.org)

---

## What is spex?

`spex` is a single Rust binary that gives developers and AI agents a **shared, persistent state layer** for coordinating software delivery.

- **Specs** are the unit of work — named feature slices with a human-gated lifecycle (`draft → approved → in_progress → done`).
- **Agents** share state through an embedded **MCP (Model Context Protocol)** server backed by a local SQLite database at `.spex/state.db`.
- **13 bundled AI agent files** install in one command and work with [OpenCode](https://opencode.ai) out of the box. `spex setup` installs them to `~/.config/opencode/agents`. Separately, `skill-builder` scaffolds custom project skills for your team's stack under `~/.agents/skills/<slug>/SKILL.md`.

---

## Installation

### One-liner (macOS and Linux)

```sh
curl -fsSL https://github.com/johangm90/spex/releases/latest/download/install.sh | sh
```

Installs to `~/.local/bin` — no `sudo` required. Supports: macOS Apple Silicon · macOS Intel · Linux x86\_64 · Linux ARM64

For a system-wide install:

```sh
curl -fsSL https://github.com/johangm90/spex/releases/latest/download/install.sh | sh -s -- --prefix /usr/local
```

### From source

```sh
git clone https://github.com/johangm90/spex.git
cd spex
cargo install --path .
```

---

## Quick Start

```sh
# 1. Create a new project (or run `spex init` inside an existing one)
spex new my-project
cd my-project

# 2. Install the bundled agents into OpenCode
spex setup

# 3. Add and approve a spec
spex spec add SPEC-001 "User authentication" -p P0
spex spec approve SPEC-001

# 4. Decompose it into tasks
spex plan build SPEC-001

# 5. Open OpenCode — the MCP server starts automatically
#    Ask @spex-architect to begin work on SPEC-001

# 6. Monitor progress
spex pulse
```

---

## Spec Lifecycle

```
draft ──► approved ──► in_progress ⇄ paused ──► done
            ▲
      human gate (spex spec approve)
```

Human approval is enforced by the CLI — no agent can skip it.

---

## Commands

| Command | Description |
|---------|-------------|
| `spex new <NAME>` | Bootstrap a new project |
| `spex init` | Initialise spex in an existing project |
| `spex setup` | Install bundled agents and write OpenCode MCP config |
| `spex spec add <ID> <TITLE>` | Create a new spec |
| `spex spec approve <ID>` | Human-approve a spec (required before agents can start) |
| `spex spec start <ID>` / `spex spec done <ID>` | Move a spec through its lifecycle |
| `spex spec list` | List all specs |
| `spex spec show <ID>` | Show spec details and tasks |
| `spex plan build <ID>` | Interactively add tasks to a spec |
| `spex task add ...` / `spex task list` | Manage tasks within a spec |
| `spex pulse` | Project status dashboard |
| `spex trace` | Append-only domain event log |
| `spex mcp serve` / `spex mcp setup` | Start the MCP server or write MCP config |
| `spex skill install --all` / `spex skill list` | Install or list bundled agents |
| `spex memory list ...` / `spex memory search ...` | Inspect agent memory entries |
| `spex doctor` | Run health checks |

Run `spex --help` or `spex <command> --help` for the current command surface and flags.

---

## Bundled Agents

Install once with `spex setup`, then use from [OpenCode](https://opencode.ai):

| Agent | Mode | Role |
|-------|------|------|
| `spex-architect` | primary | Primary engineering copilot — inspects, decides, executes low-risk work directly, and uses SDD workflows for larger changes |
| `spec-writer` | subagent | Drafts complete spec/slice documents with acceptance criteria and open questions |
| `task-planner` | subagent | Decomposes approved specs into granular, independently verifiable tasks |
| `adr-writer` | subagent | Captures architecture decisions in MADR format |
| `sdd-builder` | subagent | Implements tasks from approved specs, runs tests, and marks tasks done |
| `skill-builder` | subagent | Scaffolds custom spex-compatible agents for any team's tech stack |
| `repo-explorer` | subagent | Maps the repo quickly and summarizes relevant files, flows, and conventions |
| `debugger` | subagent | Investigates failures, isolates root causes, and applies or recommends minimal fixes |
| `reviewer` | subagent | Reviews code for bugs, regressions, risks, and missing tests |
| `test-writer` | subagent | Adds or updates focused tests for new behavior, bug fixes, and regressions |
| `release-helper` | subagent | Prepares release readiness checks, PR summaries, and release notes |
| `security-reviewer` | subagent | Reviews code for auth, secret, input, and permission risks |

> **Need a specialist?** Ask `@spex-architect` to invoke `@skill-builder` and describe your stack. You'll get a custom agent tailored to your conventions in seconds.

---

## Contributing

```sh
git clone https://github.com/johangm90/spex.git
cd spex

# Project-appropriate validation for this Rust repo
cargo build
cargo test
cargo clippy -- -D warnings
```

See [CONTRIBUTING.md](CONTRIBUTING.md) for the full guide.

---

## License

[MIT](LICENSE)
