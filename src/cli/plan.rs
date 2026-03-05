use anyhow::Result;
use colored::Colorize;
use inquire::{validator::Validation, Text};
use sqlx::SqlitePool;

use crate::sdd::{
    event::emit_event,
    spec::get_spec,
    task::{create_task, list_tasks},
};

use super::util::colorize_status;

pub async fn cmd_plan_build(pool: &SqlitePool, spec_id: &str) -> Result<()> {
    let spec = get_spec(pool, spec_id)
        .await?
        .ok_or_else(|| anyhow::anyhow!("Spec '{}' not found", spec_id))?;

    println!(
        "{}",
        format!("═══ Plan for {} ════════════════════", spec.id).cyan()
    );
    println!("  {} — {}", spec.id, spec.title);
    println!("  Status: {}", colorize_status(&spec.status));
    println!();
    println!("Add tasks interactively. Leave task ID blank to finish.");
    println!();

    let mut task_count = 0;
    loop {
        let task_id = Text::new("Task ID (blank to finish):")
            .prompt()?;

        if task_id.trim().is_empty() {
            break;
        }

        let title = Text::new("Task title:")
            .with_validator(|input: &str| {
                if input.trim().is_empty() {
                    Ok(Validation::Invalid("Title cannot be empty".into()))
                } else {
                    Ok(Validation::Valid)
                }
            })
            .prompt()?;

        let agent = Text::new("Agent (e.g. spex-backend):")
            .with_default("spex-backend")
            .prompt()?;

        let inputs_str = Text::new("Inputs (comma-separated artifact IDs, or blank):").prompt()?;

        let inputs: Vec<String> = if inputs_str.trim().is_empty() {
            vec![]
        } else {
            inputs_str
                .split(',')
                .map(|s| s.trim().to_string())
                .collect()
        };

        let output_artifact = Text::new("Output artifact ID (or blank):").prompt()?;

        let output_artifact = if output_artifact.trim().is_empty() {
            None
        } else {
            Some(output_artifact.trim().to_string())
        };

        create_task(
            pool,
            task_id.trim(),
            spec_id,
            title.trim(),
            agent.trim(),
            &inputs,
            output_artifact.as_deref(),
        )
        .await?;

        println!("{} Task {} added.", "✓".green(), task_id.trim().cyan());
        task_count += 1;
    }

    if task_count > 0 {
        emit_event(
            pool,
            "PlanBuilt",
            Some(spec_id),
            Some("human"),
            &format!("{{\"task_count\": {}}}", task_count),
        )
        .await?;
        println!("{} Plan built with {} task(s).", "✓".green(), task_count);
    } else {
        println!("{} No tasks added.", "ℹ".blue());
    }

    Ok(())
}

pub async fn cmd_plan_show(pool: &SqlitePool, spec_id: &str) -> Result<()> {
    let spec = get_spec(pool, spec_id)
        .await?
        .ok_or_else(|| anyhow::anyhow!("Spec '{}' not found", spec_id))?;

    println!(
        "{}",
        format!("═══ Plan: {} ═══════════════════════", spec.id).cyan()
    );
    println!("  {} — {}", spec.id.cyan(), spec.title.bold());
    println!("  Status: {}", colorize_status(&spec.status));
    println!();

    let tasks = list_tasks(pool, Some(spec_id)).await?;

    if tasks.is_empty() {
        println!(
            "  {}",
            "No tasks. Use `spex plan build` to add tasks.".dimmed()
        );
        return Ok(());
    }

    println!(
        "  {:<15} {:<12} {:<15} {}",
        "Task ID".bold(),
        "Status".bold(),
        "Agent".bold(),
        "Title".bold()
    );
    println!("  {}", "─".repeat(65).dimmed());

    for task in &tasks {
        println!(
            "  {:<15} {:<12} {:<15} {}",
            task.id.cyan(),
            colorize_status(&task.status),
            task.agent.dimmed(),
            task.title
        );
    }
    Ok(())
}
