use anyhow::Result;
use colored::Colorize;
use sqlx::SqlitePool;

use crate::sdd::analyze::{analyze_spec, Severity};

/// `spex analyze <SPEC_ID>` — cross-artifact consistency check before implementation.
///
/// Returns `true` when the report contains a HIGH-severity finding, so the
/// caller can exit non-zero.
pub async fn cmd_analyze(pool: &SqlitePool, spec_id: &str, json: bool) -> Result<bool> {
    let known_agents = crate::skills_mgr::bundled_agent_names();
    let report = analyze_spec(pool, spec_id, &known_agents).await?;

    if json {
        println!("{}", serde_json::to_string_pretty(&report)?);
        return Ok(report.has_blocking());
    }

    println!("{} {}", "Analyze:".bold(), spec_id.bold().cyan());
    println!(
        "  Status: {}  ·  ACs: {}  ·  Tasks: {}",
        report.spec_status.as_deref().unwrap_or("(none)"),
        if report.ac_labels.is_empty() {
            "0".to_string()
        } else {
            format!(
                "{} ({})",
                report.ac_labels.len(),
                report.ac_labels.join(", ")
            )
        },
        report.task_count,
    );

    if !report.coverage.is_empty() {
        let covered = report
            .coverage
            .iter()
            .filter(|c| !c.covered_by.is_empty())
            .count();
        println!("  AC coverage: {}/{}", covered, report.coverage.len());
    }

    if report.findings.is_empty() {
        println!("  {} no findings", "✓".green());
        return Ok(false);
    }

    println!();
    for f in &report.findings {
        let tag = match f.severity {
            Severity::High => f.severity.label().red().bold(),
            Severity::Medium => f.severity.label().yellow().bold(),
            Severity::Low => f.severity.label().dimmed(),
        };
        println!("  {tag}  {}  {}", f.check.cyan(), f.detail);
    }

    let (h, m, l) = report.counts();
    println!();
    println!(
        "  {} high · {} medium · {} low",
        h.to_string().red(),
        m.to_string().yellow(),
        l,
    );

    if report.has_blocking() {
        println!("  {} not ready to implement", "✗".red().bold());
    } else {
        println!("  {} no blockers", "✓".green());
    }

    Ok(report.has_blocking())
}
