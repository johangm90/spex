use anyhow::Result;
use colored::Colorize;
use sqlx::SqlitePool;

use crate::sdd::event::query_events;
use crate::sdd::sessions::list_sessions;
use crate::sdd::task::list_tasks;

pub async fn cmd_trace(
    pool: &SqlitePool,
    spec: Option<&str>,
    agent: Option<&str>,
    task: Option<&str>,
    full: bool,
    limit: Option<i64>,
    offset: Option<i64>,
) -> Result<()> {
    let limit = limit.unwrap_or(50);
    let events = query_events(pool, None, spec, agent, Some(limit), None, None, offset).await?;

    if full {
        return print_full_timeline(pool, spec, agent, task, limit, offset, events).await;
    }

    let events = if let Some(task_id) = task {
        events
            .into_iter()
            .filter(|event| event_matches_task(event, task_id))
            .collect()
    } else {
        events
    };

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

async fn print_full_timeline(
    pool: &SqlitePool,
    spec: Option<&str>,
    agent: Option<&str>,
    task: Option<&str>,
    limit: i64,
    offset: Option<i64>,
    events: Vec<crate::sdd::event::Event>,
) -> Result<()> {
    let mut timeline = Vec::new();

    for event in events.into_iter().filter(|event| match task {
        Some(task_id) => event_matches_task(event, task_id),
        None => true,
    }) {
        let actor = event.agent.clone().unwrap_or_else(|| "—".to_string());
        timeline.push(TimelineEntry {
            timestamp: event.timestamp.clone(),
            kind: "EVENT",
            actor,
            description: format!("{} {}", event.r#type, summarize_payload(&event.payload)),
        });
    }

    for session in list_sessions(pool, spec, agent, false, limit).await? {
        if task.is_some_and(|task_id| session.task_id.as_deref() != Some(task_id)) {
            continue;
        }
        timeline.push(TimelineEntry {
            timestamp: session.started_at.clone(),
            kind: "SESSION",
            actor: session.id.clone(),
            description: format!(
                "started by {}{}",
                session.agent,
                session
                    .task_id
                    .as_deref()
                    .map(|task_id| format!(" for {task_id}"))
                    .unwrap_or_default()
            ),
        });

        if let Some(ended_at) = session.ended_at.clone() {
            timeline.push(TimelineEntry {
                timestamp: ended_at,
                kind: "SESSION",
                actor: session.id,
                description: format!("ended after {}s", session.duration_secs.unwrap_or_default()),
            });
        }
    }

    let mut tasks = list_tasks(pool, spec, Some(limit), offset).await?;
    if let Some(agent_filter) = agent {
        tasks.retain(|entry| entry.agent == agent_filter);
    }
    if let Some(task_id) = task {
        tasks.retain(|entry| entry.id == task_id);
    }

    for task in tasks {
        timeline.push(TimelineEntry {
            timestamp: task.created_at.clone(),
            kind: "TASK",
            actor: task.id.clone(),
            description: format!("created for {} by {}", task.spec, task.agent),
        });

        if task.updated_at != task.created_at {
            timeline.push(TimelineEntry {
                timestamp: task.updated_at.clone(),
                kind: "TASK",
                actor: task.id,
                description: format!("status {}", task.status),
            });
        }
    }

    timeline.sort_by(|left, right| left.timestamp.cmp(&right.timestamp));

    if timeline.is_empty() {
        println!("{}", "No trace entries found.".dimmed());
        return Ok(());
    }

    println!(
        "{:<22} {:<8} {:<18} {}",
        "Timestamp".bold(),
        "Type".bold(),
        "Agent/ID".bold(),
        "Description".bold()
    );
    println!("{}", "─".repeat(90).dimmed());

    for entry in timeline {
        println!(
            "{:<22} {:<8} {:<18} {}",
            format_timestamp(&entry.timestamp).dimmed(),
            entry.kind.yellow(),
            truncate(&entry.actor, 18).cyan(),
            entry.description.dimmed(),
        );
    }

    Ok(())
}

#[derive(Debug)]
struct TimelineEntry {
    timestamp: String,
    kind: &'static str,
    actor: String,
    description: String,
}

fn event_matches_task(event: &crate::sdd::event::Event, task_id: &str) -> bool {
    if event.payload.contains(task_id) {
        return true;
    }

    let Ok(payload) = serde_json::from_str::<serde_json::Value>(&event.payload) else {
        return false;
    };

    payload.get("task").and_then(serde_json::Value::as_str) == Some(task_id)
        || payload.get("task_id").and_then(serde_json::Value::as_str) == Some(task_id)
        || (payload
            .get("entity_kind")
            .and_then(serde_json::Value::as_str)
            == Some("task")
            && payload.get("entity_id").and_then(serde_json::Value::as_str) == Some(task_id))
}

fn format_timestamp(ts: &str) -> String {
    if ts.len() >= 19 {
        ts[..19].replace('T', " ")
    } else {
        ts.to_string()
    }
}

fn truncate(s: &str, max: usize) -> String {
    if s.len() <= max {
        s.to_string()
    } else {
        format!("{}…", &s[..max - 1])
    }
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
