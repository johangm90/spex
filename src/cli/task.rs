use anyhow::Result;
use colored::Colorize;
use sqlx::SqlitePool;

use crate::config::load_config;
use crate::sdd::{
    task::{create_task, list_tasks},
    workflow::{complete_task, fail_task, start_task},
};

use super::util::colorize_status;

pub async fn cmd_task_add(
    pool: &SqlitePool,
    spec_id: &str,
    task_id: &str,
    title: &str,
    agent: &str,
    inputs: &[String],
    output_artifact: Option<String>,
) -> Result<()> {
    let task = create_task(
        pool,
        task_id,
        spec_id,
        title,
        agent,
        inputs,
        output_artifact.as_deref(),
    )
    .await?;

    println!(
        "{} Task {} created for spec {}",
        "✓".green(),
        task.id.cyan(),
        task.spec.cyan()
    );
    Ok(())
}

pub async fn cmd_task_start(pool: &SqlitePool, id: &str, updated_by: &str) -> Result<()> {
    let task = start_task(pool, id, updated_by).await?;
    println!("{} Task {} started.", "✓".green(), task.id.cyan());
    Ok(())
}

pub async fn cmd_task_done(pool: &SqlitePool, id: &str, updated_by: &str) -> Result<()> {
    let config = load_config()?;
    match complete_task(pool, id, updated_by, Some(&config)).await {
        Ok(task) => {
            println!(
                "{} Task {} marked {}.",
                "✓".green().bold(),
                task.id.cyan(),
                colorize_status("done")
            );
        }
        Err(err) => {
            let msg = err.to_string();
            eprintln!("{} Cannot complete task {}", "✗".red().bold(), id.cyan());
            eprintln!("  {}", msg.yellow());
            print_task_gate_hints(id, &msg);
            std::process::exit(1);
        }
    }
    Ok(())
}

fn print_task_gate_hints(task_id: &str, error_msg: &str) {
    if error_msg.contains("missing evidence bundle")
        || error_msg.contains("missing completion summary")
        || error_msg.contains("missing successful")
        || error_msg.contains("evidence bundle")
    {
        eprintln!(
            "  {} Submit evidence:  spex policy evidence submit <bundle-id> --spec <spec-id> --task {} --summary \"...\"",
            "→".blue(),
            task_id
        );
    }
    if error_msg.contains("requires approval")
        || error_msg.contains("waiting on approval")
        || error_msg.contains("approval")
    {
        eprintln!(
            "  {} Request approval: spex policy approval request {} --reason \"...\"",
            "→".blue(),
            task_id
        );
        eprintln!(
            "  {} List approvals:   spex policy approval list --entity-id {}",
            "→".blue(),
            task_id
        );
    }
}

pub async fn cmd_task_fail(pool: &SqlitePool, id: &str) -> Result<()> {
    let task = fail_task(pool, id).await?;
    println!(
        "{} Task {} marked {}.",
        "✗".red(),
        task.id.cyan(),
        colorize_status("failed")
    );
    Ok(())
}

pub async fn cmd_task_list(
    pool: &SqlitePool,
    spec_id: Option<&str>,
    json: bool,
    limit: Option<i64>,
    offset: Option<i64>,
) -> Result<()> {
    let tasks = list_tasks(pool, spec_id, limit, offset).await?;

    if json {
        println!("{}", serde_json::to_string_pretty(&tasks)?);
        return Ok(());
    }

    if tasks.is_empty() {
        println!("{}", "No tasks found.".dimmed());
        return Ok(());
    }

    println!(
        "{:<15} {:<12} {:<12} {:<15} {}",
        "Task ID".bold(),
        "Spec".bold(),
        "Status".bold(),
        "Agent".bold(),
        "Title".bold()
    );
    println!("{}", "─".repeat(75).dimmed());

    for task in &tasks {
        println!(
            "{:<15} {:<12} {:<12} {:<15} {}",
            task.id.cyan(),
            task.spec.dimmed(),
            colorize_status(&task.status),
            task.agent,
            task.title
        );
    }
    Ok(())
}
