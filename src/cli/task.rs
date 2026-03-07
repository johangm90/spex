use anyhow::Result;
use colored::Colorize;
use sqlx::SqlitePool;

use crate::sdd::{
    event::emit_event,
    task::{
        create_task, list_tasks, task_runtime_metadata, update_task_status, TaskLockRequirement,
    },
    task_lock::release_task_locks,
};

use super::util::colorize_status;

pub async fn cmd_task_add(
    pool: &SqlitePool,
    spec_id: &str,
    task_id: &str,
    title: &str,
    agent: &str,
    inputs: &[String],
    depends_on: &[String],
    conflicts_with: &[String],
    lock_set: &[String],
    priority: i64,
    risk_level: &str,
    execution_bucket: &str,
    estimate_points: i64,
    unblock_value: i64,
    plan_version: Option<String>,
    output_artifact: Option<String>,
) -> Result<()> {
    let lock_requirements: Vec<TaskLockRequirement> = lock_set
        .iter()
        .filter_map(|entry| {
            let (lock_type, resource) = entry.split_once(':')?;
            Some(TaskLockRequirement {
                lock_type: lock_type.to_string(),
                resource: resource.to_string(),
            })
        })
        .collect();
    let task = create_task(
        pool,
        task_id,
        spec_id,
        title,
        agent,
        inputs,
        depends_on,
        conflicts_with,
        lock_set,
        &lock_requirements,
        priority,
        risk_level,
        execution_bucket,
        estimate_points,
        unblock_value,
        plan_version.as_deref(),
        output_artifact.as_deref(),
    )
    .await?;

    let meta = task_runtime_metadata(&task);
    println!(
        "{} Task {} created for spec {}",
        "✓".green(),
        task.id.cyan(),
        task.spec.cyan()
    );
    println!(
        "  priority={} risk={} bucket={} estimate={} unblock={} locks={}",
        meta.priority,
        meta.risk_level,
        meta.execution_bucket,
        meta.estimate_points,
        meta.unblock_value,
        meta.lock_requirements.len()
    );
    Ok(())
}

pub async fn cmd_task_start(pool: &SqlitePool, id: &str) -> Result<()> {
    let task = update_task_status(pool, id, "claimed").await?;
    emit_event(
        pool,
        "TaskStarted",
        Some(&task.spec),
        Some(&task.agent),
        &format!("{{\"task\": \"{}\"}}", id),
    )
    .await?;
    println!("{} Task {} claimed.", "✓".green(), task.id.cyan());
    Ok(())
}

pub async fn cmd_task_done(pool: &SqlitePool, id: &str) -> Result<()> {
    let task = update_task_status(pool, id, "done").await?;
    let _ = release_task_locks(pool, id).await?;
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

pub async fn cmd_task_block(pool: &SqlitePool, id: &str) -> Result<()> {
    let task = update_task_status(pool, id, "blocked").await?;
    let _ = release_task_locks(pool, id).await?;
    emit_event(
        pool,
        "TaskBlocked",
        Some(&task.spec),
        Some(&task.agent),
        &format!("{{\"task\": \"{}\"}}", id),
    )
    .await?;
    println!(
        "{} Task {} marked {}.",
        "✗".red(),
        task.id.cyan(),
        colorize_status("blocked")
    );
    Ok(())
}

pub async fn cmd_task_review(pool: &SqlitePool, id: &str) -> Result<()> {
    let task = update_task_status(pool, id, "awaiting_review").await?;
    emit_event(
        pool,
        "TaskInReview",
        Some(&task.spec),
        Some(&task.agent),
        &format!("{{\"task\": \"{}\"}}", id),
    )
    .await?;
    println!(
        "{} Task {} marked {}.",
        "✓".green(),
        task.id.cyan(),
        colorize_status("awaiting_review")
    );
    Ok(())
}

pub async fn cmd_task_verify(pool: &SqlitePool, id: &str) -> Result<()> {
    let task = update_task_status(pool, id, "verifying").await?;
    emit_event(
        pool,
        "TaskVerifying",
        Some(&task.spec),
        Some(&task.agent),
        &format!("{{\"task\": \"{}\"}}", id),
    )
    .await?;
    println!(
        "{} Task {} marked {}.",
        "✓".green(),
        task.id.cyan(),
        colorize_status("verifying")
    );
    Ok(())
}

pub async fn cmd_task_cancel(pool: &SqlitePool, id: &str) -> Result<()> {
    let task = update_task_status(pool, id, "cancelled").await?;
    let _ = release_task_locks(pool, id).await?;
    emit_event(
        pool,
        "TaskCancelled",
        Some(&task.spec),
        Some(&task.agent),
        &format!("{{\"task\": \"{}\"}}", id),
    )
    .await?;
    println!(
        "{} Task {} marked {}.",
        "✓".green(),
        task.id.cyan(),
        colorize_status("cancelled")
    );
    Ok(())
}

pub async fn cmd_task_list(pool: &SqlitePool, spec_id: Option<&str>, json: bool) -> Result<()> {
    let tasks = list_tasks(pool, spec_id).await?;

    if json {
        println!("{}", serde_json::to_string_pretty(&tasks)?);
        return Ok(());
    }

    if tasks.is_empty() {
        println!("{}", "No tasks found.".dimmed());
        return Ok(());
    }

    println!(
        "{:<15} {:<12} {:<12} {:<15} {:<6} {:<20} {}",
        "Task ID".bold(),
        "Spec".bold(),
        "Status".bold(),
        "Agent".bold(),
        "Pri".bold(),
        "Bucket/Risk".bold(),
        "Title".bold()
    );
    println!("{}", "─".repeat(110).dimmed());

    for task in &tasks {
        let meta = task_runtime_metadata(task);
        println!(
            "{:<15} {:<12} {:<12} {:<15} {:<6} {:<20} {}",
            task.id.cyan(),
            task.spec.dimmed(),
            colorize_status(&task.status),
            task.agent,
            meta.priority,
            format!(
                "{}/{}/e{}/u{}",
                meta.execution_bucket, meta.risk_level, meta.estimate_points, meta.unblock_value
            ),
            task.title
        );
        if !meta.depends_on.is_empty() || !meta.lock_requirements.is_empty() {
            let deps = if meta.depends_on.is_empty() {
                "-".to_string()
            } else {
                meta.depends_on.join(",")
            };
            let locks = if meta.lock_requirements.is_empty() {
                "-".to_string()
            } else {
                meta.lock_requirements
                    .iter()
                    .map(|l| format!("{}:{}", l.lock_type, l.resource))
                    .collect::<Vec<_>>()
                    .join(",")
            };
            println!(
                "{:<15} {:<12} {:<12} {:<15} {:<6} {:<20} deps={} locks={}",
                "".dimmed(),
                "".dimmed(),
                "".dimmed(),
                "".dimmed(),
                "".dimmed(),
                "".dimmed(),
                deps,
                locks
            );
        }
    }
    Ok(())
}
