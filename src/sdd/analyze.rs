//! Cross-artifact consistency analysis for a spec, run before implementation.
//!
//! Deterministic, read-only checks over the spec body (stored in architect
//! memory as `spec_<ID>`), its tasks, and — when referenced — the project
//! constitution artifact. Powers `spex analyze` and the `@spec-analyzer` agent.

use anyhow::Result;
use serde::Serialize;
use sqlx::SqlitePool;
use std::collections::BTreeSet;

use crate::sdd::{
    db::find_project_root, memory::memory_get_full, spec::get_spec, task::list_tasks,
};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum Severity {
    High,
    Medium,
    Low,
}

impl Severity {
    pub fn label(&self) -> &'static str {
        match self {
            Severity::High => "HIGH",
            Severity::Medium => "MEDIUM",
            Severity::Low => "LOW",
        }
    }
}

#[derive(Debug, Clone, Serialize)]
pub struct Finding {
    pub severity: Severity,
    pub check: &'static str,
    pub detail: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct AcCoverage {
    pub ac: String,
    pub covered_by: Vec<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct AnalysisReport {
    pub spec_id: String,
    pub spec_status: Option<String>,
    pub ac_labels: Vec<String>,
    pub ac_total_field: i64,
    pub task_count: usize,
    pub coverage: Vec<AcCoverage>,
    pub findings: Vec<Finding>,
}

impl AnalysisReport {
    /// True when at least one HIGH-severity finding exists (drives exit code 1).
    pub fn has_blocking(&self) -> bool {
        self.findings.iter().any(|f| f.severity == Severity::High)
    }

    pub fn counts(&self) -> (usize, usize, usize) {
        let mut h = 0;
        let mut m = 0;
        let mut l = 0;
        for f in &self.findings {
            match f.severity {
                Severity::High => h += 1,
                Severity::Medium => m += 1,
                Severity::Low => l += 1,
            }
        }
        (h, m, l)
    }
}

/// Run the full analysis for `spec_id`.
///
/// `known_agents` is the set of agent names a task may legitimately reference
/// (typically the bundled agent names); pass an empty slice to skip that check.
pub async fn analyze_spec(
    pool: &SqlitePool,
    spec_id: &str,
    known_agents: &[String],
) -> Result<AnalysisReport> {
    let mut findings: Vec<Finding> = Vec::new();

    let Some(spec) = get_spec(pool, spec_id).await? else {
        findings.push(Finding {
            severity: Severity::High,
            check: "spec_not_found",
            detail: format!("no spec with id {spec_id}"),
        });
        return Ok(AnalysisReport {
            spec_id: spec_id.to_string(),
            spec_status: None,
            ac_labels: Vec::new(),
            ac_total_field: 0,
            task_count: 0,
            coverage: Vec::new(),
            findings,
        });
    };

    if spec.status == "draft" {
        findings.push(Finding {
            severity: Severity::Medium,
            check: "spec_in_draft",
            detail: "spec is still in draft — approve before implementation".into(),
        });
    }

    // ── Spec body ────────────────────────────────────────────────────────────
    let body = load_spec_body(pool, spec_id).await?;
    let mut ac_labels: Vec<String> = Vec::new();

    match &body {
        None => findings.push(Finding {
            severity: Severity::High,
            check: "no_spec_body",
            detail: format!("no `spec_{spec_id}` entry in spex-architect memory (spec-writer never ran, or it was pruned)"),
        }),
        Some(text) => {
            ac_labels = parse_ac_labels(text);

            if ac_labels.is_empty() {
                findings.push(Finding {
                    severity: Severity::High,
                    check: "no_acceptance_criteria",
                    detail: "spec body has no `AC-N` labels".into(),
                });
            }

            if let Some(reason) = unresolved_marker(text) {
                findings.push(Finding {
                    severity: Severity::High,
                    check: "unresolved_decisions",
                    detail: format!("{reason} — resolve before implementation"),
                });
            }

            let markers = ambiguity_markers(text);
            if !markers.is_empty() {
                findings.push(Finding {
                    severity: Severity::Medium,
                    check: "ambiguity_markers",
                    detail: format!("found: {}", markers.join(", ")),
                });
            }

            let placeholders = template_placeholders(text);
            if !placeholders.is_empty() {
                findings.push(Finding {
                    severity: Severity::Medium,
                    check: "unfilled_template",
                    detail: format!("template placeholders left in body: {}", placeholders.join(", ")),
                });
            }

            if spec.ac_total > 0 && spec.ac_total as usize != ac_labels.len() {
                findings.push(Finding {
                    severity: Severity::Medium,
                    check: "ac_count_mismatch",
                    detail: format!(
                        "specs.ac_total = {} but body has {} AC label(s) ({})",
                        spec.ac_total,
                        ac_labels.len(),
                        ac_labels.join(", ")
                    ),
                });
            }
        }
    }

    // ── Tasks ────────────────────────────────────────────────────────────────
    let tasks = list_tasks(pool, Some(spec_id), None, None).await?;
    let known_agents: BTreeSet<&str> = known_agents.iter().map(String::as_str).collect();
    let check_agents = !known_agents.is_empty();

    if tasks.is_empty() {
        findings.push(Finding {
            severity: Severity::High,
            check: "no_tasks",
            detail: "spec has no tasks — run @task-planner".into(),
        });
    }

    for t in &tasks {
        if t.status == "failed" {
            findings.push(Finding {
                severity: Severity::Medium,
                check: "failed_task",
                detail: format!("{} \"{}\" is in failed status", t.id, t.title),
            });
        }
        if check_agents && !known_agents.contains(t.agent.as_str()) {
            findings.push(Finding {
                severity: Severity::Medium,
                check: "unknown_task_agent",
                detail: format!("{} references unknown agent `{}`", t.id, t.agent),
            });
        }
    }

    // ── AC ↔ task coverage ──────────────────────────────────────────────────
    let mut coverage: Vec<AcCoverage> = Vec::new();
    if !ac_labels.is_empty() && !tasks.is_empty() {
        let task_refs: Vec<(String, BTreeSet<String>)> = tasks
            .iter()
            .map(|t| {
                let hay = format!(
                    "{} {} {}",
                    t.title,
                    t.output_artifact.as_deref().unwrap_or(""),
                    t.inputs
                );
                (
                    t.id.clone(),
                    parse_ac_labels(&hay).into_iter().collect::<BTreeSet<_>>(),
                )
            })
            .collect();

        let any_task_cites_ac = task_refs.iter().any(|(_, refs)| !refs.is_empty());

        if !any_task_cites_ac {
            findings.push(Finding {
                severity: Severity::Low,
                check: "tasks_missing_ac_refs",
                detail: "no task cites an AC id — AC coverage cannot be verified. Add `(AC-N)` to task titles".into(),
            });
        } else {
            for ac in &ac_labels {
                let covered_by: Vec<String> = task_refs
                    .iter()
                    .filter(|(_, refs)| refs.contains(ac))
                    .map(|(id, _)| id.clone())
                    .collect();
                if covered_by.is_empty() {
                    findings.push(Finding {
                        severity: Severity::Medium,
                        check: "ac_not_covered",
                        detail: format!("{ac} is not referenced by any task"),
                    });
                }
                coverage.push(AcCoverage {
                    ac: ac.clone(),
                    covered_by,
                });
            }

            for (id, refs) in &task_refs {
                if refs.is_empty() {
                    findings.push(Finding {
                        severity: Severity::Low,
                        check: "task_without_ac_ref",
                        detail: format!("{id} cites no AC id"),
                    });
                }
            }
        }
    }

    // ── Dependencies ────────────────────────────────────────────────────────
    for dep in parse_json_str_array(&spec.depends_on) {
        match get_spec(pool, &dep).await? {
            None => findings.push(Finding {
                severity: Severity::Medium,
                check: "dependency_missing",
                detail: format!("depends_on `{dep}` does not exist"),
            }),
            Some(d) if d.status == "draft" => findings.push(Finding {
                severity: Severity::High,
                check: "dependency_not_ready",
                detail: format!("depends_on `{dep}` is still in draft"),
            }),
            Some(_) => {}
        }
    }

    // ── Constitution references ─────────────────────────────────────────────
    if let Some(text) = &body {
        let lower = text.to_lowercase();
        if lower.contains("constitution") || contains_principle_ref(text) {
            match find_project_root() {
                Ok(root) if root.join("docs/constitution.md").is_file() => {}
                Ok(_) => findings.push(Finding {
                    severity: Severity::High,
                    check: "constitution_missing",
                    detail: "spec references the constitution but docs/constitution.md is absent"
                        .into(),
                }),
                Err(_) => {}
            }
        }
    }

    sort_findings(&mut findings);

    Ok(AnalysisReport {
        spec_id: spec_id.to_string(),
        spec_status: Some(spec.status),
        ac_labels,
        ac_total_field: spec.ac_total,
        task_count: tasks.len(),
        coverage,
        findings,
    })
}

// ─── Body loading ───────────────────────────────────────────────────────────

async fn load_spec_body(pool: &SqlitePool, spec_id: &str) -> Result<Option<String>> {
    let key = format!("spec_{spec_id}");
    let Some(entry) = memory_get_full(pool, "spex-architect", &key, None).await? else {
        return Ok(None);
    };
    Ok(Some(normalise_body(&entry.value)))
}

/// The spec body may be stored as a raw markdown string, a JSON string, or a
/// JSON object. Reduce all three to plain text for scanning.
fn normalise_body(value: &str) -> String {
    match serde_json::from_str::<serde_json::Value>(value) {
        Ok(serde_json::Value::String(s)) => s,
        Ok(serde_json::Value::Object(map)) => {
            for k in ["body", "content", "spec", "markdown", "text"] {
                if let Some(serde_json::Value::String(s)) = map.get(k) {
                    return s.clone();
                }
            }
            serde_json::to_string_pretty(&serde_json::Value::Object(map)).unwrap_or_default()
        }
        _ => value.to_string(),
    }
}

// ─── Text scanners ──────────────────────────────────────────────────────────

/// Extract unique `AC-N` labels (case-insensitive), ordered by number.
fn parse_ac_labels(body: &str) -> Vec<String> {
    let bytes = body.as_bytes();
    let mut nums: BTreeSet<u32> = BTreeSet::new();
    let mut i = 0;
    while i + 3 <= bytes.len() {
        let is_ac = matches!(bytes[i], b'A' | b'a')
            && matches!(bytes[i + 1], b'C' | b'c')
            && bytes[i + 2] == b'-';
        if is_ac {
            let mut j = i + 3;
            while j < bytes.len() && bytes[j].is_ascii_digit() {
                j += 1;
            }
            if j > i + 3 {
                if let Ok(n) = body[i + 3..j].parse::<u32>() {
                    nums.insert(n);
                }
                i = j;
                continue;
            }
        }
        i += 1;
    }
    nums.into_iter().map(|n| format!("AC-{n}")).collect()
}

fn unresolved_marker(body: &str) -> Option<String> {
    let lower = body.to_lowercase();
    if body.contains("[ ]") {
        return Some("unchecked checkbox(es) in the spec body".into());
    }
    if lower.contains("awaiting approval") {
        return Some("\"Awaiting approval\" section present".into());
    }
    if lower.contains("needs_human_approval") {
        return Some("needs_human_approval entries present".into());
    }
    None
}

fn ambiguity_markers(body: &str) -> Vec<String> {
    const MARKERS: &[&str] = &["TBD", "FIXME", "???", "XXX"];
    let mut found: Vec<String> = MARKERS
        .iter()
        .filter(|m| body.contains(**m))
        .map(|m| (*m).to_string())
        .collect();
    // "TODO" as a standalone word (avoid matching e.g. "todos")
    if body
        .split(|c: char| !c.is_ascii_alphabetic())
        .any(|w| w == "TODO")
    {
        found.push("TODO".into());
    }
    found
}

fn template_placeholders(body: &str) -> Vec<String> {
    const PLACEHOLDERS: &[&str] = &[
        "<ID>",
        "<Title>",
        "<2 sentences",
        "<1 line>",
        "<list>",
        "Given/When/Then (testable)",
    ];
    PLACEHOLDERS
        .iter()
        .filter(|p| body.contains(**p))
        .map(|p| (*p).to_string())
        .collect()
}

fn contains_principle_ref(body: &str) -> bool {
    let bytes = body.as_bytes();
    let needle = b"principle";
    let lower = body.to_lowercase();
    let lb = lower.as_bytes();
    let mut i = 0;
    while i + needle.len() <= lb.len() {
        if &lb[i..i + needle.len()] == needle {
            let mut j = i + needle.len();
            while j < bytes.len() && bytes[j] == b' ' {
                j += 1;
            }
            if j < bytes.len() && bytes[j].is_ascii_digit() {
                return true;
            }
        }
        i += 1;
    }
    false
}

fn parse_json_str_array(raw: &str) -> Vec<String> {
    serde_json::from_str::<Vec<String>>(raw).unwrap_or_default()
}

fn sort_findings(findings: &mut [Finding]) {
    findings.sort_by_key(|f| match f.severity {
        Severity::High => 0,
        Severity::Medium => 1,
        Severity::Low => 2,
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_ac_labels_dedups_and_orders() {
        let body = "AC-2: foo\nac-10 bar\nAC-2 again\nAC-1 baz";
        assert_eq!(parse_ac_labels(body), vec!["AC-1", "AC-2", "AC-10"]);
    }

    #[test]
    fn parse_ac_labels_ignores_bare_ac() {
        assert!(parse_ac_labels("the AC- is empty and ACID too").is_empty());
    }

    #[test]
    fn unresolved_marker_detects_checkbox_and_sections() {
        assert!(unresolved_marker("Open Questions:\n- [ ] which db?").is_some());
        assert!(unresolved_marker("Awaiting approval: schema choice").is_some());
        assert!(unresolved_marker("all resolved").is_none());
    }

    #[test]
    fn ambiguity_markers_matches_words_not_substrings() {
        assert_eq!(ambiguity_markers("this is TBD"), vec!["TBD"]);
        assert!(ambiguity_markers("list of todos here").is_empty());
        assert_eq!(ambiguity_markers("a TODO remains"), vec!["TODO"]);
    }

    #[test]
    fn contains_principle_ref_needs_a_number() {
        assert!(contains_principle_ref("violates Principle 3"));
        assert!(contains_principle_ref("see principle  12"));
        assert!(!contains_principle_ref("a guiding principle here"));
    }

    #[test]
    fn normalise_body_unwraps_json_string_and_object() {
        assert_eq!(normalise_body("\"# SPEC\\nAC-1\""), "# SPEC\nAC-1");
        assert_eq!(normalise_body("{\"body\":\"hello\"}"), "hello");
        assert_eq!(normalise_body("# raw markdown"), "# raw markdown");
    }

    async fn seed_body(pool: &SqlitePool, spec_id: &str, body: &str) {
        crate::sdd::memory::memory_set(
            pool,
            "spex-architect",
            &format!("spec_{spec_id}"),
            &serde_json::to_string(body).unwrap(),
            None,
            Some("architecture"),
            None,
            None,
        )
        .await
        .unwrap();
    }

    #[tokio::test]
    async fn analyze_spec_flags_missing_body_and_tasks() {
        let pool = crate::sdd::test_helpers::make_pool().await;
        crate::sdd::spec::create_spec(&pool, "SPEC-1", "t", "P1", &[])
            .await
            .unwrap();

        let report = analyze_spec(&pool, "SPEC-1", &[]).await.unwrap();

        assert!(report.has_blocking());
        let checks: Vec<_> = report.findings.iter().map(|f| f.check).collect();
        assert!(checks.contains(&"no_spec_body"));
        assert!(checks.contains(&"no_tasks"));
    }

    #[tokio::test]
    async fn analyze_spec_clean_when_acs_covered() {
        let pool = crate::sdd::test_helpers::make_pool().await;
        crate::sdd::spec::create_spec(&pool, "SPEC-2", "t", "P1", &[])
            .await
            .unwrap();
        seed_body(
            &pool,
            "SPEC-2",
            "# SPEC-2\nAC-1: happy path\nAC-2: error path",
        )
        .await;
        crate::sdd::task::create_task(
            &pool,
            "T1",
            "SPEC-2",
            "build happy path (AC-1)",
            "sdd-builder",
            &[],
            None,
        )
        .await
        .unwrap();
        crate::sdd::task::create_task(
            &pool,
            "T2",
            "SPEC-2",
            "handle error path (AC-2)",
            "sdd-builder",
            &[],
            None,
        )
        .await
        .unwrap();

        let report = analyze_spec(&pool, "SPEC-2", &["sdd-builder".to_string()])
            .await
            .unwrap();

        assert!(!report.has_blocking(), "findings: {:?}", report.findings);
        assert_eq!(report.ac_labels, vec!["AC-1", "AC-2"]);
        assert_eq!(
            report
                .coverage
                .iter()
                .filter(|c| !c.covered_by.is_empty())
                .count(),
            2
        );
    }

    #[tokio::test]
    async fn analyze_spec_blocks_on_unresolved_decision() {
        let pool = crate::sdd::test_helpers::make_pool().await;
        crate::sdd::spec::create_spec(&pool, "SPEC-3", "t", "P1", &[])
            .await
            .unwrap();
        seed_body(
            &pool,
            "SPEC-3",
            "# SPEC-3\nAC-1: x\nOpen Questions:\n- [ ] which hash algorithm?",
        )
        .await;
        crate::sdd::task::create_task(
            &pool,
            "T1",
            "SPEC-3",
            "do x (AC-1)",
            "sdd-builder",
            &[],
            None,
        )
        .await
        .unwrap();

        let report = analyze_spec(&pool, "SPEC-3", &[]).await.unwrap();

        assert!(report.has_blocking());
        assert!(report
            .findings
            .iter()
            .any(|f| f.check == "unresolved_decisions"));
    }
}
