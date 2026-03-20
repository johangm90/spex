use anyhow::Result;
use chrono::Utc;
use serde::{Deserialize, Serialize};
use sqlx::SqlitePool;

#[derive(Debug, Clone, Serialize, Deserialize, sqlx::FromRow)]
pub struct Event {
    pub id: i64,
    pub r#type: String,
    pub spec: Option<String>,
    pub agent: Option<String>,
    pub payload: String, // JSON
    pub timestamp: String,
}

pub async fn emit_event(
    pool: &SqlitePool,
    event_type: &str,
    spec: Option<&str>,
    agent: Option<&str>,
    payload_json: &str,
) -> Result<()> {
    let now = Utc::now().to_rfc3339();
    sqlx::query(
        "INSERT INTO events (type, spec, agent, payload, timestamp) VALUES (?, ?, ?, ?, ?)",
    )
    .bind(event_type)
    .bind(spec)
    .bind(agent)
    .bind(payload_json)
    .bind(&now)
    .execute(pool)
    .await?;
    Ok(())
}

#[allow(clippy::too_many_arguments)]
pub async fn query_events(
    pool: &SqlitePool,
    type_filter: Option<&str>,
    spec_filter: Option<&str>,
    agent_filter: Option<&str>,
    limit: Option<i64>,
    since: Option<&str>,
    until: Option<&str>,
    offset: Option<i64>,
) -> Result<Vec<Event>> {
    let mut qb = sqlx::QueryBuilder::<sqlx::Sqlite>::new(
        "SELECT id, type, spec, agent, payload, timestamp FROM events WHERE 1=1",
    );

    if let Some(t) = type_filter {
        qb.push(" AND type = ");
        qb.push_bind(t);
    }
    if let Some(s) = spec_filter {
        qb.push(" AND spec = ");
        qb.push_bind(s);
    }
    if let Some(a) = agent_filter {
        qb.push(" AND agent = ");
        qb.push_bind(a);
    }
    if let Some(si) = since {
        qb.push(" AND timestamp >= ");
        qb.push_bind(si);
    }
    if let Some(u) = until {
        qb.push(" AND timestamp <= ");
        qb.push_bind(u);
    }

    qb.push(" ORDER BY timestamp DESC");

    if let Some(lim) = limit {
        qb.push(" LIMIT ");
        qb.push_bind(lim);
        if let Some(off) = offset {
            qb.push(" OFFSET ");
            qb.push_bind(off);
        }
    }

    let events: Vec<Event> = qb.build_query_as().fetch_all(pool).await?;
    Ok(events)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::sdd::test_helpers::make_pool;

    #[tokio::test]
    async fn test_emit_and_query_event_happy_path() {
        let pool = make_pool().await;
        emit_event(
            &pool,
            "task.created",
            Some("SPEC-001"),
            Some("agent-x"),
            r#"{"key":"val"}"#,
        )
        .await
        .unwrap();
        let events = query_events(&pool, None, None, None, None, None, None, None)
            .await
            .unwrap();
        assert_eq!(events.len(), 1);
        let e = &events[0];
        assert_eq!(e.r#type, "task.created");
        assert_eq!(e.spec.as_deref(), Some("SPEC-001"));
        assert_eq!(e.agent.as_deref(), Some("agent-x"));
        assert_eq!(e.payload, r#"{"key":"val"}"#);
        assert!(!e.timestamp.is_empty());
    }

    #[tokio::test]
    async fn test_query_events_type_filter() {
        let pool = make_pool().await;
        emit_event(&pool, "spec.created", None, None, "{}")
            .await
            .unwrap();
        emit_event(&pool, "task.done", None, None, "{}")
            .await
            .unwrap();
        let events = query_events(
            &pool,
            Some("spec.created"),
            None,
            None,
            None,
            None,
            None,
            None,
        )
        .await
        .unwrap();
        assert_eq!(events.len(), 1);
        assert_eq!(events[0].r#type, "spec.created");
    }

    #[tokio::test]
    async fn test_query_events_spec_filter() {
        let pool = make_pool().await;
        emit_event(&pool, "ev", Some("SPEC-A"), None, "{}")
            .await
            .unwrap();
        emit_event(&pool, "ev", Some("SPEC-B"), None, "{}")
            .await
            .unwrap();
        let events = query_events(&pool, None, Some("SPEC-A"), None, None, None, None, None)
            .await
            .unwrap();
        assert_eq!(events.len(), 1);
        assert_eq!(events[0].spec.as_deref(), Some("SPEC-A"));
    }

    #[tokio::test]
    async fn test_query_events_limit() {
        let pool = make_pool().await;
        for i in 0..5 {
            emit_event(&pool, "ev", None, None, &format!(r#"{{"i":{}}}"#, i))
                .await
                .unwrap();
        }
        let events = query_events(&pool, None, None, None, Some(2), None, None, None)
            .await
            .unwrap();
        assert_eq!(events.len(), 2);
    }

    #[tokio::test]
    async fn test_query_events_empty_db() {
        let pool = make_pool().await;
        let events = query_events(&pool, None, None, None, None, None, None, None)
            .await
            .unwrap();
        assert!(events.is_empty());
    }
}
