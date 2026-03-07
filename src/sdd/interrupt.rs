use anyhow::{anyhow, Result};
use chrono::Utc;
use serde::{Deserialize, Serialize};
use sqlx::SqlitePool;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Interrupt {
    pub id: String,
    pub spec_id: String,
    pub reason_type: String,
    pub status: String,
    pub preempted_tasks: String,
    pub resume_hint: Option<String>,
    pub created_at: String,
    pub updated_at: String,
}

pub async fn create_interrupt(
    pool: &SqlitePool,
    project_dir: &str,
    id: &str,
    spec_id: &str,
    reason_type: &str,
    preempted_tasks: &[String],
    resume_hint: Option<&str>,
) -> Result<Interrupt> {
    let now = Utc::now().to_rfc3339();
    let tasks_json = serde_json::to_string(preempted_tasks)?;
    sqlx::query(
        "INSERT INTO interrupts (id, project_dir, spec_id, reason_type, status, preempted_tasks, resume_hint, created_at, updated_at) \
         VALUES (?, ?, ?, ?, 'open', ?, ?, ?, ?)"
    )
    .bind(id)
    .bind(project_dir)
    .bind(spec_id)
    .bind(reason_type)
    .bind(&tasks_json)
    .bind(resume_hint)
    .bind(&now)
    .bind(&now)
    .execute(pool)
    .await?;

    get_interrupt(pool, project_dir, id)
        .await?
        .ok_or_else(|| anyhow!("Failed to create interrupt '{}'", id))
}

pub async fn get_interrupt(
    pool: &SqlitePool,
    project_dir: &str,
    id: &str,
) -> Result<Option<Interrupt>> {
    let row = sqlx::query_as::<_, (String, String, String, String, String, Option<String>, String, String)>(
        "SELECT id, spec_id, reason_type, status, preempted_tasks, resume_hint, created_at, updated_at FROM interrupts WHERE project_dir = ? AND id = ?"
    )
    .bind(project_dir)
    .bind(id)
    .fetch_optional(pool)
    .await?;

    Ok(row.map(
        |(
            id,
            spec_id,
            reason_type,
            status,
            preempted_tasks,
            resume_hint,
            created_at,
            updated_at,
        )| Interrupt {
            id,
            spec_id,
            reason_type,
            status,
            preempted_tasks,
            resume_hint,
            created_at,
            updated_at,
        },
    ))
}

pub async fn list_interrupts(
    pool: &SqlitePool,
    project_dir: &str,
    spec_filter: Option<&str>,
    status_filter: Option<&str>,
) -> Result<Vec<Interrupt>> {
    let mut query = String::from(
        "SELECT id, spec_id, reason_type, status, preempted_tasks, resume_hint, created_at, updated_at FROM interrupts WHERE project_dir = ?"
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
            String,
            String,
            String,
            Option<String>,
            String,
            String,
        ),
    >(&query);
    q = q.bind(project_dir);
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
                reason_type,
                status,
                preempted_tasks,
                resume_hint,
                created_at,
                updated_at,
            )| Interrupt {
                id,
                spec_id,
                reason_type,
                status,
                preempted_tasks,
                resume_hint,
                created_at,
                updated_at,
            },
        )
        .collect())
}

pub async fn update_interrupt(
    pool: &SqlitePool,
    project_dir: &str,
    id: &str,
    status: Option<&str>,
    resume_hint: Option<&str>,
) -> Result<Interrupt> {
    let current = get_interrupt(pool, project_dir, id)
        .await?
        .ok_or_else(|| anyhow!("Interrupt '{}' not found", id))?;
    let now = Utc::now().to_rfc3339();
    let status = status.unwrap_or(&current.status);
    let resume_hint = resume_hint.map(str::to_string).or(current.resume_hint);

    sqlx::query(
        "UPDATE interrupts SET status = ?, resume_hint = ?, updated_at = ? WHERE project_dir = ? AND id = ?",
    )
    .bind(status)
    .bind(resume_hint)
    .bind(&now)
    .bind(project_dir)
    .bind(id)
    .execute(pool)
    .await?;

    get_interrupt(pool, project_dir, id)
        .await?
        .ok_or_else(|| anyhow!("Interrupt '{}' not found after update", id))
}
