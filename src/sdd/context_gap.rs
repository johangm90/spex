use anyhow::{anyhow, Result};
use chrono::Utc;
use serde::{Deserialize, Serialize};
use sqlx::SqlitePool;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ContextGap {
    pub id: String,
    pub spec_id: String,
    pub task_id: Option<String>,
    pub kind: String,
    pub criticality: String,
    pub status: String,
    pub blocking: bool,
    pub question: String,
    pub assumption: Option<String>,
    pub resolution: Option<String>,
    pub created_at: String,
    pub updated_at: String,
}

#[allow(clippy::too_many_arguments)]
pub async fn create_context_gap(
    pool: &SqlitePool,
    id: &str,
    spec_id: &str,
    task_id: Option<&str>,
    kind: &str,
    criticality: &str,
    blocking: bool,
    question: &str,
    assumption: Option<&str>,
) -> Result<ContextGap> {
    let now = Utc::now().to_rfc3339();
    let status = if assumption.is_some() {
        "assumption_recorded"
    } else {
        "open"
    };
    sqlx::query(
        "INSERT INTO context_gaps (id, spec_id, task_id, kind, criticality, status, blocking, question, assumption, created_at, updated_at)          VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)"
    )
    .bind(id)
    .bind(spec_id)
    .bind(task_id)
    .bind(kind)
    .bind(criticality)
    .bind(status)
    .bind(if blocking { 1 } else { 0 })
    .bind(question)
    .bind(assumption)
    .bind(&now)
    .bind(&now)
    .execute(pool)
    .await?;

    get_context_gap(pool, id)
        .await?
        .ok_or_else(|| anyhow!("Failed to create context gap '{}'", id))
}

pub async fn get_context_gap(pool: &SqlitePool, id: &str) -> Result<Option<ContextGap>> {
    let row = sqlx::query_as::<_, (String, String, Option<String>, String, String, String, i64, String, Option<String>, Option<String>, String, String)>(
        "SELECT id, spec_id, task_id, kind, criticality, status, blocking, question, assumption, resolution, created_at, updated_at FROM context_gaps WHERE id = ?"
    )
    .bind(id)
    .fetch_optional(pool)
    .await?;

    Ok(row.map(
        |(
            id,
            spec_id,
            task_id,
            kind,
            criticality,
            status,
            blocking,
            question,
            assumption,
            resolution,
            created_at,
            updated_at,
        )| ContextGap {
            id,
            spec_id,
            task_id,
            kind,
            criticality,
            status,
            blocking: blocking != 0,
            question,
            assumption,
            resolution,
            created_at,
            updated_at,
        },
    ))
}

pub async fn list_context_gaps(
    pool: &SqlitePool,
    spec_filter: Option<&str>,
    status_filter: Option<&str>,
) -> Result<Vec<ContextGap>> {
    let mut query = String::from(
        "SELECT id, spec_id, task_id, kind, criticality, status, blocking, question, assumption, resolution, created_at, updated_at FROM context_gaps WHERE 1=1"
    );
    if spec_filter.is_some() {
        query.push_str(" AND spec_id = ?");
    }
    if status_filter.is_some() {
        query.push_str(" AND status = ?");
    }
    query.push_str(" ORDER BY updated_at DESC, id");

    let mut q = sqlx::query_as::<
        _,
        (
            String,
            String,
            Option<String>,
            String,
            String,
            String,
            i64,
            String,
            Option<String>,
            Option<String>,
            String,
            String,
        ),
    >(&query);
    if let Some(spec) = spec_filter {
        q = q.bind(spec);
    }
    if let Some(status) = status_filter {
        q = q.bind(status);
    }

    let rows = q.fetch_all(pool).await?;
    Ok(rows
        .into_iter()
        .map(
            |(
                id,
                spec_id,
                task_id,
                kind,
                criticality,
                status,
                blocking,
                question,
                assumption,
                resolution,
                created_at,
                updated_at,
            )| ContextGap {
                id,
                spec_id,
                task_id,
                kind,
                criticality,
                status,
                blocking: blocking != 0,
                question,
                assumption,
                resolution,
                created_at,
                updated_at,
            },
        )
        .collect())
}

pub async fn update_context_gap(
    pool: &SqlitePool,
    id: &str,
    status: Option<&str>,
    blocking: Option<bool>,
    assumption: Option<&str>,
    resolution: Option<&str>,
) -> Result<ContextGap> {
    let current = get_context_gap(pool, id)
        .await?
        .ok_or_else(|| anyhow!("Context gap '{}' not found", id))?;
    let now = Utc::now().to_rfc3339();
    let status = status.unwrap_or(&current.status);
    let blocking = blocking.unwrap_or(current.blocking);
    let assumption = assumption.map(str::to_string).or(current.assumption);
    let resolution = resolution.map(str::to_string).or(current.resolution);

    sqlx::query(
        "UPDATE context_gaps SET status = ?, blocking = ?, assumption = ?, resolution = ?, updated_at = ? WHERE id = ?"
    )
    .bind(status)
    .bind(if blocking { 1 } else { 0 })
    .bind(assumption)
    .bind(resolution)
    .bind(&now)
    .bind(id)
    .execute(pool)
    .await?;

    get_context_gap(pool, id)
        .await?
        .ok_or_else(|| anyhow!("Context gap '{}' not found after update", id))
}
