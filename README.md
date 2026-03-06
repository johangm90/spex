# spex

> **Spec-Driven Development for AI-assisted teams.**  
> Define specs, coordinate agents, track progress — all from your terminal.

[![CI](https://github.com/johangm90/spex/actions/workflows/ci.yml/badge.svg)](https://github.com/johangm90/spex/actions/workflows/ci.yml)
[![Release](https://img.shields.io/github/v/release/johangm90/spex?style=flat-square)](https://github.com/johangm90/spex/releases/latest)
[![License](https://img.shields.io/badge/license-MIT-lightgrey?style=flat-square)](LICENSE)
[![Rust](https://img.shields.io/badge/rust-stable-orange?style=flat-square)](https://www.rust-lang.org)

---

## What is spex?

`spex` is a single Rust binary that gives developers and AI agents a **shared, persistent state store** for coordinating feature delivery.

- **Specs** are the unit of work — named feature slices with a human-gated lifecycle (`draft → approved → in_progress → done`).
- **Agents** share state through an embedded **MCP (Model Context Protocol)** server backed by a local SQLite database at `.spex/state.db`.
- **10 bundled AI agent skills** (`spex-architect`, `spex-orchestrate`, `spex-backend`, `spex-frontend`, `spex-qa`, `spex-db`, `spex-devops`, `spex-gitops`, `spex-mobile`, `spex-ai-eng`) install in one command and work with [OpenCode](https://opencode.ai) out of the box.

---

## Installation

### One-liner (macOS and Linux)

```sh
curl -fsSL https://github.com/johangm90/spex/releases/latest/download/install.sh | sh
```

Supports: macOS Apple Silicon · macOS Intel · Linux x86\_64 · Linux ARM64

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

# 2. Install the 10 bundled agent skills into OpenCode
spex setup

# 3. Add and approve a spec
spex spec add SPEC-001 "User authentication" -p P0
spex spec approve SPEC-001

# 4. Decompose it into tasks
spex plan build SPEC-001

# 5. Open OpenCode — the MCP server starts automatically
#    Ask @spex-orchestrate to begin work on SPEC-001

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
| `spex setup` | Install all bundled agent skills |
| `spex spec add <ID> <TITLE>` | Create a new spec |
| `spex spec approve <ID>` | Human-approve a spec (required before agents can start) |
| `spex spec list` | List all specs |
| `spex spec show <ID>` | Show spec details and tasks |
| `spex plan build <ID>` | Interactively add tasks to a spec |
| `spex pulse` | Project status dashboard |
| `spex trace` | Append-only domain event log |
| `spex doctor` | Run 7 health checks |
| `spex mcp serve` | Start the MCP stdio server (auto-started by OpenCode) |

Run `spex <command> --help` for full flags and options.

---

## Bundled Agent Skills

Install once with `spex setup`, then use from [OpenCode](https://opencode.ai):

| Agent | Role |
|-------|------|
| `spex-architect` | PRD, ADRs, slice specs, product discovery |
| `spex-orchestrate` | Decomposes specs, delegates tasks, gates progress |
| `spex-backend` | Server-side code, APIs, business logic |
| `spex-frontend` | Web UI, design tokens, components |
| `spex-mobile` | React Native / Flutter apps |
| `spex-db` | Schema design, migrations |
| `spex-devops` | CI/CD, containers, infra |
| `spex-qa` | Tests, security reviews, acceptance gates |
| `spex-gitops` | Commits, branches, PRs, CHANGELOG |
| `spex-ai-eng` | LLM integration, RAG, prompt engineering |

---

## Contributing

```sh
git clone https://github.com/johangm90/spex.git
cd spex
cargo build
cargo test
cargo clippy -- -D warnings
```

See [CONTRIBUTING.md](CONTRIBUTING.md) for the full guide.

---

## License

[MIT](LICENSE)
