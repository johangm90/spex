use anyhow::Result;
use colored::Colorize;
use sqlx::SqlitePool;

use crate::sdd::{
    event::emit_event,
    ops_summary::summarize_spec_operations,
    spec::{create_spec, get_spec, list_specs, update_spec_status},
    task::list_tasks,
};

use super::util::colorize_status;

pub async fn cmd_spec_add(
    pool: &SqlitePool,
    project_dir: &str,
    id: &str,
    title: &str,
    priority: &str,
) -> Result<()> {
    let spec = create_spec(pool, project_dir, id, title, priority, &[]).await?;
    println!(
        "{} Spec {} created: {}",
        "✓".green(),
        spec.id.cyan(),
        spec.title
    );
    println!(
        "  Status: {} | Priority: {}",
        colorize_status(&spec.status),
        spec.priority
    );
    Ok(())
}

pub async fn cmd_spec_approve(pool: &SqlitePool, project_dir: &str, id: &str) -> Result<()> {
    let spec = update_spec_status(pool, project_dir, id, "approved", "human").await?;
    emit_event(pool, project_dir, "SpecApproved", Some(id), Some("human"), "{}").await?;
    println!("{} Spec {} approved.", "✓".green().bold(), spec.id.cyan());
    Ok(())
}

pub async fn cmd_spec_start(pool: &SqlitePool, project_dir: &str, id: &str) -> Result<()> {
    let ops = summarize_spec_operations(pool, project_dir, id).await?;
    if ops.summary.blocking_incidents > 0 || ops.summary.blocking_context_gaps > 0 {
        return Err(anyhow::anyhow!(
            "Spec '{}' has blocking operational records ({} incidents, {} context gaps); resolve them before starting.",
            id,
            ops.summary.blocking_incidents,
            ops.summary.blocking_context_gaps
        ));
    }
    let spec = update_spec_status(pool, project_dir, id, "in_progress", "human").await?;
    emit_event(pool, project_dir, "SpecStarted", Some(id), Some("human"), "{}").await?;
    println!(
        "{} Spec {} is now {}.",
        "✓".green(),
        spec.id.cyan(),
        colorize_status("in_progress")
    );
    Ok(())
}

pub async fn cmd_spec_stabilize(pool: &SqlitePool, project_dir: &str, id: &str) -> Result<()> {
    let ops = summarize_spec_operations(pool, project_dir, id).await?;
    if ops.summary.blocking_incidents > 0 || ops.summary.blocking_context_gaps > 0 {
        return Err(anyhow::anyhow!(
            "Spec '{}' cannot enter stabilizing with blockers ({} incidents, {} context gaps).",
            id,
            ops.summary.blocking_incidents,
            ops.summary.blocking_context_gaps
        ));
    }
    let spec = update_spec_status(pool, project_dir, id, "stabilizing", "human").await?;
    emit_event(
        pool,
        project_dir,
        "SpecStabilizing",
        Some(id),
        Some("human"),
        "{}",
    )
    .await?;
    println!(
        "{} Spec {} is now {}.",
        "✓".green(),
        spec.id.cyan(),
        colorize_status("stabilizing")
    );
    Ok(())
}

pub async fn cmd_spec_done(pool: &SqlitePool, project_dir: &str, id: &str) -> Result<()> {
    let ops = summarize_spec_operations(pool, project_dir, id).await?;
    if ops.summary.blocking_incidents > 0
        || ops.summary.blocking_context_gaps > 0
        || ops.summary.verification_failures > 0
    {
        return Err(anyhow::anyhow!(
            "Spec '{}' cannot be completed with blockers or failing verification ({} incidents, {} context gaps, {} failing runs).",
            id,
            ops.summary.blocking_incidents,
            ops.summary.blocking_context_gaps,
            ops.summary.verification_failures
        ));
    }
    let spec = update_spec_status(pool, project_dir, id, "done", "human").await?;
    emit_event(
        pool,
        project_dir,
        "SpecCompleted",
        Some(id),
        Some("human"),
        "{}",
    )
    .await?;
    println!(
        "{} Spec {} is {}!",
        "✓".green().bold(),
        spec.id.cyan(),
        colorize_status("done")
    );
    Ok(())
}

pub async fn cmd_spec_list(pool: &SqlitePool, project_dir: &str, json: bool) -> Result<()> {
    let specs = list_specs(pool, project_dir).await?;

    if json {
        println!("{}", serde_json::to_string_pretty(&specs)?);
        return Ok(());
    }

    if specs.is_empty() {
        println!(
            "{}",
            "No specs found. Add one with `spex spec add <ID> <TITLE>`".dimmed()
        );
        return Ok(());
    }

    println!(
        "{:<12} {:<35} {:<12} {:<6} {:<8}",
        "ID".bold(),
        "Title".bold(),
        "Status".bold(),
        "Pri".bold(),
        "AC".bold()
    );
    println!("{}", "─".repeat(75).dimmed());

    for spec in &specs {
        let ac = if spec.ac_total > 0 {
            format!("{}/{}", spec.ac_passed, spec.ac_total)
        } else {
            "—".to_string()
        };
        println!(
            "{:<12} {:<35} {:<12} {:<6} {:<8}",
            spec.id.cyan(),
            truncate(&spec.title, 34),
            colorize_status(&spec.status),
            spec.priority,
            ac
        );
    }
    Ok(())
}

pub async fn cmd_spec_show(pool: &SqlitePool, project_dir: &str, id: &str) -> Result<()> {
    let spec = get_spec(pool, project_dir, id)
        .await?
        .ok_or_else(|| anyhow::anyhow!("Spec '{}' not found", id))?;

    println!(
        "{}",
        format!("═══ {} ══════════════════════════════════", spec.id).cyan()
    );
    println!("  {}", spec.title.bold());
    println!(
        "  Status:   {} | Priority: {}",
        colorize_status(&spec.status),
        spec.priority
    );
    let ops = summarize_spec_operations(pool, project_dir, id).await?;
    println!(
        "  Ops:      {} open incidents ({} blocking) | {} open gaps ({} blocking) | {} active interrupts | {} failing verifications",
        ops.summary.open_incidents,
        ops.summary.blocking_incidents,
        ops.summary.open_context_gaps,
        ops.summary.blocking_context_gaps,
        ops.summary.active_interrupts,
        ops.summary.verification_failures,
    );
    let ac = if spec.ac_total > 0 {
        format!("{}/{}", spec.ac_passed, spec.ac_total)
    } else {
        "—".to_string()
    };
    println!("  AC:       {}", ac);
    println!("  Created:  {}", spec.created_at);
    println!("  Updated:  {}", spec.updated_at);
    if let Some(by) = &spec.updated_by {
        println!("  By:       {}", by);
    }

    let depends: Vec<String> = serde_json::from_str(&spec.depends_on).unwrap_or_default();
    if !depends.is_empty() {
        println!("  Depends:  {}", depends.join(", "));
    }

    let agents: Vec<String> = serde_json::from_str(&spec.agents).unwrap_or_default();
    if !agents.is_empty() {
        println!("  Agents:   {}", agents.join(", "));
    }

    println!();
    let tasks = list_tasks(pool, project_dir, Some(id)).await?;
    if !ops.next_actionable_tasks.is_empty() {
        println!("  {}", "Next actionable tasks:".bold());
        for task in ops.next_actionable_tasks.iter().take(5) {
            println!(
                "    {} {} {}  {}",
                task.id.cyan(),
                colorize_status(&task.status),
                task.agent.dimmed(),
                task.title
            );
        }
        println!();
    }

    if tasks.is_empty() {
        println!(
            "  {}",
            "No tasks yet. Use `spex plan build` to add tasks.".dimmed()
        );
    } else {
        println!("  {}", "Tasks:".bold());
        for task in &tasks {
            println!(
                "    {} {} {}  {}",
                task.id.cyan(),
                colorize_status(&task.status),
                task.agent.dimmed(),
                task.title
            );
        }
    }
    Ok(())
}

fn truncate(s: &str, max: usize) -> String {
    if s.len() <= max {
        s.to_string()
    } else {
        format!("{}…", &s[..max - 1])
    }
}
