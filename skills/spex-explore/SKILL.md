---
name: "spex-explore"
description: "DEPRECATED — read-first exploration is now expected of every agent. Do not load this skill."
license: "MIT"
compatibility: "opencode"
---

# ⚠️ Deprecated: spex-explore

This skill has been dissolved — its read-first exploration discipline is now expected
of **every** `spex-*` agent.

All agents must read the codebase, PRD, and relevant MCP state before producing any
output. No dedicated exploration agent is needed.

**Action:** Do not load this skill. Use the appropriate specialist agent directly
(`spex-backend`, `spex-architect`, `spex-db`, etc.) — they all read first.
