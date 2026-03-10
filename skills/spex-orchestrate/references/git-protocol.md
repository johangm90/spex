# Git Protocol Reference

Git protocol for `spex-orchestrate`: zero git commands, delegation to `spex-gitops`,
and the branching opt-in flow.

---

## Core Rule

`spex-orchestrate` runs **zero git commands**. All git operations — branching,
committing, tagging, and PR creation — are delegated entirely to `spex-gitops`.

---

## Branching Opt-in Flow

Trigger this flow after the **first `make check` passes** for a slice:

1. Ask the human:
   > _"All gates are green. Would you like me to create a feature branch and open
   > a PR for this slice? I'll delegate that to @spex-gitops."_

2. If the human **confirms** → delegate to `spex-gitops` with:
   - Slice ID
   - Slice title
   - One-paragraph summary of changes

3. `spex-gitops` will:
   - Run `git checkout -b feat/<slice-id>-<slug>` (or the project's branch naming convention)
   - Commit all changes with a conventional commit message
   - Run `gh pr create` with the slice title and summary as the PR body

4. `spex-orchestrate` does **not**:
   - Run `git checkout`, `git add`, `git commit`, `git push`, or `gh pr create`
   - Inspect git log or diff output
   - Make decisions about branch names or commit message content

5. If the human **declines** branching → continue with archiving on the current branch.
   Emit `SliceCompleted` directly from `spex-orchestrate`.

---

## Slice Close-out Delegation

When the slice is complete and branching is requested, `spex-orchestrate` also
delegates CHANGELOG updates to `spex-gitops`:

```
ORCHESTRATOR → spex-gitops
TASK: changelog-SLICE-NNN
SLICE: SLICE-NNN
INPUTS: plan_SLICE-NNN (MCP), slice spec (MCP)
EXPECTED OUTPUT: CHANGELOG entry appended
DEADLINE GATE: make check must pass
---
Append a CHANGELOG entry for SLICE-NNN. Use the slice title as the section header.
Summarise the completed tasks in bullet points. Follow the project's existing
CHANGELOG format.
```

---

## Why Zero Git?

- Keeps `spex-orchestrate` stateless with respect to the repository.
- Prevents accidental commits of incomplete wave output.
- Ensures all commits follow the conventional commit standard enforced by `spex-gitops`.
- Separates delivery coordination (this skill) from repository hygiene (spex-gitops).

See also: `skills/_shared/conventions.md` § Git Protocol per Agent.
