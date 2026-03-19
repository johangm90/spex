# Task Decomposition Reference — spex-orchestrate

Canonical patterns for breaking a slice spec into tasks, grouping them into waves, and routing each task to the correct agent. Includes worked examples for common slice types.

---

## Decomposition Principles

1. **One task = one agent = one artifact.** A task produces exactly one registered artifact and is owned by exactly one agent skill.
2. **Tasks within a wave are independent.** They can be delegated concurrently. If task B requires task A's output, they belong in different waves.
3. **Always assign a QA task in the final wave.** Every slice closes with at least one `spex-qa` verification task.
4. **Always close with a `spex-gitops` task.** The last wave (or a dedicated close-out wave) includes a CHANGELOG entry task delegated to `spex-gitops`.
5. **Minimum viable slice = 3 waves:** schema/foundation → implementation → QA + gitops.

---

## Agent Routing Quick Reference

| Task category | Agent |
|--------------|-------|
| Database schema design, migration files | `spex-db` |
| REST API endpoints, business logic, Symfony controllers/services | `spex-backend` |
| React / Vue / Twig frontend components, pages | `spex-frontend` |
| Native Android / iOS screens, Swift/Kotlin modules, KMP/CMP shared code | `spex-mobile` |
| Dockerfile, CI/CD pipeline, Kubernetes manifests, infra config | `spex-devops` |
| LLM integration, RAG pipeline, embeddings, evals | `spex-ai-eng` |
| Commit messages, branch creation, PR creation, CHANGELOG | `spex-gitops` |
| Acceptance criteria verification, regression testing | `spex-qa` |

---

## Worked Example 1 — CRUD Feature (Symfony + MariaDB)

**Slice:** SLICE-031 — Product catalog: CRUD for admin users

### Wave 1 (foundation)
| Task ID | Title | Agent | Input | Output |
|---------|-------|-------|-------|--------|
| T031-1 | Design `products` table schema | `spex-db` | slice spec | `A031-1` migration SQL |
| T031-2 | Set up CI pipeline for this slice | `spex-devops` | slice spec | `A031-2` CI YAML |

_Wave 1 gate: `make check` — migrations run, CI pipeline lint passes_

### Wave 2 (implementation)
| Task ID | Title | Agent | Input | Output |
|---------|-------|-------|-------|--------|
| T031-3 | Implement Product entity + repository | `spex-backend` | `A031-1` | `A031-3` PHP entity + repo |
| T031-4 | Implement CRUD API endpoints | `spex-backend` | `A031-3` | `A031-4` controllers + tests |
| T031-5 | Build admin UI for product list + forms | `spex-frontend` | `A031-4` API spec | `A031-5` React components |

_Wave 2 gate: `make check` — unit + integration tests pass, API contract matches spec_

### Wave 3 (QA + close-out)
| Task ID | Title | Agent | Input | Output |
|---------|-------|-------|-------|--------|
| T031-6 | Verify all acceptance criteria | `spex-qa` | slice spec + `A031-3,4,5` | `A031-6` QA report |
| T031-7 | CHANGELOG entry for SLICE-031 | `spex-gitops` | slice spec, QA report | CHANGELOG updated |

_Wave 3 gate: `make check` — all ACs verified, CHANGELOG entry present_

---

## Worked Example 2 — AI Feature (RAG search)

**Slice:** SLICE-042 — Semantic product search using vector similarity

### Wave 1 (foundation)
| Task ID | Title | Agent | Input | Output |
|---------|-------|-------|-------|--------|
| T042-1 | Add pgvector extension, `product_embeddings` table | `spex-db` | slice spec | `A042-1` migration |
| T042-2 | Define eval dataset (≥ 20 Q/A pairs) | `spex-ai-eng` | slice spec | `A042-2` `evals/semantic-search/dataset.jsonl` |

_Wave 1 gate: `make check` — migration applies cleanly, eval dataset file present_

### Wave 2 (implementation)
| Task ID | Title | Agent | Input | Output |
|---------|-------|-------|-------|--------|
| T042-3 | Ingestion pipeline: embed + store product docs | `spex-ai-eng` | `A042-1` | `A042-3` DocumentIngester |
| T042-4 | Retrieval API: `/api/search?q=` with vector similarity | `spex-backend` | `A042-1`, `A042-3` | `A042-4` SearchController |
| T042-5 | Search UI component with result cards | `spex-frontend` | `A042-4` API spec | `A042-5` SearchBar + Results |

_Wave 2 gate: `make check` — eval pass rate ≥ 80%, API returns valid results_

### Wave 3 (QA + close-out)
| Task ID | Title | Agent | Input | Output |
|---------|-------|-------|-------|--------|
| T042-6 | Run eval suite and verify all ACs | `spex-qa` | slice spec + `A042-2,3,4` | `A042-6` QA report |
| T042-7 | CHANGELOG entry for SLICE-042 | `spex-gitops` | slice spec, QA report | CHANGELOG updated |

---

## Worked Example 3 — Mobile Feature

**Slice:** SLICE-055 — Push notifications for order status on iOS and Android

### Wave 1 (foundation)
| Task ID | Title | Agent | Input | Output |
|---------|-------|-------|-------|--------|
| T055-1 | Add `device_tokens` table, notification log | `spex-db` | slice spec | `A055-1` migration |
| T055-2 | Configure APNs + FCM credentials, CI secrets | `spex-devops` | slice spec | `A055-2` infra config |

_Wave 1 gate: `make check` — migration applies, CI secrets present_

### Wave 2 (implementation)
| Task ID | Title | Agent | Input | Output |
|---------|-------|-------|-------|--------|
| T055-3 | Device token registration API endpoint | `spex-backend` | `A055-1` | `A055-3` registration endpoint |
| T055-4 | Notification dispatch service (APNs + FCM) | `spex-backend` | `A055-2`, `A055-3` | `A055-4` NotificationService |
| T055-5 | Mobile: register device token on app launch | `spex-mobile` | `A055-3` API spec | `A055-5` RN module |
| T055-6 | Mobile: handle foreground + background push | `spex-mobile` | `A055-4` payload spec | `A055-6` push handler |

_Wave 2 gate: `make check` — unit tests pass, manual smoke test on simulator_

### Wave 3 (QA + close-out)
| Task ID | Title | Agent | Input | Output |
|---------|-------|-------|-------|--------|
| T055-7 | Verify ACs: notifications received on both platforms | `spex-qa` | slice spec + artifacts | `A055-7` QA report |
| T055-8 | CHANGELOG entry for SLICE-055 | `spex-gitops` | slice spec, QA report | CHANGELOG updated |

---

## Dependency Graph Rules

| Rule | Detail |
|------|--------|
| Schema before code | `spex-db` migration tasks always in Wave 1 |
| API before UI | Backend API tasks before frontend/mobile tasks (different waves) |
| Eval dataset before implementation | `spex-ai-eng` eval dataset always Wave 1 for AI features |
| QA always last | `spex-qa` verification always in the final wave |
| CHANGELOG always last | `spex-gitops` CHANGELOG task always in the final wave |
| Circular dependency | Not allowed — if detected, redesign the task split or the slice scope |

---

## Task Naming Convention

```
[Action verb] [subject] [for/via/using context]
```

Examples:
- `Design products table schema` — not "Database"
- `Implement Product Doctrine entity and repository` — not "Backend task 1"
- `Build product list admin UI with search and pagination` — not "Frontend"
- `Verify acceptance criteria AC1–AC4` — not "QA"
- `Add CHANGELOG entry for SLICE-031` — not "Gitops task"

---

## Minimal Slice Plan Template

```
SLICE-NNN: <title>

Wave 1 — Foundation
  T0NN-1: <schema/infra task>          → spex-db / spex-devops
  T0NN-2: <second foundation task>      → spex-db / spex-devops

Wave 2 — Implementation
  T0NN-3: <backend task>                → spex-backend
  T0NN-4: <frontend/mobile task>        → spex-frontend / spex-mobile

Wave 3 — QA + Close-out
  T0NN-5: Verify all acceptance criteria → spex-qa
  T0NN-6: Add CHANGELOG entry           → spex-gitops
```
