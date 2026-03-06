---
name: "spex-orchestrate-unconfined"
description: "DEPRECATED — unconfined mode is now built into spex-orchestrate. Do not load this skill."
license: "MIT"
compatibility: "opencode"
---

# ⚠️ Deprecated: spex-orchestrate-unconfined

This skill has been absorbed as an **operating mode** of **`spex-orchestrate`**.

To run the orchestrator in unconfined (fully autonomous) mode, simply tell it:
> _"Run unconfined."_

The orchestrator will skip per-wave human confirmation checkpoints and run all waves
autonomously. It still halts if the same gate fails twice consecutively.

**Action:** Load `spex-orchestrate` instead of this skill.
