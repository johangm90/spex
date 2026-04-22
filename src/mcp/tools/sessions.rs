use anyhow::Result;
use chrono::Utc;
use serde_json::{json, Value};
use sqlx::SqlitePool;

use crate::sdd::sessions::{end_session, list_sessions, start_session, NewSession};

use super::args::{optional_bool, optional_str, required_str};

pub(super) fn tool_descriptors() -> Vec<Value> {
    vec![
        json!({
            "name": "state_session_start",
            "description": "Start a new agent/human work session and emit a SessionStarted event.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "agent": {"type": "string", "description": "Agent or user identifier"},
                    "spec": {"type": "string", "description": "Optional spec ID this session is scoped to"},
                    "task": {"type": "string", "description": "Optional task ID this session is working on"},
                    "host": {"type": "string", "description": "Optional host/environment identifier"},
                    "notes": {"type": "string", "description": "Optional free-form notes"}
                },
                "required": ["agent"]
            }
        }),
        json!({
            "name": "state_session_end",
            "description": "End an active session, recording duration and emitting a SessionEnded event.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "session_id": {"type": "string", "description": "Session ID to end"}
                },
                "required": ["session_id"]
            }
        }),
        json!({
            "name": "state_sessions_list",
            "description": "List sessions with optional filters.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "spec": {"type": "string", "description": "Filter by spec ID"},
                    "agent": {"type": "string", "description": "Filter by agent"},
                    "active": {"type": "boolean", "description": "If true, return only active (not yet ended) sessions"}
                }
            }
        }),
    ]
}

pub(super) async fn handle(
    pool: &SqlitePool,
    tool_name: &str,
    args: &Value,
) -> Option<Result<Value>> {
    match tool_name {
        "state_session_start" => Some(handle_start(pool, args).await),
        "state_session_end" => Some(handle_end(pool, args).await),
        "state_sessions_list" => Some(handle_list(pool, args).await),
        _ => None,
    }
}

async fn handle_start(pool: &SqlitePool, args: &Value) -> Result<Value> {
    let agent = required_str(args, "agent")?;
    let spec = optional_str(args, "spec");
    let task = optional_str(args, "task");
    let host = optional_str(args, "host");
    let notes = optional_str(args, "notes");

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

    Ok(json!({
        "session_id": session.id,
        "agent": session.agent,
        "spec_id": session.spec_id,
        "task_id": session.task_id,
        "host": session.host,
        "started_at": session.started_at,
        "notes": session.notes,
    }))
}

async fn handle_end(pool: &SqlitePool, args: &Value) -> Result<Value> {
    let session_id = required_str(args, "session_id")?;
    let session = end_session(pool, session_id).await?;

    Ok(json!({
        "session_id": session.id,
        "agent": session.agent,
        "started_at": session.started_at,
        "ended_at": session.ended_at,
        "duration_secs": session.duration_secs,
    }))
}

async fn handle_list(pool: &SqlitePool, args: &Value) -> Result<Value> {
    let spec = optional_str(args, "spec");
    let agent = optional_str(args, "agent");
    let active_only = optional_bool(args, "active").unwrap_or(false);

    let sessions = list_sessions(pool, spec, agent, active_only, 100).await?;

    let items: Vec<Value> = sessions
        .iter()
        .map(|s| {
            json!({
                "session_id": s.id,
                "agent": s.agent,
                "spec_id": s.spec_id,
                "task_id": s.task_id,
                "host": s.host,
                "started_at": s.started_at,
                "ended_at": s.ended_at,
                "duration_secs": s.duration_secs,
                "notes": s.notes,
            })
        })
        .collect();

    Ok(json!({ "sessions": items, "count": items.len() }))
}
