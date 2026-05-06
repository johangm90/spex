use anyhow::{anyhow, Result};
use chrono::Utc;
use serde_json::json;
use sqlx::SqlitePool;

use crate::sdd::event::emit_event;

#[derive(Debug, Clone)]
pub struct Session {
    pub id: String,
    pub agent: String,
    pub spec_id: Option<String>,
    pub task_id: Option<String>,
    pub host: Option<String>,
    pub started_at: String,
    pub ended_at: Option<String>,
    pub duration_secs: Option<i64>,
    pub notes: Option<String>,
    #[allow(dead_code)]
    pub created_at: String,
}

type SessionRow = (
    String,
    String,
    Option<String>,
    Option<String>,
    Option<String>,
    String,
    Option<String>,
    Option<i64>,
    Option<String>,
    String,
);

fn row_to_session(r: SessionRow) -> Session {
    Session {
        id: r.0,
        agent: r.1,
        spec_id: r.2,
        task_id: r.3,
        host: r.4,
        started_at: r.5,
        ended_at: r.6,
        duration_secs: r.7,
        notes: r.8,
        created_at: r.9,
    }
}

pub struct NewSession<'a> {
    pub id: &'a str,
    pub agent: &'a str,
    pub spec_id: Option<&'a str>,
    pub task_id: Option<&'a str>,
    pub host: Option<&'a str>,
    pub notes: Option<&'a str>,
}

/// Insert a new session row and emit a SessionStarted domain event.
pub async fn start_session(pool: &SqlitePool, session: NewSession<'_>) -> Result<Session> {
    let now = Utc::now().to_rfc3339();
    sqlx::query(
        "INSERT INTO sessions (id, agent, spec_id, task_id, host, started_at, notes, created_at)
         VALUES (?, ?, ?, ?, ?, ?, ?, ?)",
    )
    .bind(session.id)
    .bind(session.agent)
    .bind(session.spec_id)
    .bind(session.task_id)
    .bind(session.host)
    .bind(&now)
    .bind(session.notes)
    .bind(&now)
    .execute(pool)
    .await?;

    let payload = json!({
        "session_id": session.id,
        "agent": session.agent,
        "spec_id": session.spec_id,
        "host": session.host,
    });
    emit_event(
        pool,
        "SessionStarted",
        session.spec_id,
        Some(session.agent),
        &payload.to_string(),
    )
    .await?;

    get_session(pool, session.id)
        .await?
        .ok_or_else(|| anyhow!("Session not found after insert: {}", session.id))
}

/// Set ended_at + duration_secs on a session and emit a SessionEnded event.
pub async fn end_session(pool: &SqlitePool, session_id: &str) -> Result<Session> {
    let session = get_session(pool, session_id)
        .await?
        .ok_or_else(|| anyhow!("Session not found: {}", session_id))?;

    let now = Utc::now().to_rfc3339();
    let started = chrono::DateTime::parse_from_rfc3339(&session.started_at)
        .map_err(|e| anyhow!("Invalid started_at: {}", e))?;
    let ended = chrono::DateTime::parse_from_rfc3339(&now)
        .map_err(|e| anyhow!("Invalid ended_at: {}", e))?;
    let duration_secs = (ended - started).num_seconds();

    sqlx::query("UPDATE sessions SET ended_at = ?, duration_secs = ? WHERE id = ?")
        .bind(&now)
        .bind(duration_secs)
        .bind(session_id)
        .execute(pool)
        .await?;

    let payload = json!({
        "session_id": session_id,
        "duration_secs": duration_secs,
    });
    emit_event(
        pool,
        "SessionEnded",
        session.spec_id.as_deref(),
        Some(session.agent.as_str()),
        &payload.to_string(),
    )
    .await?;

    get_session(pool, session_id)
        .await?
        .ok_or_else(|| anyhow!("Session not found after update: {}", session_id))
}

/// Query sessions with optional filters.
pub async fn list_sessions(
    pool: &SqlitePool,
    spec_id: Option<&str>,
    agent: Option<&str>,
    active_only: bool,
    limit: i64,
) -> Result<Vec<Session>> {
    let mut sql = String::from(
        "SELECT id, agent, spec_id, task_id, host, started_at, ended_at, \
         duration_secs, notes, created_at FROM sessions WHERE 1=1",
    );
    if spec_id.is_some() {
        sql.push_str(" AND spec_id = ?");
    }
    if agent.is_some() {
        sql.push_str(" AND agent = ?");
    }
    if active_only {
        sql.push_str(" AND ended_at IS NULL");
    }
    sql.push_str(" ORDER BY started_at DESC LIMIT ?");

    let mut q = sqlx::query_as::<_, SessionRow>(&sql);
    if let Some(s) = spec_id {
        q = q.bind(s);
    }
    if let Some(a) = agent {
        q = q.bind(a);
    }
    q = q.bind(limit);

    let rows = q.fetch_all(pool).await?;
    Ok(rows.into_iter().map(row_to_session).collect())
}

/// Get a single session by ID.
pub async fn get_session(pool: &SqlitePool, id: &str) -> Result<Option<Session>> {
    let row = sqlx::query_as::<_, SessionRow>(
        "SELECT id, agent, spec_id, task_id, host, started_at, ended_at, \
         duration_secs, notes, created_at FROM sessions WHERE id = ?",
    )
    .bind(id)
    .fetch_optional(pool)
    .await?;
    Ok(row.map(row_to_session))
}

/// Save a named checkpoint for the current session state.
pub async fn save_session_checkpoint(
    pool: &SqlitePool,
    session_id: &str,
    agent: &str,
    spec_id: Option<&str>,
    task_id: Option<&str>,
    checkpoint_data: serde_json::Value,
    label: Option<&str>,
) -> Result<crate::sdd::readiness::SessionCheckpoint> {
    // 1. Verify the session exists.
    get_session(pool, session_id)
        .await?
        .ok_or_else(|| anyhow!("Session not found: {}", session_id))?;

    // 2. Generate a unique checkpoint ID.
    let id = format!(
        "ckpt-{}",
        chrono::Utc::now().timestamp_nanos_opt().unwrap_or(0)
    );

    // 3. Persist the checkpoint.
    let checkpoint = crate::sdd::readiness::save_checkpoint(
        pool,
        &id,
        session_id,
        spec_id,
        task_id,
        agent,
        &checkpoint_data.to_string(),
        label,
    )
    .await?;

    // 4. Emit domain event.
    let payload = json!({
        "session_id": session_id,
        "checkpoint_id": id,
        "agent": agent,
        "label": label.unwrap_or(""),
    });
    emit_event(
        pool,
        "SessionCheckpointSaved",
        spec_id,
        Some(agent),
        &payload.to_string(),
    )
    .await?;

    // 5. Return the checkpoint.
    Ok(checkpoint)
}

/// Restore session context from a checkpoint (returns the checkpoint data for the caller to apply).
pub async fn restore_session_checkpoint(
    pool: &SqlitePool,
    session_id: &str,
    checkpoint_id: Option<&str>,
) -> Result<crate::sdd::readiness::SessionCheckpoint> {
    let checkpoint = if let Some(ckpt_id) = checkpoint_id {
        // 1a. Query by ID and verify it belongs to this session.
        crate::sdd::readiness::get_checkpoint_by_id(pool, ckpt_id, session_id)
            .await?
            .ok_or_else(|| {
                anyhow!(
                    "Checkpoint '{}' not found for session '{}'",
                    ckpt_id,
                    session_id
                )
            })?
    } else {
        // 1b. Fetch the latest checkpoint for this session.
        crate::sdd::readiness::get_latest_checkpoint(pool, session_id)
            .await?
            .ok_or_else(|| anyhow!("No checkpoints found for session '{}'", session_id))?
    };

    // 3. Emit domain event.
    let payload = json!({
        "session_id": session_id,
        "checkpoint_id": checkpoint.id,
    });
    emit_event(
        pool,
        "SessionCheckpointRestored",
        checkpoint.spec_id.as_deref(),
        Some(checkpoint.agent.as_str()),
        &payload.to_string(),
    )
    .await?;

    // 4. Return the checkpoint.
    Ok(checkpoint)
}

#[cfg(test)]
mod tests {
    use super::super::test_helpers::open_test_db;
    use super::*;

    #[tokio::test]
    async fn test_start_and_end_session() {
        let pool = open_test_db().await;

        let session = start_session(
            &pool,
            NewSession {
                id: "sess-001",
                agent: "sdd-builder",
                spec_id: None,
                task_id: None,
                host: Some("localhost"),
                notes: Some("test session"),
            },
        )
        .await
        .unwrap();

        assert_eq!(session.id, "sess-001");
        assert_eq!(session.agent, "sdd-builder");
        assert!(session.ended_at.is_none());

        let ended = end_session(&pool, "sess-001").await.unwrap();
        assert!(ended.ended_at.is_some());
        assert!(ended.duration_secs.is_some());
    }

    #[tokio::test]
    async fn test_list_sessions() {
        let pool = open_test_db().await;

        start_session(
            &pool,
            NewSession {
                id: "sess-a",
                agent: "agent-x",
                spec_id: None,
                task_id: None,
                host: None,
                notes: None,
            },
        )
        .await
        .unwrap();

        let sessions = list_sessions(&pool, None, Some("agent-x"), false, 10)
            .await
            .unwrap();
        assert_eq!(sessions.len(), 1);

        let active = list_sessions(&pool, None, None, true, 10).await.unwrap();
        assert_eq!(active.len(), 1);

        end_session(&pool, "sess-a").await.unwrap();

        let active_after = list_sessions(&pool, None, None, true, 10).await.unwrap();
        assert_eq!(active_after.len(), 0);
    }

    // ------------------------------------------------------------------
    // Checkpoint save / restore service tests
    // ------------------------------------------------------------------

    #[tokio::test]
    async fn test_save_session_checkpoint_returns_correct_data() {
        let pool = open_test_db().await;

        start_session(
            &pool,
            NewSession {
                id: "sess-ckpt-1",
                agent: "sdd-builder",
                spec_id: None,
                task_id: None,
                host: None,
                notes: None,
            },
        )
        .await
        .unwrap();

        let data = serde_json::json!({"step": "planning", "progress": 42});
        let cp = save_session_checkpoint(
            &pool,
            "sess-ckpt-1",
            "sdd-builder",
            None,
            None,
            data.clone(),
            Some("my-label"),
        )
        .await
        .unwrap();

        assert_eq!(cp.session_id, "sess-ckpt-1");
        assert_eq!(cp.agent, "sdd-builder");
        assert_eq!(cp.label.as_deref(), Some("my-label"));
        assert_eq!(cp.checkpoint_data, data.to_string());
        assert!(cp.id.starts_with("ckpt-"));
    }

    #[tokio::test]
    async fn test_restore_session_checkpoint_by_id() {
        let pool = open_test_db().await;

        start_session(
            &pool,
            NewSession {
                id: "sess-ckpt-2",
                agent: "sdd-builder",
                spec_id: None,
                task_id: None,
                host: None,
                notes: None,
            },
        )
        .await
        .unwrap();

        let cp = save_session_checkpoint(
            &pool,
            "sess-ckpt-2",
            "sdd-builder",
            None,
            None,
            serde_json::json!({"x": 1}),
            Some("first"),
        )
        .await
        .unwrap();

        let restored = restore_session_checkpoint(&pool, "sess-ckpt-2", Some(&cp.id))
            .await
            .unwrap();

        assert_eq!(restored.id, cp.id);
        assert_eq!(restored.session_id, "sess-ckpt-2");
        assert_eq!(restored.label.as_deref(), Some("first"));
    }

    #[tokio::test]
    async fn test_restore_session_checkpoint_latest() {
        let pool = open_test_db().await;

        start_session(
            &pool,
            NewSession {
                id: "sess-ckpt-3",
                agent: "sdd-builder",
                spec_id: None,
                task_id: None,
                host: None,
                notes: None,
            },
        )
        .await
        .unwrap();

        save_session_checkpoint(
            &pool,
            "sess-ckpt-3",
            "sdd-builder",
            None,
            None,
            serde_json::json!({"v": 1}),
            Some("first"),
        )
        .await
        .unwrap();

        // Small sleep to ensure ordering by saved_at.
        tokio::time::sleep(std::time::Duration::from_millis(5)).await;

        let second = save_session_checkpoint(
            &pool,
            "sess-ckpt-3",
            "sdd-builder",
            None,
            None,
            serde_json::json!({"v": 2}),
            Some("second"),
        )
        .await
        .unwrap();

        // Restore with None should return the most recent checkpoint.
        let restored = restore_session_checkpoint(&pool, "sess-ckpt-3", None)
            .await
            .unwrap();

        assert_eq!(restored.id, second.id);
        assert_eq!(restored.label.as_deref(), Some("second"));
    }

    #[tokio::test]
    async fn test_restore_session_checkpoint_no_checkpoints_returns_error() {
        let pool = open_test_db().await;

        start_session(
            &pool,
            NewSession {
                id: "sess-ckpt-4",
                agent: "sdd-builder",
                spec_id: None,
                task_id: None,
                host: None,
                notes: None,
            },
        )
        .await
        .unwrap();

        let result = restore_session_checkpoint(&pool, "sess-ckpt-4", None).await;
        assert!(result.is_err(), "should error when no checkpoints exist");
        let msg = result.unwrap_err().to_string();
        assert!(msg.contains("No checkpoints found"), "error message: {msg}");
    }
}
