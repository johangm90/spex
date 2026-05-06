# SPEC-008 — Refactorización del flujo de orquestación de spex-architect

**Status**: draft  
**Priority**: P0  
**Depends on**: ninguna (cambio de system prompt puro, sin código Rust)  
**Owner**: spex-architect  

---

## Overview

El agente `spex-architect` actualmente usa un flujo monolítico: toda tarea pasa por el proceso SDD completo (spec → aprobación → task planning → sdd-builder), independientemente de si la tarea es un rename de función o una nueva feature de arquitectura. Esto genera fricción innecesaria para trabajo cotidiano y no tiene un mecanismo estructurado de HITL para trabajo complejo.

Este spec refactoriza el **comportamiento del agente** (system prompt) para introducir tres modos de operación bien definidos:

1. **Fast-track** — para tareas simples: actúa directo, sin overhead de estado
2. **Grill-me HITL** — para tareas complejas: interrogatorio estructurado antes de crear el spec
3. **Fallback multi-entorno** — auto-detect silencioso de MCP → CLI → archivos

El cambio vive **exclusivamente en el system prompt** de `spex-architect`. No requiere modificaciones al código Rust ni al schema SQLite.

---

## Acceptance Criteria

1. **AC-1** — El agente clasifica automáticamente toda tarea entrante como `simple` o `complex` usando la heurística definida en este spec, sin preguntar al usuario.
2. **AC-2** — Para tareas `simple`, el agente ejecuta el flujo fast-track (Inspect → Act → Verify → Report) sin crear spec, task ni eventos en el estado.
3. **AC-3** — Para tareas `complex`, el agente activa el modo grill-me: hace preguntas una a la vez, con opciones numeradas y recomendación explícita marcada.
4. **AC-4** — El grill-me termina cuando todas las ramas del árbol de decisiones están resueltas; el agente lo detecta y genera un spec técnico completo en estado `draft`.
5. **AC-5** — El agente detecta aprobación del spec mediante lenguaje natural del usuario (lista de palabras/frases definida) y avanza automáticamente a task planning con `@task-planner`.
6. **AC-6** — Post-aprobación, el flujo SDD siempre pasa por `@task-planner` → `@sdd-builder`, nunca implementa directo desde el spec.
7. **AC-7** — Post-implementación, el agente corre `validation_commands.primary` (`cargo test --all-targets`). Si pasa → marca spec done. Si falla → reporta y espera instrucciones.
8. **AC-8** — Al inicio de sesión, el agente detecta silenciosamente el entorno disponible: MCP tools → spex CLI → archivos `.spex/`. Sin notificación al usuario.
9. **AC-9** — En modo CLI, el agente tiene paridad completa de operaciones de memoria con modo MCP, usando `spex memory set/show/list/search` para todas las claves.
10. **AC-10** — En modo archivos, el agente lee y escribe estado en `.spex/specs/`, `.spex/tasks/`, `.spex/memory/` con frontmatter YAML estructurado.
11. **AC-11** — Si el usuario responde "tú decides" o equivalente en el grill-me, el agente aplica su recomendación automáticamente y continúa.
12. **AC-12** — El fast-track corre `validation_commands.primary` antes de reportar. No reporta sin verificar.

---

## Flujo de clasificación automática

### Señales de complejidad

| Señal | Peso | Ejemplos simples | Ejemplos complejos |
|-------|------|-----------------|-------------------|
| **Archivos afectados** | Alto | 1-3 archivos, mismo módulo | 4+ archivos, múltiples módulos |
| **Cruza subsistemas** | Alto | Solo CLI, solo domain, solo MCP | CLI + domain + MCP + tests |
| **Cambia contrato público** | Crítico | Lógica interna, refactor local | API pública, schema SQL, MCP tools |
| **Nueva feature con comportamiento visible** | Crítico | Fix de bug, rename, doc update | Nueva command, nuevo workflow, nueva integración |

### Heurística de decisión

```
classify(request):
  signals = []
  
  if affects_public_contract(request):      signals.push(COMPLEX, weight=3)
  if is_new_user_visible_feature(request):  signals.push(COMPLEX, weight=3)
  if crosses_subsystems(request):           signals.push(COMPLEX, weight=2)
  if files_affected > 3:                    signals.push(COMPLEX, weight=1)
  
  if sum(COMPLEX weights) >= 3: return COMPLEX
  else: return SIMPLE
```

### Ejemplos concretos

| Tarea | Clasificación | Razón |
|-------|--------------|-------|
| "renombra la función `get_spec` a `fetch_spec`" | SIMPLE | 1-2 archivos, sin contrato público |
| "arregla el test que falla en policy.rs" | SIMPLE | Fix local, sin nueva feature |
| "agrega un comentario a este módulo" | SIMPLE | Doc update, sin comportamiento |
| "agrega `spex eval export` command" | COMPLEX | Nueva feature visible, cruza CLI+domain+MCP |
| "refactoriza el schema de sessions" | COMPLEX | Cambia schema SQL (contrato público) |
| "implementa autenticación en el MCP server" | COMPLEX | Nueva feature, múltiples subsistemas |
| "actualiza el README" | SIMPLE | Solo docs |
| "agrega un campo opcional a un struct interno" | SIMPLE | Sin contrato público, 1-2 archivos |

---

## Flujo fast-track (tareas simples)

### Pasos exactos

1. **Inspect** — Lee el código relevante. Entiende el contexto. Si hay ambigüedad que cambiaría el comportamiento, pregunta UNA sola vez.
2. **Act** — Implementa el cambio mínimo correcto. Edita archivos directamente.
3. **Verify** — Corre `validation_commands.primary`. Si falla, corrige y re-verifica antes de reportar.
4. **Report** — Informa qué cambió, qué archivos, qué validación corrió y pasó. Menciona riesgos residuales si los hay.

### Lo que NO hace en fast-track

- ❌ No crea spec en el estado
- ❌ No crea tasks
- ❌ No emite eventos
- ❌ No invoca `@task-planner` ni `@sdd-builder`
- ❌ No pregunta si la tarea es simple o compleja
- ❌ No reporta sin haber verificado primero

---

## Flujo grill-me HITL (tareas complejas)

### Pasos exactos

1. **Anunciar modo** — El agente dice explícitamente: *"Esta tarea es compleja. Voy a hacerte algunas preguntas antes de crear el spec."*
2. **Mapear el árbol de decisiones** — El agente identifica internamente todas las ramas de decisión relevantes para la tarea (arquitectura, scope, riesgos, integraciones, etc.).
3. **Preguntar una a la vez** — Para cada rama, formula una pregunta con:
   - Contexto breve (1-2 líneas)
   - Opciones numeradas (A, B, C, D...)
   - Recomendación del agente marcada con `*(Recomendado)*`
4. **Procesar respuesta** — El usuario elige una opción (letra, número, o "tú decides"). Si dice "tú decides", el agente aplica su recomendación y continúa.
5. **Detectar completitud** — Cuando todas las ramas están resueltas, el agente dice: *"Todas las decisiones están resueltas. Generando el spec..."*
6. **Generar spec** — Crea el spec técnico completo con todo el contexto de las respuestas. Lo registra en el estado como `draft` via el backend disponible (MCP/CLI/archivos).

### Formato de pregunta

```
**Pregunta N de ~M — [Tema]**

[Contexto breve de por qué esta decisión importa]

Opciones:

- **A) [Opción A]** — [descripción]. *(Recomendado)*
- **B) [Opción B]** — [descripción].
- **C) [Opción C]** — [descripción].

¿Cuál prefieres?
```

### Criterio de completitud

El grill-me termina cuando el agente ha resuelto:
- Arquitectura / approach técnico
- Scope (qué entra, qué no entra)
- Integraciones afectadas (CLI / MCP / domain / schema)
- Estrategia de validación
- Riesgos principales y mitigaciones

Si una rama no aplica a la tarea, el agente la omite silenciosamente.

---

## Flujo de aprobación y ejecución SDD

### Detección de aprobación

El agente detecta aprobación cuando el usuario dice cualquiera de:

> aprobado, approved, sí, si, yes, go, adelante, lgtm, ok, okay, perfecto, dale, hazlo, procede, proceed, ship it, merge it, build it, let's go, va, vamos

La detección es case-insensitive. Si el mensaje contiene alguna de estas palabras como respuesta al spec presentado, se considera aprobación.

Si el usuario hace preguntas o pide cambios al spec, el agente los incorpora y re-presenta el spec antes de esperar nueva aprobación.

### Pasos post-aprobación

1. **Actualizar estado** — Marca el spec como `approved` en el backend disponible.
2. **Task planning** — Invoca `@task-planner` con el spec completo + contexto del proyecto.
3. **Implementación** — Invoca `@sdd-builder` para cada task, en orden de dependencias.
4. **Validación** — Corre `validation_commands.primary`. Si pasa → marca spec `done`. Si falla → reporta error específico y espera instrucciones del usuario.

---

## Detección de entorno (auto-detect silencioso)

### Tabla de decisión

| Condición | Backend seleccionado |
|-----------|---------------------|
| MCP tools responden sin error | `mcp` |
| MCP falla / no disponible + `spex` CLI existe en PATH | `cli` |
| MCP falla + CLI no disponible + `.spex/` existe o puede crearse | `files` |

### Pseudocódigo

```
detect_backend():
  try:
    result = state_snapshot()  # MCP tool
    if result.ok: return "mcp"
  catch:
    pass
  
  try:
    result = bash("spex --version")
    if result.ok: return "cli"
  catch:
    pass
  
  return "files"  # fallback siempre disponible
```

### Operaciones por modo

| Operación | Modo MCP | Modo CLI | Modo archivos |
|-----------|----------|----------|---------------|
| Leer estado | `state_snapshot()` | `spex brief --json` | Leer `.spex/specs/*.md` |
| Crear spec | `state_slice_create()` | `spex spec create` | Escribir `.spex/specs/SPEC-NNN.md` |
| Actualizar spec | `state_slice_update()` | `spex spec update` | Editar frontmatter del archivo |
| Crear task | `state_task_create()` | `spex task create` | Escribir `.spex/tasks/TASK-NNN.md` |
| Guardar memoria | `memory_set()` | `spex memory set` | Escribir `.spex/memory/<key>.md` |
| Leer memoria | `memory_get()` | `spex memory show` | Leer `.spex/memory/<key>.md` |
| Buscar memoria | `memory_search()` | `spex memory search` | Grep en `.spex/memory/` |
| Emitir evento | `state_event_emit()` | `spex event emit` | Append a `.spex/events.md` |

---

## Modo CLI — tabla de equivalencias completa

| MCP Tool | CLI Command | Notas |
|----------|-------------|-------|
| `state_snapshot` | `spex brief --json` | Estado completo del proyecto |
| `state_slice_create` | `spex spec create --id X --title "..."` | Crea spec en draft |
| `state_slice_update` | `spex spec update --id X --status Y` | Actualiza status/ACs |
| `state_slice_get` | `spex spec show X --json` | Lee spec por ID |
| `state_task_create` | `spex task create --id X --spec Y --title "..."` | Crea task |
| `state_task_update` | `spex task update --id X --status Y` | Actualiza task |
| `state_task_get` | `spex task show X --json` | Lee task por ID |
| `state_event_emit` | `spex event emit --type X --spec Y` | Emite evento |
| `state_event_query` | `spex event list --spec X --json` | Lista eventos |
| `memory_set` | `spex memory set --agent A --key K --value 'JSON' --type T` | Guarda memoria |
| `memory_get` | `spex memory show A K --json` | Lee clave de memoria |
| `memory_list` | `spex memory list --agent A --json` | Lista toda la memoria del agente |
| `memory_search` | `spex memory search "query" --agent A --json` | Búsqueda FTS |
| `memory_delete` | `spex memory delete --agent A --key K` | Elimina clave |
| `state_artifact_register` | `spex artifact register --id X --agent A --type T --path P` | Registra artefacto |
| `state_artifact_query` | `spex artifact list --spec X --json` | Lista artefactos |
| `state_session_start` | `spex session start --agent A` | Inicia sesión |
| `state_session_end` | `spex session end --session-id X` | Termina sesión |
| `state_sessions_list` | `spex session list --json` | Lista sesiones |
| `policy_evidence_add` | `spex policy evidence add --task X --kind test_run` | Agrega evidencia |
| `policy_approval_request` | `spex policy approval request --task X --operation Y` | Solicita aprobación |

---

## Modo archivos — estructura `.spex/`

### Estructura de directorios

```
.spex/
├── config.toml          # configuración del proyecto (ya existe)
├── events.md            # log append-only de eventos
├── specs/
│   ├── SPEC-001.md
│   ├── SPEC-002.md
│   └── ...
├── tasks/
│   ├── TASK-001.md
│   └── ...
└── memory/
    ├── spex-architect/
    │   ├── session_context.md
    │   ├── active_project.md
    │   ├── repo_map.md
    │   └── ...
    └── sdd-builder/
        └── ...
```

### Formato de spec (frontmatter YAML)

```markdown
---
id: SPEC-NNN
title: "Título del spec"
status: draft | approved | in_progress | done
priority: P0 | P1 | P2 | P3
ac_total: 0
ac_passed: 0
agents: ["spex-architect"]
depends_on: []
created_at: 2026-05-06T10:00:00Z
updated_at: 2026-05-06T10:00:00Z
---

# SPEC-NNN — Título

## Overview
...

## Acceptance Criteria
1. AC-1 — ...
```

### Formato de task

```markdown
---
id: TASK-NNN
spec: SPEC-NNN
title: "Título de la task"
status: pending | in_progress | done
agent: sdd-builder
inputs: []
output_artifact: "src/foo.rs"
created_at: 2026-05-06T10:00:00Z
---

## Descripción
...
```

### Formato de memoria

```markdown
---
agent: spex-architect
key: session_context
type: config
updated_at: 2026-05-06T10:00:00Z
---

{"date":"2026-05-06","next_action":"...","session_summary":"..."}
```

### Log de eventos (`.spex/events.md`)

```markdown
## 2026-05-06T10:05:00Z | SpecCreated | spex-architect | SPEC-008
payload: {}

## 2026-05-06T10:10:00Z | SpecApproved | human | SPEC-008
payload: {}
```

---

## Cambios al system prompt de spex-architect

### Sección a REEMPLAZAR: "Core operating model"

Reemplazar la tabla de intents actual con la siguiente lógica de clasificación explícita:

```
## Core operating model

### Paso 1: Clasificar la tarea

Antes de actuar, clasifica la tarea como SIMPLE o COMPLEX:

**SIMPLE** si todas estas condiciones se cumplen:
- Afecta ≤3 archivos en el mismo módulo/subsistema
- No cambia contratos públicos (API, schema SQL, MCP tools, CLI commands)
- No es una nueva feature con comportamiento visible para el usuario
- No cruza múltiples subsistemas (CLI + domain + MCP + tests)

**COMPLEX** si cualquiera de estas condiciones se cumple:
- Nueva feature con comportamiento visible para el usuario
- Cambia contrato público (API, schema, MCP tool, CLI command)
- Cruza múltiples subsistemas
- Requiere decisiones arquitectónicas no triviales

### Paso 2: Ejecutar el flujo correspondiente

**Si SIMPLE → Fast-track:**
Inspect → Act → Verify (primary) → Report. Sin spec, sin tasks, sin eventos.

**Si COMPLEX → Grill-me HITL:**
Anuncia modo complejo → pregunta una a la vez con opciones + recomendación → genera spec draft → espera aprobación → task planning → sdd-builder → validación primary → spec done.
```

### Sección a REEMPLAZAR: "MCP-unavailable fallback"

Reemplazar con la lógica de auto-detect de tres niveles descrita en este spec (MCP → CLI → archivos), incluyendo la tabla de equivalencias completa y la estructura `.spex/`.

### Sección NUEVA: "Grill-me HITL protocol"

Agregar sección con el formato exacto de preguntas, criterio de completitud, y detección de aprobación (lista de palabras).

---

## Dependencias

Ninguna. Este spec es un cambio puro de system prompt. No requiere:
- Modificaciones al código Rust
- Nuevas migraciones SQL
- Nuevos MCP tools
- Cambios al CLI

---

## Riesgos

| Riesgo | Probabilidad | Impacto | Mitigación |
|--------|-------------|---------|------------|
| Clasificación incorrecta (simple → complex o viceversa) | Media | Medio | La heurística es conservadora: en caso de duda, clasifica como COMPLEX |
| Grill-me demasiado largo para tareas medianas | Media | Bajo | El agente omite ramas que no aplican; el usuario puede decir "suficiente, genera el spec" |
| Auto-detect falla silenciosamente en entorno raro | Baja | Alto | Fallback a archivos siempre disponible; el agente puede crear `.spex/` si no existe |
| Detección de aprobación con falsos positivos | Baja | Medio | La lista de palabras se evalúa en contexto (respuesta al spec presentado, no en cualquier mensaje) |
| Paridad CLI incompleta (comandos que no existen aún) | Media | Medio | El agente documenta qué comandos usa; si un comando no existe, cae a modo archivos para esa operación |
| Regresión en comportamiento existente para tareas simples | Baja | Alto | El fast-track es más permisivo que el flujo actual, no más restrictivo |

---

## Out of scope

- Cambios al código Rust de spex
- Nuevos comandos CLI
- Nuevas migraciones SQL
- Cambios a otros agentes (sdd-builder, task-planner, etc.)
- UI o visualización del grill-me
- Persistencia del estado del grill-me entre sesiones (si se interrumpe, reinicia)
- Integración con GitHub Issues o PRs
