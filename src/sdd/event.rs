use anyhow::Result;
use chrono::Utc;
use serde::{Deserialize, Serialize};
use sqlx::SqlitePool;

#[derive(Debug, Clone, Serialize, Deserialize)]
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

pub async fn query_events(
    pool: &SqlitePool,
    type_filter: Option<&str>,
    spec_filter: Option<&str>,
    agent_filter: Option<&str>,
    limit: Option<i64>,
    since: Option<&str>,
    until: Option<&str>,
) -> Result<Vec<Event>> {
    let mut query_str =
        String::from("SELECT id, type, spec, agent, payload, timestamp FROM events WHERE 1=1");

    if type_filter.is_some() {
        query_str.push_str(" AND type = ?");
    }
    if spec_filter.is_some() {
        query_str.push_str(" AND spec = ?");
    }
    if agent_filter.is_some() {
        query_str.push_str(" AND agent = ?");
    }
    if since.is_some() {
        query_str.push_str(" AND timestamp >= ?");
    }
    if until.is_some() {
        query_str.push_str(" AND timestamp <= ?");
    }

    query_str.push_str(" ORDER BY timestamp DESC");

    if let Some(lim) = limit {
        query_str.push_str(&format!(" LIMIT {}", lim));
    }

    let mut q = sqlx::query_as::<_, (i64, String, Option<String>, Option<String>, String, String)>(
        &query_str,
    );

    if let Some(t) = type_filter {
        q = q.bind(t);
    }
    if let Some(s) = spec_filter {
        q = q.bind(s);
    }
    if let Some(a) = agent_filter {
        q = q.bind(a);
    }
    if let Some(si) = since {
        q = q.bind(si);
    }
    if let Some(u) = until {
        q = q.bind(u);
    }

    let rows = q.fetch_all(pool).await?;

    Ok(rows
        .into_iter()
        .map(|(id, r#type, spec, agent, payload, timestamp)| Event {
            id,
            r#type,
            spec,
            agent,
            payload,
            timestamp,
        })
        .collect())
}
