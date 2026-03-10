# ADR Template

Architecture Decision Records are stored as `docs/adr/ADR-NNNN.md` and committed with:
```
git commit -m "docs(adr): add ADR-NNNN — <decision title>"
```

Every ADR must include **at least 2 alternatives** and explicit consequences.

---

## ADR-NNNN: \<Decision Title\>

**Date:** YYYY-MM-DD
**Status:** proposed | accepted | deprecated | superseded by ADR-NNNN
**Deciders:** \<agent(s) and/or human\>

---

### Context

\<Describe the situation, forces, and constraints that make this decision necessary. Include relevant domain events, system boundaries, or user requirements that are driving the need for a decision.\>

---

### Problem Statement

\<One clear sentence stating what must be decided.\>

---

### Alternatives Considered

#### Option A: \<Name\>

\<Description of the approach.\>

**Pros:**
- \<pro\>

**Cons:**
- \<con\>

---

#### Option B: \<Name\>

\<Description of the approach.\>

**Pros:**
- \<pro\>

**Cons:**
- \<con\>

---

#### Option C: \<Name\> _(optional)_

\<Description of the approach.\>

**Pros:**
- \<pro\>

**Cons:**
- \<con\>

---

### Decision

**Chosen option:** Option \<A|B|C\> — \<Name\>

\<1–2 sentences explaining the choice.\>

---

### Rationale

\<Explain why this option was selected over the alternatives. Reference specific constraints, team capabilities, timeline pressures, or domain requirements that made this the best fit.\>

---

### Consequences

**Positive:**
- \<outcome\>

**Negative / Trade-offs:**
- \<outcome\>

**Risks:**
- \<risk and any mitigation plan\>

---

### Related

- Supersedes: ADR-NNNN (or "none")
- Related slices: SLICE-NNN (or "none")
- Related artifacts: A0NN-N (or "none")
