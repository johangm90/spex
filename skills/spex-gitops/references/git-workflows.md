# Git Workflows Reference — spex-gitops

Operational git patterns: pre-commit hooks, squash strategy, rebase guidelines, and branch lifecycle. All commands assume a standard git + GitHub workflow.

---

## Pre-commit Hook Setup

Enforce commit message format automatically before every `git commit`. Use **commitlint** (Node.js) or **pre-commit** (Python).

### commitlint (recommended for Node.js / TypeScript projects)

```bash
# Install
npm install --save-dev @commitlint/cli @commitlint/config-conventional husky

# Configure commitlint
echo "export default { extends: ['@commitlint/config-conventional'] };" > commitlint.config.mjs

# Set up Husky
npx husky init
echo "npx --no -- commitlint --edit \$1" > .husky/commit-msg
chmod +x .husky/commit-msg
```

`commitlint.config.mjs` — extended with spex-specific rules:

```javascript
export default {
  extends: ['@commitlint/config-conventional'],
  rules: {
    'header-max-length':  [2, 'always', 72],
    'subject-case':       [2, 'always', 'lower-case'],
    'subject-full-stop':  [2, 'never', '.'],
    'body-leading-blank': [2, 'always'],
    // Require Refs: in body/footer — custom rule
    'body-empty':         [1, 'never'],   // warn if no body (Refs: lives there)
  },
  // No custom types beyond the standard set
};
```

### PHP / Symfony projects — captainhook

```bash
composer require --dev captainhook/captainhook captainhook/plugin-composer
vendor/bin/captainhook install
```

`captainhook.json`:
```json
{
  "commit-msg": {
    "enabled": true,
    "actions": [
      {
        "action": "\\CaptainHook\\App\\Hook\\Message\\Action\\Beams",
        "options": {
          "subjectMinLength":    10,
          "subjectMaxLength":    72,
          "enforceConventional": true
        }
      }
    ]
  },
  "pre-commit": {
    "enabled": true,
    "actions": [
      { "action": "php vendor/bin/phpcs --standard=PSR12 src/" },
      { "action": "php vendor/bin/phpstan analyse src/ --level=8" }
    ]
  }
}
```

### pre-commit (Python / polyglot projects)

```yaml
# .pre-commit-config.yaml
repos:
  - repo: https://github.com/compilerla/conventional-pre-commit
    rev: v3.4.0
    hooks:
      - id: conventional-pre-commit
        stages: [commit-msg]
        args: [feat, fix, docs, test, refactor, chore, ci, perf]

  - repo: https://github.com/pre-commit/pre-commit-hooks
    rev: v4.6.0
    hooks:
      - id: trailing-whitespace
      - id: end-of-file-fixer
      - id: check-yaml
      - id: check-merge-conflict
      - id: detect-private-key
```

```bash
pip install pre-commit
pre-commit install --hook-type commit-msg
pre-commit install
```

---

## Squash Strategy

Decide squash vs. merge-commit vs. rebase before opening a PR. The decision lives in the PR description.

### Decision table

| Scenario | Strategy | Command |
|----------|----------|---------|
| Single-purpose slice, clean history | **Squash and merge** | GitHub "Squash and merge" button, or `git merge --squash` |
| Multiple logical units within one slice | **Rebase and merge** (preserves individual commits) | GitHub "Rebase and merge" button |
| Long-running feature branch with meaningful history | **Merge commit** | `git merge --no-ff slice/NNN-<slug>` |
| WIP / fixup commits present | Squash into one clean commit before PR | `git rebase -i HEAD~N` |

### Squash-merge commit message format

When squashing a feature branch, the final squash commit message must follow Conventional Commits:

```
feat(SLICE-021): extended agent team — spex-db, spex-devops, spex-ai-eng, spex-mobile

Adds four new skills to the spex agent framework, each with rich SKILL.md files
and deep reference material. Routes database tasks to spex-db and infrastructure
tasks to spex-devops based on task type.

Refs: SLICE-021
```

---

## Interactive Rebase Guidelines

Use `git rebase -i` to clean up a branch before opening a PR.

### When to rebase

- WIP, fixup, or "address review comments" commits are in the branch
- Commits are out of logical order
- The branch has diverged from `main` and needs to be updated

### Safe rebase workflow

```bash
# 1. Update main
git fetch origin
git checkout main
git pull --ff-only

# 2. Rebase feature branch on top of updated main
git checkout slice/021-extended-agent-team
git rebase origin/main

# 3. If conflicts arise, resolve and continue
# git rebase --continue  (after resolving conflicts)
# git rebase --abort     (to bail out entirely)

# 4. Interactive squash of WIP commits (last N commits)
git rebase -i HEAD~4
# In the editor: change 'pick' → 'squash' or 'fixup' for WIP commits
```

### Rebase rules

| Rule | Detail |
|------|--------|
| Never rebase a branch that has been pushed and shared | Only rebase local-only or your own feature branches |
| Never rebase `main` | `main` is the source of truth — never rewrite it |
| Always verify after rebase | `git log --oneline` — confirm history looks correct before pushing |
| Fixup vs. squash | `fixup` discards the WIP commit message; `squash` lets you edit the combined message |

---

## Branch Lifecycle

```
main
 └── slice/NNN-<slug>          ← created here, diverges from main
       ├── (feature commits)
       ├── (fixup commits)    ← squash before PR
       └── PR opened → reviewed → squash-merged → branch deleted
```

### Branch hygiene commands

```bash
# List merged branches (safe to delete)
git branch --merged main

# Delete a local merged branch
git branch -d slice/021-extended-agent-team

# Delete a remote merged branch (human decision only)
git push origin --delete slice/021-extended-agent-team

# Find stale branches (no commits in 30+ days)
git for-each-ref --sort=committerdate refs/remotes/ \
  --format='%(committerdate:short) %(refname:short)' | \
  awk -v cutoff="$(date -d '30 days ago' '+%Y-%m-%d')" '$1 < cutoff'
```

---

## Merge Conflict Resolution Protocol

1. **Do not resolve conflicts automatically** — flag them to the human.
2. Identify the conflicting files: `git status` → look for `both modified:`.
3. Present the conflict to the human: show both `HEAD` version and incoming version.
4. Apply the human's resolution.
5. Stage the resolved file: `git add <file>`.
6. Continue the rebase or merge: `git rebase --continue` or `git merge --continue`.
7. Verify the final state: `git log --oneline -5`.

---

## Tagging and Releasing

Tagging is the **human's responsibility**. `spex-gitops` generates the release notes but never creates tags.

### Release note generation

When the human asks for release notes, produce a summary of all `[Unreleased]` CHANGELOG entries since the last tag:

```bash
# Find the last tag
git describe --tags --abbrev=0
# → v1.4.0

# List commits since that tag
git log v1.4.0..HEAD --oneline --no-merges
```

Use the commit log and CHANGELOG `[Unreleased]` section to produce the release note body. Then present it to the human to create the tag and GitHub release manually.

### Tag format

```
v<MAJOR>.<MINOR>.<PATCH>
```

- `MAJOR` — breaking change (any `!` commit or `BREAKING CHANGE` footer)
- `MINOR` — new feature (`feat` commit)
- `PATCH` — bug fix (`fix` commit)
