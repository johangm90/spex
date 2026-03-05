use anyhow::Result;
use colored::Colorize;
use sqlx::SqlitePool;

use crate::sdd::{event::query_events, spec::list_specs, task::list_tasks};

use super::util::colorize_status;

pub async fn cmd_pulse(pool: &SqlitePool, since: Option<&str>, until: Option<&str>) -> Result<()> {
    let specs = list_specs(pool).await?;
    let all_tasks = list_tasks(pool, None).await?;
    let limit = if since.is_some() || until.is_some() {
        None
    } else {
        Some(10)
    };
    let recent_events = query_events(pool, None, None, None, limit, since, until).await?;

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
        let done_count = specs.iter().filter(|s| s.status == "done").count();
        let paused_count = specs.iter().filter(|s| s.status == "paused").count();

        println!(
            "  {} draft  {} approved  {} in_progress  {} done  {} paused",
            draft_count.to_string().white(),
            approved_count.to_string().yellow(),
            in_prog_count.to_string().blue(),
            done_count.to_string().green(),
            paused_count.to_string().dimmed()
        );
        println!();

        // Per-spec progress bar
        for spec in &specs {
            let spec_tasks: Vec<_> = all_tasks.iter().filter(|t| t.spec == spec.id).collect();
            let total = spec_tasks.len();
            let done = spec_tasks.iter().filter(|t| t.status == "done").count();

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

            println!(
                "  {:<12} {} {:<11} {}/{} tasks",
                spec.id.cyan(),
                bar,
                colorize_status(&spec.status),
                done,
                total
            );
        }
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
