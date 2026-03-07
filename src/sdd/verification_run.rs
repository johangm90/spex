use anyhow::{anyhow, Result};
use chrono::Utc;
use serde::{Deserialize, Serialize};
use sqlx::SqlitePool;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct VerificationRun {
    pub id: String,
    pub spec_id: String,
    pub task_id: Option<String>,
    pub slice_id: Option<String>,
    pub kind: String,
    pub status: String,
    pub command: Option<String>,
    pub summary: String,
    pub evidence: Option<String>,
    pub created_at: String,
}

#[allow(clippy::too_many_arguments)]
pub async fn create_verification_run(
    pool: &SqlitePool,
    id: &str,
    spec_id: &str,
    task_id: Option<&str>,
    slice_id: Option<&str>,
    kind: &str,
    status: &str,
    command: Option<&str>,
    summary: &str,
    evidence: Option<&str>,
) -> Result<VerificationRun> {
    let now = Utc::now().to_rfc3339();
    sqlx::query(
        "INSERT INTO verification_runs (id, spec_id, task_id, slice_id, kind, status, command, summary, evidence, created_at)          VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?)"
    )
    .bind(id)
    .bind(spec_id)
    .bind(task_id)
    .bind(slice_id)
    .bind(kind)
    .bind(status)
    .bind(command)
    .bind(summary)
    .bind(evidence)
    .bind(&now)
    .execute(pool)
    .await?;

    get_verification_run(pool, id)
        .await?
        .ok_or_else(|| anyhow!("Failed to create verification run '{}'", id))
}

pub async fn get_verification_run(pool: &SqlitePool, id: &str) -> Result<Option<VerificationRun>> {
    let row = sqlx::query_as::<_, (String, String, Option<String>, Option<String>, String, String, Option<String>, String, Option<String>, String)>(
        "SELECT id, spec_id, task_id, slice_id, kind, status, command, summary, evidence, created_at FROM verification_runs WHERE id = ?"
    )
    .bind(id)
    .fetch_optional(pool)
    .await?;

    Ok(row.map(
        |(id, spec_id, task_id, slice_id, kind, status, command, summary, evidence, created_at)| {
            VerificationRun {
                id,
                spec_id,
                task_id,
                slice_id,
                kind,
                status,
                command,
                summary,
                evidence,
                created_at,
            }
        },
    ))
}

pub async fn list_verification_runs(
    pool: &SqlitePool,
    spec_filter: Option<&str>,
    task_filter: Option<&str>,
    status_filter: Option<&str>,
) -> Result<Vec<VerificationRun>> {
    let mut query = String::from(
        "SELECT id, spec_id, task_id, slice_id, kind, status, command, summary, evidence, created_at FROM verification_runs WHERE 1=1"
    );
    if spec_filter.is_some() {
        query.push_str(" AND spec_id = ?");
    }
    if task_filter.is_some() {
        query.push_str(" AND task_id = ?");
    }
    if status_filter.is_some() {
        query.push_str(" AND status = ?");
    }
    query.push_str(" ORDER BY created_at DESC, id");

    let mut q = sqlx::query_as::<
        _,
        (
            String,
            String,
            Option<String>,
            Option<String>,
            String,
            String,
            Option<String>,
            String,
            Option<String>,
            String,
        ),
    >(&query);
    if let Some(spec) = spec_filter {
        q = q.bind(spec);
    }
    if let Some(task) = task_filter {
        q = q.bind(task);
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
                slice_id,
                kind,
                status,
                command,
                summary,
                evidence,
                created_at,
            )| VerificationRun {
                id,
                spec_id,
                task_id,
                slice_id,
                kind,
                status,
                command,
                summary,
                evidence,
                created_at,
            },
        )
        .collect())
}
