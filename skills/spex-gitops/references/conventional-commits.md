# Conventional Commits Reference

Source: https://www.conventionalcommits.org/en/v1.0.0/

---

## Format

```
<type>(<scope>): <description>

[optional body]

[optional footer(s)]
```

---

## Valid Types

| Type | When to use |
|------|-------------|
| `feat` | A new feature or capability |
| `fix` | A bug fix |
| `docs` | Documentation only changes |
| `test` | Adding or correcting tests |
| `refactor` | Code change that neither fixes a bug nor adds a feature |
| `chore` | Maintenance tasks (deps, build scripts, tooling) |
| `ci` | CI/CD pipeline changes |
| `perf` | Performance improvement |

**No other types are valid.** If a candidate type is not in this table, map it to the closest match or use `chore`.

---

## Scope Conventions

Scope is optional but strongly recommended when the change is confined to one domain area.

| Scope | Meaning |
|-------|---------|
| `ui` | Frontend / UI components |
| `api` | Backend API layer |
| `db` | Database schema or migrations |
| `auth` | Authentication / authorisation |
| `infra` | Infrastructure / DevOps |
| `changelog` | CHANGELOG file updates |
| `deps` | Dependency updates |
| `config` | Configuration changes |
| `ci` | CI/CD pipeline files |

Use the slice ID as scope when the change maps 1-to-1 to a slice, e.g. `feat(SLICE-021):`.

---

## Subject Line Rules

- Maximum **72 characters** (including `type(scope): ` prefix) — hard limit, no exceptions
- Use the **imperative mood**: "add", "fix", "remove", not "added", "fixed", "removed"
- **No capital letter** after the colon-space (lowercase first word)
- **No trailing period**
- Must **not** be vague: "update stuff", "fix bug", "changes" are all invalid

---

## Body Guidelines

- Separate from subject line with a **blank line**
- Explain **why** the change was made, not what was changed (the diff shows that)
- Wrap lines at **100 characters** for readability
- May include motivation, trade-offs considered, or context for future readers
- Reference the slice/task that drove the change

---

## SLICE / TASK / ADR Reference Requirement

**Every commit body or footer must contain at least one traceable reference:**

```
Refs: SLICE-021
Refs: SLICE-021 / TASK-021-8
Refs: ADR-005
```

A commit without a reference is incomplete and should be flagged.

---

## Breaking Changes

Append `!` after the type/scope for breaking changes:

```
feat(api)!: rename /users endpoint to /accounts — Refs: SLICE-019
```

Or use a `BREAKING CHANGE:` footer:

```
BREAKING CHANGE: /users endpoint removed; use /accounts instead
```

---

## Examples

### ✅ Good commits

```
feat(ui): add dark-mode toggle to settings panel — Refs: SLICE-012

Users reported eye strain on the existing white background. This adds
a system-preference-aware toggle stored in localStorage.

Refs: SLICE-012 / TASK-012-3
```

```
fix(auth): prevent token refresh loop on 401 with expired refresh token

The previous implementation retried indefinitely. Now we clear the
session and redirect to login after a single failed refresh attempt.

Refs: SLICE-018 / TASK-018-2
```

```
docs(changelog): SLICE-021 — extended agent team — Refs: SLICE-021
```

### ❌ Bad commits

```
fix: stuff                           # vague, no reference
Update the thing                     # no type/scope, capitalised, no reference
feat: Add new feature.               # trailing period, capitalised first word
feat(ui): implement the entire new dashboard redesign for Q1 which includes dark mode, mobile layout, and user preferences panel   # >72 chars
chore: misc cleanup                  # vague
```

---

## Subject Line Length Check

Count characters before committing:

```bash
echo -n "feat(ui): add dark-mode toggle to settings panel" | wc -c
# → 49  ✅ within limit
```

If the count exceeds 72, shorten the description — never abbreviate the type or scope.
