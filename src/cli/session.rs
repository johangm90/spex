use anyhow::Result;
use chrono::Utc;
use colored::Colorize;
use sqlx::SqlitePool;

use crate::sdd::readiness::list_checkpoints;
use crate::sdd::sessions::{
    end_session, list_sessions, restore_session_checkpoint, save_session_checkpoint, start_session,
    NewSession,
};

pub async fn cmd_session_start(
    pool: &SqlitePool,
    agent: &str,
    spec: Option<&str>,
    task: Option<&str>,
    host: Option<&str>,
    notes: Option<&str>,
) -> Result<()> {
    let id = format!("sess-{}", Utc::now().timestamp_nanos_opt().unwrap_or(0));
    let session = start_session(
        pool,
        NewSession {
            id: &id,
            agent,
            spec_id: spec,
            task_id: task,
            host,
            notes,
        },
    )
    .await?;

    println!("{} Session started", "✓".green());
    println!("  ID:    {}", session.id.cyan());
    println!("  Agent: {}", session.agent);
    if let Some(s) = &session.spec_id {
        println!("  Spec:  {}", s);
    }
    if let Some(t) = &session.task_id {
        println!("  Task:  {}", t);
    }
    Ok(())
}

pub async fn cmd_session_end(pool: &SqlitePool, session_id: &str) -> Result<()> {
    let session = end_session(pool, session_id).await?;
    let duration = session.duration_secs.unwrap_or(0);
    println!(
        "{} Session {} ended — duration: {}s",
        "✓".green(),
        session.id.cyan(),
        duration
    );
    Ok(())
}

/// spex session checkpoint <SESSION_ID> --agent <AGENT> [--spec <SPEC>] [--task <TASK>] [--label <LABEL>] --data <JSON>
pub async fn cmd_session_checkpoint(
    pool: &SqlitePool,
    session_id: &str,
    agent: &str,
    spec: Option<&str>,
    task: Option<&str>,
    label: Option<&str>,
    data_json: &str,
) -> Result<()> {
    let data: serde_json::Value = serde_json::from_str(data_json)
        .map_err(|e| anyhow::anyhow!("Invalid JSON for --data: {}", e))?;

    let cp = save_session_checkpoint(pool, session_id, agent, spec, task, data, label).await?;

    let label_suffix = cp
        .label
        .as_deref()
        .map(|l| format!("  {}", l.dimmed()))
        .unwrap_or_default();
    println!(
        "{} Checkpoint saved: {}{}",
        "✓".green(),
        cp.id.cyan(),
        label_suffix
    );
    Ok(())
}

/// spex session restore <SESSION_ID> [--checkpoint <CHECKPOINT_ID>]
/// Restores latest checkpoint if --checkpoint not given.
pub async fn cmd_session_restore(
    pool: &SqlitePool,
    session_id: &str,
    checkpoint_id: Option<&str>,
) -> Result<()> {
    let cp = restore_session_checkpoint(pool, session_id, checkpoint_id).await?;

    println!("{} Checkpoint restored: {}", "✓".green(), cp.id.cyan());
    if let Some(label) = &cp.label {
        println!("  Label:   {}", label);
    }
    println!("  Agent:   {}", cp.agent);
    println!("  Saved:   {}", cp.saved_at);
    println!("{}", "--- checkpoint data ---".dimmed());

    let pretty = serde_json::from_str::<serde_json::Value>(&cp.checkpoint_data)
        .map(|v| serde_json::to_string_pretty(&v).unwrap_or(cp.checkpoint_data.clone()))
        .unwrap_or(cp.checkpoint_data.clone());
    println!("{}", pretty);

    Ok(())
}

/// spex session checkpoints <SESSION_ID>
/// List all checkpoints for a session.
pub async fn cmd_session_checkpoints(pool: &SqlitePool, session_id: &str) -> Result<()> {
    let checkpoints = list_checkpoints(pool, session_id).await?;

    if checkpoints.is_empty() {
        println!("{}", "No checkpoints found.".dimmed());
        return Ok(());
    }

    println!(
        "{:<30} {:<20} {:<20} {}",
        "ID".bold(),
        "Label".bold(),
        "Agent".bold(),
        "Saved".bold(),
    );
    println!("{}", "─".repeat(90).dimmed());

    for cp in &checkpoints {
        let label_col = cp.label.as_deref().unwrap_or("—");
        let saved = cp
            .saved_at
            .chars()
            .take(19)
            .collect::<String>()
            .replace('T', " ");
        println!(
            "{:<30} {:<20} {:<20} {}",
            cp.id.cyan(),
            label_col,
            cp.agent,
            saved,
        );
    }

    Ok(())
}

pub async fn cmd_session_list(
    pool: &SqlitePool,
    spec: Option<&str>,
    agent: Option<&str>,
    active: bool,
) -> Result<()> {
    let sessions = list_sessions(pool, spec, agent, active, 100).await?;

    if sessions.is_empty() {
        println!("{}", "No sessions found.".dimmed());
        return Ok(());
    }

    println!(
        "{:<10} {:<20} {:<12} {:<12} {:<20} {}",
        "ID".bold(),
        "Agent".bold(),
        "Spec".bold(),
        "Task".bold(),
        "Started".bold(),
        "Ended / Active".bold(),
    );
    println!("{}", "─".repeat(90).dimmed());

    for s in &sessions {
        let id_short = s.id.chars().take(8).collect::<String>();
        let spec_col = s.spec_id.as_deref().unwrap_or("—");
        let task_col = s.task_id.as_deref().unwrap_or("—");
        // Trim timestamp to "YYYY-MM-DD HH:MM"
        let started = s
            .started_at
            .chars()
            .take(16)
            .collect::<String>()
            .replace('T', " ");
        let ended_col = match &s.ended_at {
            Some(ts) => ts.chars().take(16).collect::<String>().replace('T', " "),
            None => "active".green().to_string(),
        };

        println!(
            "{:<10} {:<20} {:<12} {:<12} {:<20} {}",
            id_short.cyan(),
            s.agent,
            spec_col,
            task_col,
            started,
            ended_col,
        );
    }

    Ok(())
}
