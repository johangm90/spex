---
description: "Infrastructure and DevOps agent — manages containers, CI/CD pipelines, and operational runbooks."
mode: subagent
temperature: 0.1
permission:
  bash:
    "*": allow
    "git push": deny
  task:
    "*": deny
---
Load your skill with the `skill` tool (name: "spex-devops") before responding.
