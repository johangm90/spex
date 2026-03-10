# CHANGELOG Patterns Reference — spex-gitops

Canonical patterns for writing and maintaining CHANGELOG files in spex projects. Default format: **Keep a Changelog** (https://keepachangelog.com/en/1.1.0/), compatible with `standard-version` and `semantic-release`.

---

## Keep a Changelog — Structure

```markdown
# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

### Added
- ...

### Changed
- ...

### Fixed
- ...

## [1.4.0] — 2026-03-10

### Added
- ...

## [1.3.2] — 2026-02-14

### Fixed
- ...

[Unreleased]: https://github.com/org/repo/compare/v1.4.0...HEAD
[1.4.0]: https://github.com/org/repo/compare/v1.3.2...v1.4.0
[1.3.2]: https://github.com/org/repo/compare/v1.3.1...v1.3.2
```

### Rules

1. **Always have an `[Unreleased]` section** at the top — new changes accumulate here between releases.
2. **Entry per user-facing change**, not per commit. Multiple commits for the same feature → one entry.
3. **Link comparison URLs** at the bottom — never omit them.
4. **Date format:** ISO-8601 (`YYYY-MM-DD`), not "March 2026".
5. **Entry text is user-facing:** write for an end user or API consumer, not a developer. Avoid internal jargon.

---

## Valid Sub-sections

| Sub-section | When to use |
|-------------|-------------|
| `### Added` | New features, new endpoints, new commands |
| `### Changed` | Changes to existing features (backward-compatible) |
| `### Deprecated` | Features that will be removed in a future release |
| `### Removed` | Features removed in this version |
| `### Fixed` | Bug fixes |
| `### Security` | Security fixes — **always use this sub-section for security issues**, never bury them in Fixed |

---

## Slice-to-CHANGELOG Mapping

Every merged slice produces exactly one CHANGELOG entry block. Use this pattern:

```markdown
### Added
- **SLICE-021 — Extended Agent Team:** Added `spex-db`, `spex-devops`, `spex-ai-eng`, and `spex-mobile`
  as first-class skills with rich reference material and canonical code examples.

### Changed
- **SLICE-021:** `spex-orchestrate` now routes database tasks to `spex-db` and infrastructure tasks to
  `spex-devops` automatically based on task type.
```

**Format:** `**SLICE-NNN — <Slice Title>:** <User-facing description of the change.>`

---

## Full Example — Before and After a Release

### [Unreleased] block (accumulating changes)

```markdown
## [Unreleased]

### Added
- **SLICE-025 — User Notifications:** Real-time push notifications for order status changes via
  WebSocket. Requires notification permission on first visit. ([#142](https://github.com/org/repo/pull/142))
- **SLICE-024 — Dark Mode:** System-preference-aware dark mode toggle stored in localStorage.
  ([#138](https://github.com/org/repo/pull/138))

### Fixed
- **SLICE-026:** Prevent infinite token refresh loop when refresh token has expired; now redirects
  to login. ([#145](https://github.com/org/repo/pull/145))
```

### After cutting release v1.5.0

```markdown
## [1.5.0] — 2026-03-10

### Added
- **SLICE-025 — User Notifications:** Real-time push notifications for order status changes via
  WebSocket. Requires notification permission on first visit.
- **SLICE-024 — Dark Mode:** System-preference-aware dark mode toggle stored in localStorage.

### Fixed
- **SLICE-026:** Prevent infinite token refresh loop when refresh token has expired; now redirects
  to login.
```

---

## Commit Convention for CHANGELOG Updates

```
docs(changelog): SLICE-NNN — <slice title>

Adds Unreleased entry for <brief description of what changed>.

Refs: SLICE-NNN
```

Example:
```
docs(changelog): SLICE-025 — user notifications

Adds Unreleased ### Added entry for WebSocket push notification feature.

Refs: SLICE-025
```

---

## standard-version / semantic-release Integration

If the project uses `standard-version` or `semantic-release`, the CHANGELOG is auto-generated from commit messages. In this case:

1. **Do not manually edit** the version sections — they are auto-generated.
2. **Do manually maintain** the `[Unreleased]` section for human-readable summaries.
3. **Commit type → CHANGELOG section mapping:**

| Commit type | CHANGELOG section |
|-------------|------------------|
| `feat` | `### Features` (semantic-release) / `### Added` (Keep-a-Changelog) |
| `fix` | `### Bug Fixes` / `### Fixed` |
| `perf` | `### Performance Improvements` |
| `refactor` | not included (internal, not user-facing) |
| `docs`, `chore`, `test`, `ci` | not included |
| `feat!` / `BREAKING CHANGE` | `### BREAKING CHANGES` — always at the top |

---

## What NOT to Put in a CHANGELOG

| Bad entry | Problem | Fix |
|-----------|---------|-----|
| `Updated dependencies` | Not user-facing | Omit unless a dep upgrade changes behaviour |
| `Refactored auth module` | Internal, not user-facing | Omit |
| `Fixed typo in README` | Trivial, not user-facing | Omit |
| `WIP: dark mode` | Never merge WIP | Never appears |
| `Fix bug` | Too vague | Describe the specific bug and its impact |
| `Implement SLICE-024` | Opaque to end users | Use `Dark Mode: ...` |

---

## Security Entry Pattern

Security entries must always use the `### Security` sub-section and include a CVE or advisory reference when available:

```markdown
### Security
- **CVE-2026-XXXXX:** Patched SQL injection vulnerability in the order search endpoint. Update
  immediately if running version < 1.4.3. ([#147](https://github.com/org/repo/pull/147))
```
