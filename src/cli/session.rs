use anyhow::Result;
use chrono::Utc;
use colored::Colorize;
use sqlx::SqlitePool;

use crate::sdd::sessions::{end_session, list_sessions, start_session, NewSession};

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
