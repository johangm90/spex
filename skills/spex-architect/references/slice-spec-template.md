# Slice Spec Template

Use this structure for every vertical slice spec. Store the completed spec in MCP via `memory_set(agent="spex-architect", key="slice_SLICE-NNN")`.

---

## SLICE-NNN: \<Title\>

**Status:** draft | approved | in-progress | done
**Priority:** P0 | P1 | P2 | P3
**Depends on:** SLICE-NNN, ... (or "none")

---

### Purpose & Scope

**In scope:**
- \<bullet: what this slice delivers\>

**Out of scope:**
- \<bullet: what is explicitly deferred or excluded\>

---

### Domain Context

**Primary bounded context:** \<context name\>
**Secondary bounded contexts touched:** \<context name(s) or "none"\>

\<1–2 sentences describing how this slice fits within the domain model.\>

---

### User Story / Scenario

```
As a <role>,
I want <capability>,
so that <outcome>.
```

_Optional: include a numbered scenario walkthrough for complex flows._

---

### API Surface (draft)

\<List proposed endpoints, CLI commands, or event contracts. Mark as DRAFT until the implementing agent finalises them.\>

```
POST /api/v1/<resource>
  Request:  { field: type, ... }
  Response: { field: type, ... }
  Errors:   4xx / 5xx cases
```

---

### Domain Events

| Event | Direction | Description |
|-------|-----------|-------------|
| `<EventName>` | produced | \<What triggers this event and what it carries\> |
| `<EventName>` | consumed | \<What this slice reacts to and how\> |

---

### Data Requirements

- **New entities / tables:** \<name and key fields, or "none"\>
- **Existing entities modified:** \<name and changes, or "none"\>
- **Migrations required:** yes / no
- **Sensitive data:** \<PII, credentials, etc. — or "none"\>

---

### Dependent Artifacts

| Artifact ID | Type | Provided by |
|-------------|------|-------------|
| \<A0NN-N\> | \<type\> | \<agent\> |

---

### Sub-tasks

| Task ID | Title | Agent |
|---------|-------|-------|
| T0NN-1 | \<task title\> | \<spex-agent\> |
| T0NN-2 | \<task title\> | \<spex-agent\> |

---

### Acceptance Criteria

Each criterion must be **independently verifiable** — avoid compound criteria.

- [ ] AC1: \<specific, observable outcome\>
- [ ] AC2: \<specific, observable outcome\>
- [ ] AC3: \<specific, observable outcome\>

---

### Open Questions / Risks

- \<question or risk — or "none"\>
