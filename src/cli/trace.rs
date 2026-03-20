use anyhow::Result;
use colored::Colorize;
use sqlx::SqlitePool;

use crate::sdd::event::query_events;

pub async fn cmd_trace(
    pool: &SqlitePool,
    spec: Option<&str>,
    agent: Option<&str>,
    limit: Option<i64>,
    offset: Option<i64>,
) -> Result<()> {
    let limit = limit.unwrap_or(50);
    let events = query_events(pool, None, spec, agent, Some(limit), None, None, offset).await?;

    if events.is_empty() {
        println!("{}", "No events found.".dimmed());
        return Ok(());
    }

    println!(
        "{:<22} {:<25} {:<12} {:<15} {}",
        "Timestamp".bold(),
        "Type".bold(),
        "Spec".bold(),
        "Agent".bold(),
        "Payload".bold()
    );
    println!("{}", "─".repeat(90).dimmed());

    for ev in &events {
        let ts = if ev.timestamp.len() >= 19 {
            ev.timestamp[..19].replace('T', " ")
        } else {
            ev.timestamp.clone()
        };

        let payload_summary = summarize_payload(&ev.payload);

        println!(
            "{:<22} {:<25} {:<12} {:<15} {}",
            ts.dimmed(),
            ev.r#type.yellow(),
            ev.spec.as_deref().unwrap_or("—").cyan(),
            ev.agent.as_deref().unwrap_or("—").dimmed(),
            payload_summary.dimmed()
        );
    }
    Ok(())
}

fn summarize_payload(payload: &str) -> String {
    if payload == "{}" || payload.is_empty() {
        return "—".to_string();
    }
    if payload.len() <= 60 {
        payload.to_string()
    } else {
        format!("{}…", &payload[..57])
    }
}
