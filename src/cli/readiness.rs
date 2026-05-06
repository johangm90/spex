use anyhow::Result;
use chrono::Utc;
use colored::Colorize;
use sqlx::SqlitePool;

use crate::sdd::readiness::{
    approve_review, enter_review, insert_review_requirement, list_review_requirements,
    operator_readiness, satisfy_review_requirement, spec_readiness, transition_phase,
    ReviewRequirementKind, WorkflowPhaseKind,
};

/// spex readiness spec <SPEC_ID>
/// Shows readiness report for a single spec: current phase, requirements, blockers.
pub async fn cmd_readiness_spec(pool: &SqlitePool, spec_id: &str) -> Result<()> {
    let report = spec_readiness(pool, spec_id).await?;

    println!("Readiness: {}", spec_id.bold().cyan());
    println!(
        "  Phase:        {}",
        report.current_phase.as_deref().unwrap_or("(none)").cyan()
    );
    println!(
        "  Requirements: {}/{} satisfied",
        report.review_requirements_satisfied, report.review_requirements_total
    );

    if !report.blockers.is_empty() {
        println!("  Blockers:");
        for b in &report.blockers {
            println!("    {} [{}] {}", "✗".red(), b.kind.dimmed(), b.description);
        }
    }

    if report.ready {
        println!("  Status: {} READY", "✓".green());
    } else {
        println!("  Status: {}", "NOT READY".red().bold());
    }

    Ok(())
}

/// spex readiness operator
/// Shows operator-level readiness across all specs.
pub async fn cmd_readiness_operator(pool: &SqlitePool) -> Result<()> {
    let report = operator_readiness(pool).await?;

    if report.specs.is_empty() {
        println!("{}", "No specs found.".dimmed());
        return Ok(());
    }

    println!(
        "{:<20} {:<14} {:<14} {}",
        "Spec".bold(),
        "Phase".bold(),
        "Requirements".bold(),
        "Ready".bold(),
    );
    println!("{}", "─".repeat(65).dimmed());

    for s in &report.specs {
        let phase = s.current_phase.as_deref().unwrap_or("—");
        let reqs = format!(
            "{}/{}",
            s.review_requirements_satisfied, s.review_requirements_total
        );
        let ready_col = if s.ready {
            "✓ READY".green().to_string()
        } else {
            "✗ BLOCKED".red().to_string()
        };
        println!(
            "{:<20} {:<14} {:<14} {}",
            s.spec_id.cyan(),
            phase,
            reqs,
            ready_col,
        );
    }

    println!("{}", "─".repeat(65).dimmed());
    println!(
        "Total: {}  Ready: {}  Blocked: {}",
        report.total_specs,
        report.ready_specs.to_string().green(),
        report.blocked_specs.to_string().red(),
    );

    Ok(())
}

/// spex readiness phase <SPEC_ID> <PHASE>
/// Transition a spec to a new workflow phase.
/// PHASE: planning | in_progress | review | done
pub async fn cmd_readiness_phase(
    pool: &SqlitePool,
    spec_id: &str,
    phase: &str,
    entered_by: Option<&str>,
    notes: Option<&str>,
) -> Result<()> {
    let kind = WorkflowPhaseKind::from_str(phase).ok_or_else(|| {
        anyhow::anyhow!(
            "Unknown phase '{}'. Valid: planning, in_progress, review, done",
            phase
        )
    })?;

    let new_phase = transition_phase(pool, spec_id, kind, entered_by, notes).await?;

    println!(
        "{} Spec {} transitioned to phase {}",
        "✓".green(),
        spec_id.cyan(),
        new_phase.phase.bold()
    );
    if let Some(by) = entered_by {
        println!("  By: {}", by);
    }
    if let Some(n) = notes {
        println!("  Notes: {}", n);
    }

    Ok(())
}

/// spex readiness enter-review <SPEC_ID>
/// Enter review phase and seed default requirements.
pub async fn cmd_readiness_enter_review(
    pool: &SqlitePool,
    spec_id: &str,
    agent: Option<&str>,
) -> Result<()> {
    enter_review(pool, spec_id, agent).await?;

    println!(
        "{} Spec {} entered review phase",
        "✓".green(),
        spec_id.cyan()
    );

    let reqs = list_review_requirements(pool, spec_id).await?;
    if !reqs.is_empty() {
        println!("  Requirements:");
        for r in &reqs {
            let status = if r.satisfied {
                "✓".green().to_string()
            } else {
                "✗".red().to_string()
            };
            println!("    {} [{}] {}", status, r.kind.dimmed(), r.description);
        }
    }

    Ok(())
}

/// spex readiness approve <SPEC_ID> --by <APPROVER>
/// Approve review for a spec.
pub async fn cmd_readiness_approve(
    pool: &SqlitePool,
    spec_id: &str,
    approved_by: &str,
) -> Result<()> {
    let transitioned = approve_review(pool, spec_id, approved_by).await?;

    if transitioned {
        println!(
            "{} Review approved. Spec transitioned to done.",
            "✓".green()
        );
    } else {
        println!(
            "{} Review approved. Waiting for other requirements.",
            "✓".green()
        );
    }

    Ok(())
}

/// spex readiness add-requirement <SPEC_ID> --kind <KIND> --description <DESC>
/// Add a review requirement to a spec.
pub async fn cmd_readiness_add_requirement(
    pool: &SqlitePool,
    spec_id: &str,
    kind: &str,
    description: &str,
) -> Result<()> {
    let req_kind = ReviewRequirementKind::from_str(kind).ok_or_else(|| {
        anyhow::anyhow!(
            "Unknown requirement kind '{}'. Valid: test_pass, lint_pass, review_approved, custom",
            kind
        )
    })?;

    let id = format!(
        "rreq-{}-{}",
        kind,
        Utc::now().timestamp_nanos_opt().unwrap_or(0)
    );
    let req = insert_review_requirement(pool, &id, spec_id, req_kind, description).await?;

    println!("{} Requirement added to {}", "✓".green(), spec_id.cyan());
    println!("  ID:          {}", req.id.dimmed());
    println!("  Kind:        {}", req.kind);
    println!("  Description: {}", req.description);

    Ok(())
}

/// spex readiness satisfy <REQ_ID> --by <WHO>
/// Satisfy a review requirement.
pub async fn cmd_readiness_satisfy_requirement(
    pool: &SqlitePool,
    req_id: &str,
    satisfied_by: &str,
) -> Result<()> {
    satisfy_review_requirement(pool, req_id, Some(satisfied_by)).await?;

    println!(
        "{} Requirement {} satisfied by {}",
        "✓".green(),
        req_id.cyan(),
        satisfied_by
    );

    Ok(())
}
