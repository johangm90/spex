# PR Template & Branch Naming Policy

---

## Branch Naming Policy

| Branch type | Pattern | Example |
|-------------|---------|---------|
| Slice feature | `slice/NNN-<kebab-slug>` | `slice/021-extended-agent-team` |
| Standalone feature | `feat/NNN-<kebab-slug>` | `feat/019-git-identity` |
| Bug fix | `fix/NNN-<kebab-slug>` | `fix/022-token-refresh-loop` |
| Chore / maintenance | `chore/<kebab-slug>` | `chore/update-deps-march-2026` |
| Documentation | `docs/<kebab-slug>` | `docs/adr-005-state-backend` |
| Release | `release/vMAJOR.MINOR.PATCH` | `release/v1.4.0` |

**Rules:**
- Slug is the slice/task title in kebab-case, **maximum 40 characters**
- All lowercase, hyphens only (no underscores, no spaces)
- Branch must diverge from `main` (or the agreed base branch), never created on `main`
- If the branch already exists and is stale (>30 days, no commits), flag it before reusing

---

## PR Body Template

Use this template verbatim when generating a PR body. Replace all `<placeholder>` values.

```markdown
## Summary

- <One-sentence description of what this PR delivers>
- <Key design decision or trade-off made, if any>
- <Any notable out-of-scope items deferred to a future slice>

## Changes

<!-- File-level list of what changed and why -->
- `<path/to/file>` — <reason for change>
- `<path/to/file>` — <reason for change>

## Testing

<!-- How a reviewer can verify the behaviour -->
- [ ] `make check` passes locally
- [ ] <Specific manual step to verify the primary feature>
- [ ] <Edge case or error path to test>

## Checklist

- [ ] Commit messages conform to Conventional Commits spec
- [ ] Subject lines are ≤ 72 characters
- [ ] All commits reference at least one SLICE/TASK/ADR ID
- [ ] No direct commits to `main`
- [ ] No secrets or credentials in diff
- [ ] CHANGELOG updated (if user-facing change)
- [ ] PR targets the correct base branch
```

---

## Example Filled PR Body

```markdown
## Summary

- Implements the extended agent team for spex: adds spex-db, spex-devops, spex-ai-eng, and spex-mobile as first-class agents with full SKILL.md files.
- Chose a shared `_shared/conventions.md` reference rather than duplicating commit/artifact conventions in each skill file.
- On-call runbooks and mobile deep-link configuration deferred to SLICE-023.

## Changes

- `skills/spex-db/SKILL.md` — new database modeller skill
- `skills/spex-devops/SKILL.md` — new infrastructure/DevOps skill
- `skills/spex-ai-eng/SKILL.md` — new AI feature integrator skill
- `skills/spex-mobile/SKILL.md` — new mobile implementer skill
- `skills/_shared/conventions.md` — extracted shared git and artifact conventions

## Testing

- [ ] `make check` passes locally
- [ ] Each skill file loads without error in opencode
- [ ] `spex-orchestrate` correctly routes a sample DB task to `spex-db`

## Checklist

- [x] Commit messages conform to Conventional Commits spec
- [x] Subject lines are ≤ 72 characters
- [x] All commits reference SLICE-021
- [x] No direct commits to `main`
- [x] No secrets or credentials in diff
- [x] CHANGELOG updated
- [x] PR targets `main`
```

---

## `gh pr create` Command Pattern

```bash
gh pr create \
  --title "feat: SLICE-NNN — <slice title>" \
  --base main \
  --head slice/NNN-<slug> \
  --body "$(cat <<'EOF'
## Summary
...
## Changes
...
## Testing
...
## Checklist
...
EOF
)"
```

**Always use a HEREDOC** for the body to preserve Markdown formatting and avoid shell quoting issues.
