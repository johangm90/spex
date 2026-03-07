use anyhow::{anyhow, Result};
use chrono::Utc;
use serde::{Deserialize, Serialize};
use sqlx::SqlitePool;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Incident {
    pub id: String,
    pub spec_id: String,
    pub task_id: Option<String>,
    pub title: String,
    pub severity: String,
    pub status: String,
    pub source: String,
    pub blocking: bool,
    pub repro_steps: Option<String>,
    pub root_cause: Option<String>,
    pub fix_strategy: Option<String>,
    pub created_at: String,
    pub updated_at: String,
}

#[allow(clippy::too_many_arguments)]
pub async fn create_incident(
    pool: &SqlitePool,
    id: &str,
    spec_id: &str,
    task_id: Option<&str>,
    title: &str,
    severity: &str,
    source: &str,
    blocking: bool,
    repro_steps: Option<&str>,
) -> Result<Incident> {
    let now = Utc::now().to_rfc3339();
    sqlx::query(
        "INSERT INTO incidents (id, spec_id, task_id, title, severity, status, source, blocking, repro_steps, created_at, updated_at)          VALUES (?, ?, ?, ?, ?, 'new', ?, ?, ?, ?, ?)"
    )
    .bind(id)
    .bind(spec_id)
    .bind(task_id)
    .bind(title)
    .bind(severity)
    .bind(source)
    .bind(if blocking { 1 } else { 0 })
    .bind(repro_steps)
    .bind(&now)
    .bind(&now)
    .execute(pool)
    .await?;

    get_incident(pool, id)
        .await?
        .ok_or_else(|| anyhow!("Failed to create incident '{}'", id))
}

pub async fn get_incident(pool: &SqlitePool, id: &str) -> Result<Option<Incident>> {
    let row = sqlx::query_as::<_, (String, String, Option<String>, String, String, String, String, i64, Option<String>, Option<String>, Option<String>, String, String)>(
        "SELECT id, spec_id, task_id, title, severity, status, source, blocking, repro_steps, root_cause, fix_strategy, created_at, updated_at FROM incidents WHERE id = ?"
    )
    .bind(id)
    .fetch_optional(pool)
    .await?;

    Ok(row.map(
        |(
            id,
            spec_id,
            task_id,
            title,
            severity,
            status,
            source,
            blocking,
            repro_steps,
            root_cause,
            fix_strategy,
            created_at,
            updated_at,
        )| Incident {
            id,
            spec_id,
            task_id,
            title,
            severity,
            status,
            source,
            blocking: blocking != 0,
            repro_steps,
            root_cause,
            fix_strategy,
            created_at,
            updated_at,
        },
    ))
}

pub async fn list_incidents(
    pool: &SqlitePool,
    spec_filter: Option<&str>,
    status_filter: Option<&str>,
) -> Result<Vec<Incident>> {
    let mut query = String::from(
        "SELECT id, spec_id, task_id, title, severity, status, source, blocking, repro_steps, root_cause, fix_strategy, created_at, updated_at FROM incidents WHERE 1=1"
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
            String,
            i64,
            Option<String>,
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
                title,
                severity,
                status,
                source,
                blocking,
                repro_steps,
                root_cause,
                fix_strategy,
                created_at,
                updated_at,
            )| Incident {
                id,
                spec_id,
                task_id,
                title,
                severity,
                status,
                source,
                blocking: blocking != 0,
                repro_steps,
                root_cause,
                fix_strategy,
                created_at,
                updated_at,
            },
        )
        .collect())
}

pub async fn update_incident(
    pool: &SqlitePool,
    id: &str,
    status: Option<&str>,
    blocking: Option<bool>,
    root_cause: Option<&str>,
    fix_strategy: Option<&str>,
) -> Result<Incident> {
    let current = get_incident(pool, id)
        .await?
        .ok_or_else(|| anyhow!("Incident '{}' not found", id))?;
    let now = Utc::now().to_rfc3339();
    let status = status.unwrap_or(&current.status);
    let blocking = blocking.unwrap_or(current.blocking);
    let root_cause = root_cause.map(str::to_string).or(current.root_cause);
    let fix_strategy = fix_strategy.map(str::to_string).or(current.fix_strategy);

    sqlx::query(
        "UPDATE incidents SET status = ?, blocking = ?, root_cause = ?, fix_strategy = ?, updated_at = ? WHERE id = ?"
    )
    .bind(status)
    .bind(if blocking { 1 } else { 0 })
    .bind(root_cause)
    .bind(fix_strategy)
    .bind(&now)
    .bind(id)
    .execute(pool)
    .await?;

    get_incident(pool, id)
        .await?
        .ok_or_else(|| anyhow!("Incident '{}' not found after update", id))
}
