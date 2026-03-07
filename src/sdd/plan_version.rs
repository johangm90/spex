use anyhow::{anyhow, Result};
use chrono::Utc;
use serde::{Deserialize, Serialize};
use sqlx::SqlitePool;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PlanVersion {
    pub id: String,
    pub spec_id: String,
    pub version: i64,
    pub status: String,
    pub reason: Option<String>,
    pub plan_json: String,
    pub created_at: String,
}

pub async fn create_plan_version(
    pool: &SqlitePool,
    project_dir: &str,
    id: &str,
    spec_id: &str,
    version: i64,
    reason: Option<&str>,
    plan_json: &str,
) -> Result<PlanVersion> {
    let now = Utc::now().to_rfc3339();
    sqlx::query(
        "INSERT INTO plan_versions (id, project_dir, spec_id, version, status, reason, plan_json, created_at) \
         VALUES (?, ?, ?, ?, 'active', ?, ?, ?)",
    )
    .bind(id)
    .bind(project_dir)
    .bind(spec_id)
    .bind(version)
    .bind(reason)
    .bind(plan_json)
    .bind(&now)
    .execute(pool)
    .await?;
    get_plan_version(pool, project_dir, id)
        .await?
        .ok_or_else(|| anyhow!("Plan version '{}' not found", id))
}

pub async fn get_plan_version(
    pool: &SqlitePool,
    project_dir: &str,
    id: &str,
) -> Result<Option<PlanVersion>> {
    let row = sqlx::query_as::<_, (String, String, i64, String, Option<String>, String, String)>(
        "SELECT id, spec_id, version, status, reason, plan_json, created_at FROM plan_versions WHERE project_dir = ? AND id = ?",
    )
    .bind(project_dir)
    .bind(id)
    .fetch_optional(pool)
    .await?;
    Ok(row.map(
        |(id, spec_id, version, status, reason, plan_json, created_at)| PlanVersion {
            id,
            spec_id,
            version,
            status,
            reason,
            plan_json,
            created_at,
        },
    ))
}

pub async fn list_plan_versions(
    pool: &SqlitePool,
    project_dir: &str,
    spec_filter: Option<&str>,
) -> Result<Vec<PlanVersion>> {
    let rows = if let Some(spec) = spec_filter {
        sqlx::query_as::<_, (String, String, i64, String, Option<String>, String, String)>(
            "SELECT id, spec_id, version, status, reason, plan_json, created_at FROM plan_versions WHERE project_dir = ? AND spec_id = ? ORDER BY version DESC",
        )
        .bind(project_dir)
        .bind(spec)
        .fetch_all(pool)
        .await?
    } else {
        sqlx::query_as::<_, (String, String, i64, String, Option<String>, String, String)>(
            "SELECT id, spec_id, version, status, reason, plan_json, created_at FROM plan_versions WHERE project_dir = ? ORDER BY spec_id, version DESC",
        )
        .bind(project_dir)
        .fetch_all(pool)
        .await?
    };
    Ok(rows
        .into_iter()
        .map(
            |(id, spec_id, version, status, reason, plan_json, created_at)| PlanVersion {
                id,
                spec_id,
                version,
                status,
                reason,
                plan_json,
                created_at,
            },
        )
        .collect())
}

pub async fn get_active_plan_version(
    pool: &SqlitePool,
    project_dir: &str,
    spec_id: &str,
) -> Result<Option<PlanVersion>> {
    let row = sqlx::query_as::<_, (String, String, i64, String, Option<String>, String, String)>(
        "SELECT id, spec_id, version, status, reason, plan_json, created_at FROM plan_versions WHERE project_dir = ? AND spec_id = ? AND status = 'active' ORDER BY version DESC LIMIT 1",
    )
    .bind(project_dir)
    .bind(spec_id)
    .fetch_optional(pool)
    .await?;
    Ok(row.map(
        |(id, spec_id, version, status, reason, plan_json, created_at)| PlanVersion {
            id,
            spec_id,
            version,
            status,
            reason,
            plan_json,
            created_at,
        },
    ))
}

pub async fn supersede_plan_versions(
    pool: &SqlitePool,
    project_dir: &str,
    spec_id: &str,
) -> Result<()> {
    sqlx::query(
        "UPDATE plan_versions SET status = 'superseded' WHERE project_dir = ? AND spec_id = ? AND status = 'active'",
    )
    .bind(project_dir)
    .bind(spec_id)
    .execute(pool)
    .await?;
    Ok(())
}
