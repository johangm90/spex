use anyhow::Result;
use colored::Colorize;
use sqlx::SqlitePool;

use crate::sdd::{
    event::emit_event,
    task::{create_task, list_tasks, update_task_status},
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

pub async fn cmd_task_start(pool: &SqlitePool, id: &str) -> Result<()> {
    let task = update_task_status(pool, id, "in_progress").await?;
    emit_event(
        pool,
        "TaskStarted",
        Some(&task.spec),
        Some(&task.agent),
        &format!("{{\"task\": \"{}\"}}", id),
    )
    .await?;
    println!("{} Task {} started.", "✓".green(), task.id.cyan());
    Ok(())
}

pub async fn cmd_task_done(pool: &SqlitePool, id: &str) -> Result<()> {
    let task = update_task_status(pool, id, "done").await?;
    emit_event(
        pool,
        "TaskCompleted",
        Some(&task.spec),
        Some(&task.agent),
        &format!("{{\"task\": \"{}\"}}", id),
    )
    .await?;
    println!(
        "{} Task {} marked {}.",
        "✓".green().bold(),
        task.id.cyan(),
        colorize_status("done")
    );
    Ok(())
}

pub async fn cmd_task_fail(pool: &SqlitePool, id: &str) -> Result<()> {
    let task = update_task_status(pool, id, "failed").await?;
    emit_event(
        pool,
        "TaskFailed",
        Some(&task.spec),
        Some(&task.agent),
        &format!("{{\"task\": \"{}\"}}", id),
    )
    .await?;
    println!(
        "{} Task {} marked {}.",
        "✗".red(),
        task.id.cyan(),
        colorize_status("failed")
    );
    Ok(())
}

pub async fn cmd_task_list(pool: &SqlitePool, spec_id: Option<&str>, json: bool, limit: Option<i64>, offset: Option<i64>) -> Result<()> {
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
