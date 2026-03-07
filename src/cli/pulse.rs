use anyhow::Result;
use colored::Colorize;
use sqlx::SqlitePool;

use crate::sdd::{
    event::query_events, ops_summary::summarize_spec_operations, spec::list_specs, task::list_tasks,
};

use super::util::colorize_status;

pub async fn cmd_pulse(
    pool: &SqlitePool,
    project_dir: &str,
    since: Option<&str>,
    until: Option<&str>,
) -> Result<()> {
    let specs = list_specs(pool, project_dir).await?;
    let all_tasks = list_tasks(pool, project_dir, None).await?;
    let limit = if since.is_some() || until.is_some() {
        None
    } else {
        Some(10)
    };
    let recent_events =
        query_events(pool, project_dir, None, None, None, limit, since, until).await?;

    println!(
        "{}",
        "╔══════════════════════════════════════════════╗".cyan()
    );
    println!(
        "{}",
        "║              spex — project pulse            ║".cyan()
    );
    println!(
        "{}",
        "╚══════════════════════════════════════════════╝".cyan()
    );
    println!();

    // Specs summary
    if specs.is_empty() {
        println!("  {}", "No specs. Add with `spex spec add`.".dimmed());
    } else {
        println!("  {} Specs", "■".bold());
        println!();

        let draft_count = specs.iter().filter(|s| s.status == "draft").count();
        let approved_count = specs.iter().filter(|s| s.status == "approved").count();
        let in_prog_count = specs.iter().filter(|s| s.status == "in_progress").count();
        let blocked_count = specs.iter().filter(|s| s.status == "blocked").count();
        let stabilizing_count = specs.iter().filter(|s| s.status == "stabilizing").count();
        let done_count = specs.iter().filter(|s| s.status == "done").count();
        let paused_count = specs.iter().filter(|s| s.status == "paused").count();

        println!(
            "  {} draft  {} approved  {} in_progress  {} blocked  {} stabilizing  {} done  {} paused",
            draft_count.to_string().white(),
            approved_count.to_string().yellow(),
            in_prog_count.to_string().blue(),
            blocked_count.to_string().red(),
            stabilizing_count.to_string().yellow(),
            done_count.to_string().green(),
            paused_count.to_string().dimmed()
        );
        println!();

        let mut total_blocking_incidents = 0usize;
        let mut total_blocking_gaps = 0usize;
        let mut total_active_interrupts = 0usize;
        let mut total_verification_failures = 0usize;

        // Per-spec progress bar
        for spec in &specs {
            let spec_tasks: Vec<_> = all_tasks.iter().filter(|t| t.spec == spec.id).collect();
            let total = spec_tasks.len();
            let done = spec_tasks.iter().filter(|t| t.status == "done").count();

            let ops = summarize_spec_operations(pool, project_dir, &spec.id).await?;
            total_blocking_incidents += ops.summary.blocking_incidents;
            total_blocking_gaps += ops.summary.blocking_context_gaps;
            total_active_interrupts += ops.summary.active_interrupts;
            total_verification_failures += ops.summary.verification_failures;

            let bar = if total > 0 {
                let filled = (done * 20) / total;
                let empty = 20 - filled;
                format!(
                    "[{}{}]",
                    "█".repeat(filled).green(),
                    "░".repeat(empty).dimmed()
                )
            } else {
                format!("[{}]", "░".repeat(20).dimmed())
            };

            let mut alerts = Vec::new();
            if ops.summary.blocking_incidents > 0 {
                alerts.push(
                    format!("{} blocking incidents", ops.summary.blocking_incidents)
                        .red()
                        .to_string(),
                );
            }
            if ops.summary.blocking_context_gaps > 0 {
                alerts.push(
                    format!("{} blocking gaps", ops.summary.blocking_context_gaps)
                        .red()
                        .to_string(),
                );
            }
            if ops.summary.active_interrupts > 0 {
                alerts.push(
                    format!("{} active interrupts", ops.summary.active_interrupts)
                        .yellow()
                        .to_string(),
                );
            }
            if ops.summary.verification_failures > 0 {
                alerts.push(
                    format!("{} verify fails", ops.summary.verification_failures)
                        .yellow()
                        .to_string(),
                );
            }

            println!(
                "  {:<12} {} {:<11} {}/{} tasks{}",
                spec.id.cyan(),
                bar,
                colorize_status(&spec.status),
                done,
                total,
                if alerts.is_empty() {
                    String::new()
                } else {
                    format!("  {}", alerts.join(" | "))
                }
            );
        }

        println!();
        println!(
            "  {} blockers  {} gaps  {} interrupts  {} failing verifications",
            total_blocking_incidents.to_string().red(),
            total_blocking_gaps.to_string().red(),
            total_active_interrupts.to_string().yellow(),
            total_verification_failures.to_string().yellow(),
        );
    }

    // Recent events
    println!();
    println!("  {} Recent Activity", "▶".bold());
    if recent_events.is_empty() {
        println!("  {}", "(no events yet)".dimmed());
    } else {
        for ev in &recent_events {
            let ts = &ev.timestamp[..16].replace('T', " ");
            let spec_str = ev
                .spec
                .as_deref()
                .map(|s| format!(" [{}]", s))
                .unwrap_or_default();
            println!(
                "  {} {} {}{}",
                ts.dimmed(),
                ev.r#type.yellow(),
                ev.agent.as_deref().unwrap_or("").dimmed(),
                spec_str.dimmed()
            );
        }
    }

    println!();
    Ok(())
}
