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
6. **Split by decider.** Answer accepted from the human → `resolved` (`by: human`). Human says "you decide"/"tú decides"/"your call" → apply recommendation → `resolved` (`by: recommendation`). Reversible, low-blast-radius default the human hasn't seen → still ask; only auto-apply on explicit deferral.
7. A branch that needs a human sign-off you never got → `needs_human_approval` (never silently resolved).

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
Store in `grilling_decisions` (~250 tok):
```
{
  task_summary,
  resolved: [{branch, choice, summary, by: "human"|"recommendation"}],
  needs_human_approval: [{branch, question, options:[...], recommendation}]
}
```
Emit `GrillingResolved`. Only say "Shared understanding reached." when `needs_human_approval` is empty.
`BLOCKED` from subagents → new branch → ask human → move to `resolved` → re-delegate.

## Decision ledger gate
`needs_human_approval` non-empty → the orchestrator presents each entry to the human (one at a time, options + recommendation) and does **not** advance to `@spec-writer` / `@task-planner` until it clears. `@spec-writer` mirrors both lists into the spec's `## Clarifications` and adds an `Open Questions` checkbox per unapproved entry.