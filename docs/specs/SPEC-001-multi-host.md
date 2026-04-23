# SPEC-001 — Compatibilidad multi-tool con host profiles y rutas estándar

Status: implemented
Priority: P0

## Overview

Extend `spex` so setup, agent installation, and MCP configuration work across multiple AI tool hosts instead of assuming OpenCode-only defaults.

The implemented host set is:
- OpenCode
- GitHub Copilot CLI
- VS Code
- Pi / pi-subagents

## Goals

- Introduce explicit host profiles with canonical names and standard paths.
- Make bundled-agent installation host-aware, including Copilot's `.agent.md` format.
- Make MCP config setup host-aware for global and project-local layouts.
- Keep backward compatibility for the default OpenCode workflow.

## Acceptance criteria

1. A host domain module exists with supported hosts and path/profile metadata.
2. CLI path helpers cover OpenCode, Copilot, and the shared `~/.agents/skills` location.
3. `spex setup` supports host selection and can configure multiple supported hosts.
4. `spex mcp setup` writes the correct config shape for OpenCode, Copilot, and VS Code.
5. `spex skill install --all` and `spex skill list` respect host-specific agent directories and file extensions.
6. Bundled agent installation strips OpenCode-only frontmatter when installing Copilot `.agent.md` files.
7. Doctor and prompt discovery understand the host-aware bundled-agent locations.
8. CLI help text documents bundled-agent paths separately from generated custom skills under `~/.agents/skills/<slug>/SKILL.md`.

## Implemented artifacts

- `src/host/mod.rs`
- `src/cli/util.rs`
- `src/cli/mcp_cmd.rs`
- `src/cli/skill_cmd.rs`
- `src/skills_mgr/mod.rs`
- `src/doctor/mod.rs`
- `src/main.rs`

## Verification

- Host profile unit coverage in `src/host/mod.rs`
- MCP host/config coverage in `src/cli/mcp_cmd.rs`
- Bundled-agent installation coverage in `src/skills_mgr/mod.rs`
- Help text coverage in `src/main.rs`
