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
- **16 bundled AI agent files** plus workflow skills (e.g. `grilling`) install in one command and work with [OpenCode](https://opencode.ai) out of the box. `spex setup` installs agents to `~/.config/opencode/agents` and bundled skills to `~/.agents/skills/<slug>/SKILL.md`. Separately, `skill-builder` scaffolds custom project skills for your team's stack.

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

### Windows

```powershell
iwr -useb https://github.com/johangm90/spex/releases/latest/download/install.ps1 | iex
```

Installs to `%LOCALAPPDATA%\spex` and adds it to your user `PATH` — no admin required. Supports x86\_64 and ARM64.

To pin a specific version:

```powershell
$env:SPEX_VERSION = "v0.6.0"
iwr -useb https://github.com/johangm90/spex/releases/latest/download/install.ps1 | iex
```

### Self-update

Once installed, keep spex up to date with:

```sh
spex update
```

Check for a new version without installing:

```sh
spex update --check
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
| `spex analyze <ID>` | Cross-artifact consistency check before implementation (exit 1 on blockers) |
| `spex task add ...` / `spex task list` | Manage tasks within a spec |
| `spex task export <SPEC> --to github\|markdown` | Project a spec's tasks to a ticket backend (`[tickets]` in `.spex/config.toml`) |
| `spex pulse` | Project status dashboard |
| `spex trace` | Append-only domain event log |
| `spex mcp serve` / `spex mcp setup` | Start the MCP server or write MCP config |
| `spex skill install --all` / `spex skill list` | Install or list bundled agents |
| `spex memory list ...` / `spex memory search ...` | Inspect agent memory entries |
| `spex doctor` | Run health checks |
| `spex update` | Update spex to the latest release (`--check` to only check) |

Run `spex --help` or `spex <command> --help` for the current command surface and flags.

---

## Bundled Agents

Install once with `spex setup`, then use from [OpenCode](https://opencode.ai):

| Agent | Mode | Role |
|-------|------|------|
| `spex-architect` | primary | Primary orchestrator — classifies requests, coordinates specialists, and manages state; never implements directly |
| `spec-writer` | subagent | Drafts complete spec/slice documents with acceptance criteria and open questions |
| `task-planner` | subagent | Decomposes approved specs into granular, independently verifiable tasks |
| `spec-analyzer` | subagent | Consistency gate — runs `spex analyze`, flags AC/task gaps and unresolved decisions before implementation |
| `adr-writer` | subagent | Captures architecture decisions in MADR format |
| `sdd-builder` | subagent | Implements tasks from approved specs, runs tests, and marks tasks done |
| `sdd-builder-deep` | subagent | `sdd-builder` on the reasoning-model tier — the router picks it for complex tasks (`SPEX_MODEL_DEEP`) |
| `skill-builder` | subagent | Scaffolds custom spex-compatible agents for any team's tech stack |
| `repo-explorer` | subagent | Maps the repo quickly and summarizes relevant files, flows, and conventions |
| `debugger` | subagent | Investigates failures, isolates root causes, and applies or recommends minimal fixes |
| `reviewer` | subagent | Reviews code for bugs, regressions, risks, and missing tests |
| `verifier` | subagent | QA gate — runs full validation, maps ACs to evidence, satisfies review requirements; never approves |
| `test-writer` | subagent | Adds or updates focused tests for new behavior, bug fixes, and regressions |
| `release-helper` | subagent | Prepares release readiness checks, PR summaries, and release notes |
| `security-reviewer` | subagent | Reviews code for auth, secret, input, and permission risks |

> **Need a specialist?** Ask `@spex-architect` to invoke `@skill-builder` and describe your stack. You'll get a custom agent tailored to your conventions in seconds.

---

## Workflow phases

`spex-architect` drives an explicit pipeline for non-trivial work:

```
Brief/Constitution → Clarify → Specify → Plan → Tasks → Analyze → Implement → Verify
```

- **Clarify** (`grilling` skill) records decisions as `resolved` vs. `needs_human_approval`; drafting is gated until the ledger clears.
- **Analyze** — `spex analyze <SPEC>` is a deterministic check (AC↔task coverage, unresolved decisions, ambiguity, dependency readiness); `@spec-analyzer` wraps it with judgment.
- **Verify** — `@verifier` runs `validation_commands.full`, maps every AC to evidence, and satisfies the readiness requirements `@task-planner` seeded. Only a human's approval, relayed by `@spex-architect` via `state_readiness_approve`, closes the spec.

## Model tiers (complexity router)

`@spex-architect` calls the `state_workflow_classify` MCP tool on each task and routes by tier:

| Tier | Flow | Builder |
|------|------|---------|
| `trivial` | act directly, no spec | `@sdd-builder` |
| `standard` | grill → delegate | `@sdd-builder` |
| `complex` | full spec → analyze → SDD | `@sdd-builder-deep` |

Set model tiers in your host (env vars, consumed by agent frontmatter):

```sh
export SPEX_MODEL_FAST=…       # repo-explorer, spex-daily
export SPEX_MODEL_BUILD=…      # sdd-builder, verifier, test-writer, debugger
export SPEX_MODEL_REASONING=…  # spex-architect, spec-writer, spec-analyzer
export SPEX_MODEL_DEEP=…       # sdd-builder-deep (unset → router falls back to sdd-builder)
```

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
