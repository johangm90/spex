use anyhow::Result;
use colored::Colorize;
use inquire::{validator::Validation, Text};
use sqlx::SqlitePool;

use crate::sdd::{
    event::emit_event,
    ops_summary::summarize_spec_operations,
    plan_version::{create_plan_version, list_plan_versions, supersede_plan_versions},
    spec::get_spec,
    task::{create_task, list_tasks, task_runtime_metadata},
};

use super::util::colorize_status;

pub async fn cmd_plan_build(pool: &SqlitePool, project_dir: &str, spec_id: &str) -> Result<()> {
    let spec = get_spec(pool, project_dir, spec_id)
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
        let task_id = Text::new("Task ID (blank to finish):").prompt()?;

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
            project_dir,
            task_id.trim(),
            spec_id,
            title.trim(),
            agent.trim(),
            &inputs,
            &[],
            &[],
            &[],
            &[],
            100,
            "medium",
            "coordinated_parallel",
            3,
            0,
            None,
            output_artifact.as_deref(),
        )
        .await?;

        println!("{} Task {} added.", "✓".green(), task_id.trim().cyan());
        task_count += 1;
    }

    if task_count > 0 {
        let tasks = list_tasks(pool, project_dir, Some(spec_id)).await?;
        let existing_versions = list_plan_versions(pool, project_dir, Some(spec_id)).await?;
        let next_version = existing_versions
            .iter()
            .map(|p| p.version)
            .max()
            .unwrap_or(0)
            + 1;
        supersede_plan_versions(pool, project_dir, spec_id).await?;
        let plan_id = format!("PLAN-{}-v{}", spec_id, next_version);
        let plan_json = serde_json::json!({
            "spec": spec_id,
            "version": next_version,
            "task_count": tasks.len(),
            "tasks": tasks,
        });
        create_plan_version(
            pool,
            project_dir,
            &plan_id,
            spec_id,
            next_version,
            Some("interactive plan build"),
            &plan_json.to_string(),
        )
        .await?;
        emit_event(
            pool,
            project_dir,
            "PlanBuilt",
            Some(spec_id),
            Some("human"),
            &format!(
                "{{\"task_count\": {}, \"plan_version\": \"{}\"}}",
                task_count, plan_id
            ),
        )
        .await?;
        println!("{} Plan built with {} task(s).", "✓".green(), task_count);
        println!("{} Active plan version: {}", "✓".green(), plan_id.cyan());
    } else {
        println!("{} No tasks added.", "ℹ".blue());
    }

    Ok(())
}

pub async fn cmd_plan_show(pool: &SqlitePool, project_dir: &str, spec_id: &str) -> Result<()> {
    let spec = get_spec(pool, project_dir, spec_id)
        .await?
        .ok_or_else(|| anyhow::anyhow!("Spec '{}' not found", spec_id))?;

    println!(
        "{}",
        format!("═══ Plan: {} ═══════════════════════", spec.id).cyan()
    );
    println!("  {} — {}", spec.id.cyan(), spec.title.bold());
    println!("  Status: {}", colorize_status(&spec.status));
    let ops = summarize_spec_operations(pool, project_dir, spec_id).await?;
    println!(
        "  Ops: {} blocking incidents | {} blocking gaps | {} active interrupts | {} failing verifications",
        ops.summary.blocking_incidents,
        ops.summary.blocking_context_gaps,
        ops.summary.active_interrupts,
        ops.summary.verification_failures,
    );
    if ops.summary.blocking_incidents > 0 || ops.summary.blocking_context_gaps > 0 {
        println!(
            "  {}",
            "Spec is not ready for the next wave until blockers are resolved.".red()
        );
    } else if !ops.next_actionable_tasks.is_empty() {
        println!("  {}", "Next actionable tasks:".bold());
        for task in ops.next_actionable_tasks.iter().take(5) {
            let meta = task_runtime_metadata(task);
            println!(
                "    {} {} {}  {} [{} / {}]",
                task.id.cyan(),
                colorize_status(&task.status),
                task.agent.dimmed(),
                task.title,
                meta.execution_bucket,
                meta.risk_level
            );
        }
    }
    println!();

    let tasks = list_tasks(pool, project_dir, Some(spec_id)).await?;

    if tasks.is_empty() {
        println!(
            "  {}",
            "No tasks. Use `spex plan build` to add tasks.".dimmed()
        );
        return Ok(());
    }

    println!(
        "  {:<15} {:<12} {:<15} {:<8} {:<20} {}",
        "Task ID".bold(),
        "Status".bold(),
        "Agent".bold(),
        "Pri".bold(),
        "Bucket/Risk".bold(),
        "Title".bold()
    );
    println!("  {}", "─".repeat(100).dimmed());

    for task in &tasks {
        let meta = task_runtime_metadata(task);
        println!(
            "  {:<15} {:<12} {:<15} {:<8} {:<20} {}",
            task.id.cyan(),
            colorize_status(&task.status),
            task.agent.dimmed(),
            meta.priority,
            format!(
                "{}/{}/e{}/u{}",
                meta.execution_bucket, meta.risk_level, meta.estimate_points, meta.unblock_value
            ),
            task.title
        );
        if !meta.depends_on.is_empty()
            || !meta.lock_requirements.is_empty()
            || !meta.conflicts_with.is_empty()
        {
            let deps = if meta.depends_on.is_empty() {
                "-".to_string()
            } else {
                meta.depends_on.join(",")
            };
            let conflicts = if meta.conflicts_with.is_empty() {
                "-".to_string()
            } else {
                meta.conflicts_with.join(",")
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
                "  {:<15} {:<12} {:<15} {:<8} {:<20} deps={} conflicts={} locks={}",
                "".dimmed(),
                "".dimmed(),
                "".dimmed(),
                "".dimmed(),
                "".dimmed(),
                deps,
                conflicts,
                locks
            );
        }
    }
    Ok(())
}

pub async fn cmd_plan_dag(pool: &SqlitePool, project_dir: &str, spec_id: &str) -> Result<()> {
    let spec = get_spec(pool, project_dir, spec_id)
        .await?
        .ok_or_else(|| anyhow::anyhow!("Spec '{}' not found", spec_id))?;
    let tasks = list_tasks(pool, project_dir, Some(spec_id)).await?;
    if tasks.is_empty() {
        println!("{}", "No tasks to visualize.".dimmed());
        return Ok(());
    }
    println!(
        "{}",
        format!("═══ DAG: {} ═══════════════════════", spec.id).cyan()
    );
    println!("  {} — {}", spec.id.cyan(), spec.title.bold());
    println!();
    for task in &tasks {
        let meta = task_runtime_metadata(task);
        let deps = if meta.depends_on.is_empty() {
            "(root)".to_string()
        } else {
            meta.depends_on.join(", ")
        };
        let conflicts = if meta.conflicts_with.is_empty() {
            String::new()
        } else {
            format!(" | conflicts: {}", meta.conflicts_with.join(", "))
        };
        println!("  {} <- {}{}", task.id.cyan(), deps, conflicts);
    }
    println!();
    println!("  {}", "Edges: dependency -> task".dimmed());
    for task in &tasks {
        let meta = task_runtime_metadata(task);
        for dep in &meta.depends_on {
            println!("  {} -> {}", dep.dimmed(), task.id.cyan());
        }
    }
    Ok(())
}
