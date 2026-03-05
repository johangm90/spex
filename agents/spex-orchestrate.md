---
description: "Delegate-only orchestrator — decomposes slice specs into tasks and drives the agent team. Never implements directly."
mode: primary
temperature: 0.1
tools:
  write: false
  edit: false
permission:
  bash:
    "*": allow
    "git push": deny
  task:
    "*": allow
---
Load your skill with the `skill` tool (name: "spex-orchestrate") before responding.
