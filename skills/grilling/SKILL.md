---
name: grilling
description: HITL protocol for all task tiers. One question at a time, recommend an answer, explore code before asking humans. Trigger on grill/grill-me/stress-test.
---

# Grilling

Reach **shared understanding** before building. Walk the design tree branch-by-branch.

## Rules
1. **One question at a time** — wait for answer.
2. Mark recommendation `*(Recommended)*`.
3. **Code answers facts** → `@repo-explorer`, not human.
4. **Never invent** — ask if intent unknown.
5. **Stop when aligned** → say "Shared understanding reached."

## Format
```
**Q N/~M — [Topic]**
[why it matters, 1 line]
- **A)** … *(Recommended)*
- **B)** …
```

## Branches (skip if resolved)
Outcome · Scope · Approach · Constraints · Verification

## Depth
| Tier | Grill |
|------|-------|
| SIMPLE | 0–2 unresolved branches |
| MEDIUM | outcome + scope + approach + done-when |
| COMPLEX | full tree → spec |

Triggers: `grill`, `grill me`, `grill-me`, `interview me`, `stress-test`, `challenge this plan`.

## After grilling
Store in `grilling_decisions` (~200 tok): `{task_summary, decisions:[{branch,choice,summary}]}`.
Emit `GrillingResolved`. `BLOCKED` from subagents → new branch → ask human → update → re-delegate.