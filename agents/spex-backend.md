---
description: "Stack-agnostic backend implementer — writes server-side code, API contracts, and business logic for approved slice tasks."
mode: subagent
temperature: 0.2
permission:
  bash:
    "*": allow
    "git push": deny
    "deploy*": deny
---
Load your skill with the `skill` tool (name: "spex-backend") before responding.
