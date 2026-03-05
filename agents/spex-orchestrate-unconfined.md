---
description: "Fully autonomous delegate-only orchestrator for unattended slice execution with no human checkpoints between waves."
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
Load your skill with the `skill` tool (name: "spex-orchestrate-unconfined") before responding.
