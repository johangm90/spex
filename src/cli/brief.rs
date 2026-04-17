use anyhow::Result;
use serde_json::json;
use sqlx::SqlitePool;

use crate::sdd::{event::query_events, memory::memory_get_full, spec::list_specs, task::list_tasks};

/// `spex brief` — compact project context dump for AI session kickoff.
///
/// Outputs a machine-readable JSON block (--json) or a human-readable
/// markdown brief suitable for pasting as context into any AI session.
pub async fn cmd_brief(pool: &SqlitePool, json_output: bool) -> Result<()> {
    let specs = list_specs(pool, None, None).await?;
    let all_tasks = list_tasks(pool, None, None, None).await?;
    let recent_events = query_events(pool, None, None, None, Some(10), None, None, None).await?;

    // Try to fetch last session context stored by spex-architect.
    let session_ctx = memory_get_full(pool, "spex-architect", "session_context", None)
        .await
        .unwrap_or(None);

    // ── Bucket specs ─────────────────────────────────────────────────────────
    let active: Vec<_> = specs
        .iter()
        .filter(|s| s.status == "in_progress")
        .collect();
    let pending_approval: Vec<_> = specs.iter().filter(|s| s.status == "draft").collect();
    let paused: Vec<_> = specs.iter().filter(|s| s.status == "paused").collect();
    let done_specs: Vec<_> = specs.iter().filter(|s| s.status == "done").collect();

    if json_output {
        // ── JSON mode ─────────────────────────────────────────────────────────
        let active_json: Vec<_> = active
            .iter()
            .map(|s| {
                let tasks: Vec<_> = all_tasks.iter().filter(|t| t.spec == s.id).collect();
                let done_count = tasks.iter().filter(|t| t.status == "done").count();
                let current_task = tasks
                    .iter()
                    .find(|t| t.status == "in_progress")
                    .map(|t| json!({"id": t.id, "title": t.title}));
                let pending_tasks: Vec<_> = tasks
                    .iter()
                    .filter(|t| t.status == "pending")
                    .map(|t| json!({"id": t.id, "title": t.title}))
                    .collect();
                json!({
                    "id": s.id,
                    "title": s.title,
                    "priority": s.priority,
                    "tasks_done": done_count,
                    "tasks_total": tasks.len(),
                    "current_task": current_task,
                    "pending_tasks": pending_tasks,
                })
            })
            .collect();

        let last_session = session_ctx
            .as_ref()
            .and_then(|m| serde_json::from_str::<serde_json::Value>(&m.value).ok());

        let output = json!({
            "generated_at": chrono::Utc::now().format("%Y-%m-%dT%H:%M:%SZ").to_string(),
            "summary": {
                "total_specs": specs.len(),
                "active": active.len(),
                "pending_approval": pending_approval.len(),
                "paused": paused.len(),
                "done": done_specs.len(),
            },
            "active_specs": active_json,
            "pending_approval": pending_approval.iter().map(|s| json!({"id": s.id, "title": s.title})).collect::<Vec<_>>(),
            "paused": paused.iter().map(|s| json!({"id": s.id, "title": s.title})).collect::<Vec<_>>(),
            "recent_events": recent_events.iter().take(5).map(|e| json!({
                "type": e.r#type,
                "spec": e.spec,
                "agent": e.agent,
                "timestamp": &e.timestamp[..16],
            })).collect::<Vec<_>>(),
            "last_session": last_session,
        });

        println!("{}", serde_json::to_string_pretty(&output)?);
        return Ok(());
    }

    // ── Human-readable markdown mode ─────────────────────────────────────────
    let now = chrono::Utc::now().format("%Y-%m-%d %H:%M UTC").to_string();
    println!("## Project Brief — {now}");
    println!();

    // Active work
    if active.is_empty() {
        println!("### Active work");
        println!("_Nothing in progress._");
        println!();
    } else {
        println!("### Active work");
        println!();
        println!("| Spec | Title | Progress | Current task |");
        println!("|------|-------|----------|--------------|");
        for s in &active {
            let tasks: Vec<_> = all_tasks.iter().filter(|t| t.spec == s.id).collect();
            let done_count = tasks.iter().filter(|t| t.status == "done").count();
            let current = tasks
                .iter()
                .find(|t| t.status == "in_progress")
                .map(|t| format!("{}: {}", t.id, t.title))
                .unwrap_or_else(|| "—".to_string());
            println!(
                "| {} | {} | {}/{} | {} |",
                s.id,
                s.title,
                done_count,
                tasks.len(),
                current
            );
        }
        println!();
    }

    // Pending approval
    if !pending_approval.is_empty() {
        println!("### Pending your approval");
        println!();
        for s in &pending_approval {
            println!("- {} \"{}\"", s.id, s.title);
        }
        println!();
    }

    // Paused
    if !paused.is_empty() {
        println!("### Paused");
        println!();
        for s in &paused {
            println!("- {} \"{}\"", s.id, s.title);
        }
        println!();
    }

    // Recent events (last session activity)
    if !recent_events.is_empty() {
        println!("### Recent activity");
        println!();
        for ev in recent_events.iter().take(5) {
            let ts = &ev.timestamp[..16].replace('T', " ");
            let spec_str = ev
                .spec
                .as_deref()
                .map(|s| format!(" [{}]", s))
                .unwrap_or_default();
            println!(
                "- `{}` {} {}{}",
                ts,
                ev.r#type,
                ev.agent.as_deref().unwrap_or(""),
                spec_str
            );
        }
        println!();
    }

    // Last session summary
    if let Some(ctx) = session_ctx {
        if let Ok(parsed) = serde_json::from_str::<serde_json::Value>(&ctx.value) {
            if let Some(summary) = parsed.get("session_summary").and_then(|v| v.as_str()) {
                println!("### Last session");
                println!();
                println!("{summary}");
                println!();
            }
            if let Some(next) = parsed.get("next_action").and_then(|v| v.as_str()) {
                println!("### Next up");
                println!();
                println!("→ {next}");
                println!();
            }
        }
    }

    // No specs at all
    if specs.is_empty() {
        println!("_No specs yet. Tell the agent what you want to build._");
        println!();
    }

    Ok(())
}
