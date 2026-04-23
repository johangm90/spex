# SPEC-005: Sistema de evals y scorecards para agentes

**Status:** Draft  
**Priority:** P1  
**Dependencies:** SPEC-002, SPEC-003, SPEC-004  
**Created:** 2026-04-22  

---

## Overview

This spec adds a local-first evaluation system to spex so teams can measure agent quality explicitly instead of inferring it from task completion alone. The goal is to persist structured eval runs, generate reusable scorecards, and compare outcomes over time using the same SQLite-backed control plane that already stores specs, tasks, evidence, approvals, sessions, and events.

With SPEC-002 and SPEC-003 in place, spex can now enforce workflow invariants and evidence gates. With SPEC-004, it can also attribute work, track sessions, and emit enterprise audit signals. What is still missing is a first-class answer to: **how good was this agent result, why, and is it improving or regressing over time?**

This spec introduces that answer.

---

## Problem Statement

Today, spex can tell whether a task or spec reached `done`, whether evidence was attached, and whether policy gates were satisfied. It cannot yet capture a normalized evaluation of the quality of the work itself. Teams need a durable mechanism to record judgments such as:

- whether the implementation appears correct
- whether validation coverage was sufficient
- whether the work complied with policy and review expectations
- whether the change increased operational or architectural risk
- whether a newer run is better or worse than a prior baseline

Without a first-class eval model, these judgments remain buried in prose, external tools, or human memory. That weakens auditability, makes agent comparison ad hoc, and limits enterprise adoption where reproducible quality scoring matters.

---

## Goals

- Persist **evaluation runs** as durable records in local `state.db`
- Support **scorecards** with structured dimension-level results instead of only freeform notes
- Allow comparison of a current eval against a baseline to identify **improvements, regressions, or no-change**
- Expose evals through both **CLI** and **MCP** for humans and agents
- Keep the design **local-first, append-only, auditable, and policy-aware**

---

## Non-Goals

- No hosted eval service or remote dashboard
- No requirement for one universal judging method across every repository
- No automatic hard-blocking of workflow transitions solely because of a poor scorecard unless future policy rules explicitly opt into that
- No retroactive rescoring of historical work unless a new eval run is explicitly created
- No cross-project analytics layer in v1

---

## Proposed Capabilities

### 1. Eval run records

Add a first-class eval entity that records:

- eval ID
- evaluator identity
- target scope (`spec`, `task`, `artifact`, or comparable subject reference)
- timestamp
- summary / rationale
- optional links to evidence bundles, artifacts, events, sessions, or validations
- overall outcome

These records should be append-only from an audit perspective.

### 2. Structured scorecards

Each eval run can produce a scorecard containing normalized dimensions such as:

- correctness
- validation coverage
- policy compliance
- risk / blast radius

Each dimension should support structured values that can be compared across runs without relying on prose parsing.

### 3. Baseline and regression comparison

Allow a caller to compare one eval run against another baseline and compute:

- per-dimension deltas
- overall classification: improved / regressed / unchanged
- explicit references to the compared eval IDs

### 4. CLI and MCP inspection surfaces

Expose commands/tools to:

- create eval runs
- list eval runs by spec/task/artifact
- fetch a single eval with scorecard detail
- compare evals or fetch the latest-vs-baseline view

### 5. Audit integration

Eval creation and major comparison operations should emit domain events so that the audit log reflects not only execution and evidence, but also evaluation activity.

---

## Acceptance Criteria

### AC-1: Eval records are first-class and traceable
**Given** an approved or in-progress spec with at least one task or registered artifact  
**When** an eval result is recorded for agent work  
**Then** spex persists an evaluation record in local `state.db` with a stable identifier, timestamp, evaluator identity, scope reference, and outcome  
**And** the record can reference at least one of: spec ID, task ID, artifact ID, or event/session context without duplicating the underlying source objects

### AC-2: Scorecards expose measurable quality dimensions
**Given** an evaluation record exists for a task, spec, or artifact  
**When** a scorecard is generated or retrieved  
**Then** it includes explicit dimension results for correctness, validation coverage, policy compliance, and risk/blast-radius  
**And** each dimension has a normalized status or score that can be compared across runs without relying on freeform prose alone

### AC-3: Baselines and regressions are comparable over time
**Given** two or more eval records exist for the same logical scope or comparison group  
**When** a user requests a comparison  
**Then** spex returns the baseline and current results with per-dimension deltas and an overall regression / neutral / improvement classification  
**And** the comparison output identifies exactly which eval runs were compared

### AC-4: Eval results are inspectable from CLI and MCP
**Given** eval records exist in `state.db`  
**When** a human uses the CLI or an agent uses MCP to inspect evals  
**Then** they can list and fetch eval results filtered by spec, task, artifact, status, and time range  
**And** the interface returns structured data sufficient to identify the latest scorecard and a selected baseline without direct database access

### AC-5: The design remains local-first and backward-compatible
**Given** a repository already using spex with the current project-local SQLite architecture  
**When** this feature is introduced  
**Then** all eval data is stored locally in `state.db` and remains usable with no network dependency  
**And** existing specs, tasks, artifacts, events, evidence, policy, and memory behavior continue to function when no eval records are present

### AC-6: Invalid or orphaned evals are rejected safely
**Given** a caller attempts to create or compare an eval record with a missing scope reference, unsupported score dimension, or nonexistent baseline target  
**When** spex validates the request  
**Then** it rejects the operation with a specific error that identifies the invalid field or missing dependency  
**And** no partial eval or comparison record is persisted

---

## Dependencies

| Spec/System | Type | Notes |
|---|---|---|
| SPEC-002 | blocks-this | Required for transactional workflow hardening and audit-safe persistence |
| SPEC-003 | blocks-this | Required for evidence/policy concepts that evals should integrate with |
| SPEC-004 | strongly-informs | Sessions, attribution, trace, and webhook audit surfaces are useful context for eval provenance |
| `.spex/state.db` | integration | Eval storage must fit the existing local-first SQLite model |
| CLI + MCP surfaces | integration | Eval inspection and creation must be available to both humans and agents |

---

## Risks / Open Questions

1. **Score normalization** — Should dimensions be ordinal (`pass/warn/fail`) or numeric in v1?
2. **Overall score weighting** — Fixed global weighting vs configurable per project?
3. **Baseline selection** — Explicit eval IDs only, or convenience modes like “latest green” or “latest approved”?
4. **Governance interaction** — Should future policy configs be able to require a minimum scorecard outcome before completion?
5. **Storage growth** — Long-lived repos may accumulate large numbers of evals and comparisons; retention/archival may be needed later.

---

## Recommended Implementation Themes

This should likely be implemented in slices like:

1. **Schema + domain model**
   - eval tables
   - scorecard dimensions
   - comparison/baseline references

2. **Recording APIs**
   - Rust domain functions
   - CLI create/list/show
   - MCP tools for create/query

3. **Comparison and scorecards**
   - normalized score computation
   - latest-vs-baseline comparison helpers

4. **Audit / integration**
   - domain events for eval creation and comparison
   - trace integration and possible webhook extension

5. **Validation and tests**
   - persistence tests
   - rollback tests
   - CLI/MCP integration tests

---

## Approval Notes

This spec is intentionally scoped to the **data model and operator surfaces** for evals and scorecards. It does **not** yet assume a fully autonomous judge or external benchmark runner. That keeps v1 aligned with spex’s local-first architecture and lets us add richer evaluators later without blocking the core audit trail.
