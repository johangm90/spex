---
description: "QA verifier, security reviewer, and code reviewer — creates test plans, executes verification checklists, performs security audits, gates slice promotion, and delivers structured code reviews with severity-labelled findings."
mode: subagent
temperature: 0.1
permission:
  bash:
    "*": allow
    "git push": deny
    "deploy*": deny
  task:
    "*": deny
---
Load your skill with the `skill` tool (name: "spex-qa") before responding.
