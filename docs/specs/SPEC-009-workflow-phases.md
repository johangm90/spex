# SPEC-009 — Fases explícitas del workflow y conexión readiness/policy

**Status**: draft
**Priority**: P0
**Depends on**: SPEC-008 (clasificación de tareas del orquestador)
**Owner**: spex-architect

---

## Overview

`spex` tiene tres subsistemas de *governance* (readiness, policy/evidence, evals) implementados en Rust y expuestos vía MCP (`state_readiness_*`, `policy_*`, `state_eval_*`), pero **ningún agente bundled los invoca**. El workflow de agentes salta de `@task-planner` directo a `@sdd-builder` y de ahí a `@reviewer`, sin registrar evidencia, sin sembrar requisitos de revisión y sin transiciones de fase.

Este spec define un **workflow de fases explícito** alineado con el modelo Brief/Constitution → Clarify → Specify → Plan → Tasks → Analyze → Implement → Verify, y conecta la capa de agentes con la maquinaria de readiness/policy que ya existe.

El trabajo se entrega en fases incrementales. Este spec cubre las **fases 1–5**; cada una es independientemente verificable.

---

## Fases de entrega

| Fase | Alcance | Toca Rust |
|------|---------|-----------|
| **1** | Conectar readiness/policy al workflow de agentes | No — solo agentes bundled + docs |
| **2** | Clarify con *decision ledger* (resueltas vs. requieren aprobación humana) | No — skill `grilling` + `@spec-writer` |
| **3** | Fase Analyze: `spex analyze <SPEC>` + `@spec-analyzer` | Sí — comando CLI nuevo, sin schema |
| **4** | Ticketing pluggable (spex-state / GitHub Issues / `.md`) | Sí — trait `TicketSink`, sin schema |
| **5** | Router de complejidad + tiers de modelo + fixes OpenCode | Sí — MCP `state_workflow_classify`, sin schema |

Constitution vive como artefacto en el repo (`docs/constitution.md`), nunca en SQLite.

---

## Acceptance Criteria

### Fase 1 — readiness/policy conectados

1. **AC-1** — `@task-planner`, tras crear tasks para un spec aprobado, transiciona el spec a fase `planning` (`state_readiness_phase_transition`) y siembra los requisitos de revisión por defecto (`test_pass`, `lint_pass`, `review_approved`) vía `state_readiness_add_requirement`, más cualquier requisito `custom` derivado de ACs no cubiertos por tests unitarios.
2. **AC-2** — `@spex-architect`, al arrancar la implementación de un spec, transiciona el spec a fase `in_progress`.
3. **AC-3** — `@sdd-builder`, al cerrar una task, registra evidencia con `policy_evidence_add` (`spec`, `task`, `summary` = comando de validación corrido, `passed` según resultado) además del evento `TaskCompleted`.
4. **AC-4** — Existe un agente `@verifier` (subagent, `edit: deny`, `bash: allow`) que: entra a review (`state_readiness_enter_review`), corre `validation_commands.full`, satisface `test_pass` y `lint_pass` (`state_readiness_satisfy_requirement`) sólo si pasan, registra evidencia por cada uno, y emite un veredicto `PASS`/`FAIL` con la lista de ACs verificados.
5. **AC-5** — `@verifier` **nunca** satisface `review_approved` ni marca el spec `done`. Esa transición la dispara `@spex-architect` con `state_readiness_approve` sólo tras aprobación humana explícita.
6. **AC-6** — `@spex-architect` incluye `state_readiness_operator` en su carga de arranque de sesión y `@spex-daily` lo incluye en su brief; ambos surfacean specs bloqueados y requisitos insatisfechos.
7. **AC-7** — El routing del orquestador enruta `verify | qa | verificar` a `@verifier`, y el flujo COMPLEX SDD pasa por `@verifier` antes de `state_readiness_approve`.
8. **AC-8** — `spex doctor` no reporta drift de conteo de agentes tras añadir `@verifier` (README, PRD y ADR-001 actualizados a 14).

### Fase 2 — Clarify / decision ledger

9. **AC-9** — El skill `grilling` produce `grilling_decisions` con la forma `{task_summary, resolved:[{branch,choice,summary,by}], needs_human_approval:[{branch,question,options,recommendation}]}` donde `by ∈ {recommendation, human}`.
10. **AC-10** — `@spec-writer` escribe ambas listas en el spec como sección `## Clarifications` fechada, y añade un `[ ]` en `Open Questions` por cada entrada de `needs_human_approval`.
11. **AC-11** — `@spex-architect` no avanza a `@task-planner` mientras `needs_human_approval` no esté vacío; presenta esas decisiones al humano una a una.

### Fase 3 — Analyze

12. **AC-12** — `spex analyze <SPEC_ID>` produce: matriz AC↔task, ACs sin task, tasks sin AC, referencias a principios de `docs/constitution.md` inexistentes, y términos marcadores de ambigüedad (`TBD`, `TODO`, `???`). Exit code `1` si hay hallazgos de severidad alta.
13. **AC-13** — `@spec-analyzer` (subagent, `edit: deny`, `bash: allow`) envuelve `spex analyze` y añade juicio cualitativo; se invoca obligatoriamente en el flujo COMPLEX antes de implementar.

### Fase 4 — Ticketing pluggable

14. **AC-14** — `enum TicketBackend { SpexState, GitHub, Markdown }` con un trait `TicketSink` (`create_ticket`, `update_status`, `link`). `SpexState` siempre activo; `GitHub` y `Markdown` opt-in por `[tickets]` en `.spex/config.toml`.
15. **AC-15** — `spex task export --to github|md` proyecta las tasks de un spec al backend elegido; GitHub usa `gh issue create` y guarda el número de issue en `tasks.output_artifact` o un mapping. `Markdown` escribe `.spex/tasks/TASK-NNN.md` con frontmatter YAML.
16. **AC-16** — El backend no disponible (sin `gh`, sin remote) degrada con aviso claro, nunca aborta el plan.

### Fase 5 — Router de complejidad y modelos

17. **AC-17** — MCP tool `state_workflow_classify({description, files_touched?, crosses_subsystems?, public_contract?})` → `{tier: trivial|standard|complex, score, rationale}` usando la heurística de SPEC-008. Sin llamadas a LLM.
18. **AC-18** — Los agentes bundled declaran `model: "{env:SPEX_MODEL_FAST|BUILD|REASONING}"` según rol; `spex setup` pregunta los tres valores (o escribe defaults comentados si el host no soporta interpolación).
19. **AC-19** — Existe `@sdd-builder-deep` idéntico a `@sdd-builder` salvo tier `REASONING`; `@spex-architect` lo elige cuando `state_workflow_classify` devuelve `complex`.
20. **AC-20** — `spex setup` instala `commands/*.md` (`spex-constitution|clarify|specify|plan|tasks|analyze|implement|verify`) al directorio de comandos del host cuando el host los soporta (OpenCode, Copilot).
21. **AC-21** — `host::HostProfile` para OpenCode apunta el MCP global al archivo de config correcto (verificar `~/.config/opencode/opencode.json` vs `config.json`); test de regresión añadido.

---

## Mapa workflow → spex

| Fase del modelo | Mecanismo en spex |
|---|---|
| Brief / Constitution | `docs/PRD.md` + `docs/constitution.md` (artefacto). `spex brief` / `@spex-daily` |
| Clarify | skill `grilling` → `grilling_decisions` con decision ledger (Fase 2) |
| Specify | `@spec-writer` → `state_slice_create` + `## Clarifications` en el spec. Gate: `spex spec approve` |
| Plan | `@adr-writer` para decisiones; notas técnicas en el spec |
| Tasks | `@task-planner` → `state_task_create` + backend de tickets pluggable (Fase 4) |
| Analyze | `spex analyze` + `@spec-analyzer` (Fase 3) |
| Implement | `@spex-architect` → `@sdd-builder`/`@sdd-builder-deep` por deps → `@reviewer` |
| Verify / QA | `@verifier` → readiness requirements + evidencia → `state_readiness_approve` tras OK humano |

---

## Out of scope

- Cambios al schema SQLite (todas las tablas de readiness/policy/evals ya existen).
- Constitution en base de datos.
- Persistencia del estado del router entre sesiones.
- Sincronización bidireccional continua con GitHub Issues (Fase 4 es export one-way + status update).
- Selección de modelo vía llamada a un LLM clasificador (sólo heurística en esta iteración).

---

## Riesgos

| Riesgo | Mitigación |
|--------|-----------|
| `enter_review` siembra requisitos duplicados si `@task-planner` ya los creó | `enter_review` sólo siembra si la lista está vacía (ya implementado en `sdd/readiness.rs`) |
| `@verifier` marca `review_approved` por error y cierra el spec sin humano | AC-5 lo prohíbe explícitamente; `state_readiness_approve` sólo lo llama el orquestador |
| Interpolación `{env:...}` no soportada por todos los hosts | `spex setup` escribe defaults comentados y documenta el override manual |
| Drift de conteo en docs al añadir agentes | AC-8; actualizar README/PRD/ADR-001 en la misma entrega |

---

https://claude.ai/code/session_01HY9dq7EQjNd3qJTc1LYESq
