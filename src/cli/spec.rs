use anyhow::Result;
use colored::Colorize;
use sqlx::SqlitePool;

use crate::config::load_config;
use crate::sdd::{
    spec::{create_spec, get_spec, list_specs},
    task::list_tasks,
    workflow::{approve_spec, complete_spec, start_spec},
};

use super::util::colorize_status;

pub async fn cmd_spec_add(pool: &SqlitePool, id: &str, title: &str, priority: &str) -> Result<()> {
    let spec = create_spec(pool, id, title, priority, &[]).await?;
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

pub async fn cmd_spec_approve(pool: &SqlitePool, id: &str) -> Result<()> {
    let config = load_config()?;
    let spec = approve_spec(pool, id, "human", Some(&config)).await?;
    println!("{} Spec {} approved.", "✓".green().bold(), spec.id.cyan());
    Ok(())
}

pub async fn cmd_spec_start(pool: &SqlitePool, id: &str) -> Result<()> {
    let spec = start_spec(pool, id, "human").await?;
    println!(
        "{} Spec {} is now {}.",
        "✓".green(),
        spec.id.cyan(),
        colorize_status("in_progress")
    );
    Ok(())
}

pub async fn cmd_spec_done(pool: &SqlitePool, id: &str) -> Result<()> {
    let config = load_config()?;
    match complete_spec(pool, id, "human", Some(&config)).await {
        Ok(spec) => {
            println!(
                "{} Spec {} is {}!",
                "✓".green().bold(),
                spec.id.cyan(),
                colorize_status("done")
            );
        }
        Err(err) => {
            let msg = err.to_string();
            eprintln!("{} Cannot complete spec {}", "✗".red().bold(), id.cyan());
            eprintln!("  {}", msg.yellow());
            print_spec_gate_hints(id, &msg);
            std::process::exit(1);
        }
    }
    Ok(())
}

fn print_spec_gate_hints(spec_id: &str, error_msg: &str) {
    if error_msg.contains("task(s) are still open") || error_msg.contains("open tasks") {
        eprintln!(
            "  {} Complete open tasks first: spex task list --spec {}",
            "→".blue(),
            spec_id
        );
    }
    if error_msg.contains("ac_passed") || error_msg.contains("acceptance criteria") {
        eprintln!(
            "  {} Update AC counts: spex spec show {}",
            "→".blue(),
            spec_id
        );
    }
    if error_msg.contains("missing evidence bundle")
        || error_msg.contains("missing completion summary")
        || error_msg.contains("missing successful")
        || error_msg.contains("evidence bundle")
        || error_msg.contains("artifact links")
    {
        eprintln!(
            "  {} Submit evidence:  spex policy evidence submit <bundle-id> --spec {} --summary \"...\"",
            "→".blue(),
            spec_id
        );
    }
    if error_msg.contains("requires approval")
        || error_msg.contains("waiting on approval")
        || error_msg.contains("approval")
    {
        eprintln!(
            "  {} Request approval: spex policy approval request {} --reason \"...\"",
            "→".blue(),
            spec_id
        );
        eprintln!(
            "  {} List approvals:   spex policy approval list --entity-id {}",
            "→".blue(),
            spec_id
        );
    }
}

pub async fn cmd_spec_list(
    pool: &SqlitePool,
    json: bool,
    limit: Option<i64>,
    offset: Option<i64>,
) -> Result<()> {
    let specs = list_specs(pool, limit, offset).await?;

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

pub async fn cmd_spec_show(pool: &SqlitePool, id: &str) -> Result<()> {
    let spec = get_spec(pool, id)
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
    let tasks = list_tasks(pool, Some(id), None, None).await?;
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
