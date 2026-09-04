use anyhow::{anyhow, Result};
use colored::Colorize;
use sqlx::SqlitePool;

use crate::config::load_config;
use crate::sdd::{
    db::find_project_root,
    event::emit_event,
    spec::get_spec,
    task::{create_task, list_tasks},
    workflow::{complete_task, fail_task, start_task},
};
use crate::tickets::{resolve_backend, sink_for};

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

pub async fn cmd_task_export(
    pool: &SqlitePool,
    spec_id: &str,
    to: Option<&str>,
    dry_run: bool,
) -> Result<()> {
    let spec = get_spec(pool, spec_id)
        .await?
        .ok_or_else(|| anyhow!("no spec with id {spec_id}"))?;
    let tasks = list_tasks(pool, Some(spec_id), None, None).await?;
    if tasks.is_empty() {
        println!("{}", format!("No tasks for {spec_id}.").dimmed());
        return Ok(());
    }

    let cfg = load_config()?;
    let backend = resolve_backend(to, cfg.tickets.as_ref())?;
    let root = find_project_root()?;
    let sink = sink_for(backend, cfg.tickets.as_ref(), &root);

    if let Err(e) = sink.preflight() {
        eprintln!(
            "{} {} backend unavailable: {e}",
            "✗".red().bold(),
            backend.name()
        );
        eprintln!("  Tasks stay in spex state; nothing was exported.");
        std::process::exit(1);
    }

    println!(
        "{} {} → {}{}",
        "Export".bold(),
        spec_id.cyan(),
        backend.name(),
        if dry_run {
            "  (dry run)".dimmed().to_string()
        } else {
            String::new()
        }
    );

    let (mut ok, mut failed) = (0usize, 0usize);
    for t in &tasks {
        match sink.export_task(t, &spec, dry_run) {
            Ok(r) => {
                ok += 1;
                let loc = r.url.clone().unwrap_or_else(|| r.external_id.clone());
                println!("  {} {}  →  {}", "✓".green(), t.id.cyan(), loc);
                if !dry_run {
                    let payload = serde_json::json!({
                        "backend": r.backend,
                        "external_id": r.external_id,
                        "url": r.url,
                        "task": t.id,
                    });
                    emit_event(
                        pool,
                        "TaskExported",
                        Some(spec_id),
                        Some("cli"),
                        &payload.to_string(),
                    )
                    .await?;
                }
            }
            Err(e) => {
                failed += 1;
                eprintln!("  {} {}  {e}", "✗".red(), t.id.cyan());
            }
        }
    }

    println!();
    println!("  {ok} exported · {failed} failed");
    if failed > 0 && ok == 0 {
        std::process::exit(1);
    }
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
