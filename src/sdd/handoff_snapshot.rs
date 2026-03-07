use anyhow::{anyhow, Result};
use chrono::Utc;
use serde::{Deserialize, Serialize};
use sqlx::SqlitePool;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HandoffSnapshot {
    pub id: String,
    pub spec_id: String,
    pub interrupt_id: Option<String>,
    pub last_wave: Option<i64>,
    pub last_task: Option<String>,
    pub files_touched: String,
    pub decisions: String,
    pub open_risks: String,
    pub next_steps: String,
    pub created_at: String,
}

#[allow(clippy::too_many_arguments)]
pub async fn create_handoff_snapshot(
    pool: &SqlitePool,
    project_dir: &str,
    id: &str,
    spec_id: &str,
    interrupt_id: Option<&str>,
    last_wave: Option<i64>,
    last_task: Option<&str>,
    files_touched: &[String],
    decisions: &[String],
    open_risks: &[String],
    next_steps: &[String],
) -> Result<HandoffSnapshot> {
    let now = Utc::now().to_rfc3339();
    let files_json = serde_json::to_string(files_touched)?;
    let decisions_json = serde_json::to_string(decisions)?;
    let risks_json = serde_json::to_string(open_risks)?;
    let next_json = serde_json::to_string(next_steps)?;

    sqlx::query(
        "INSERT INTO handoff_snapshots (id, project_dir, spec_id, interrupt_id, last_wave, last_task, files_touched, decisions, open_risks, next_steps, created_at) \
         VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)"
    )
    .bind(id)
    .bind(project_dir)
    .bind(spec_id)
    .bind(interrupt_id)
    .bind(last_wave)
    .bind(last_task)
    .bind(&files_json)
    .bind(&decisions_json)
    .bind(&risks_json)
    .bind(&next_json)
    .bind(&now)
    .execute(pool)
    .await?;

    get_handoff_snapshot(pool, project_dir, id)
        .await?
        .ok_or_else(|| anyhow!("Failed to create handoff snapshot '{}'", id))
}

pub async fn get_handoff_snapshot(
    pool: &SqlitePool,
    project_dir: &str,
    id: &str,
) -> Result<Option<HandoffSnapshot>> {
    let row = sqlx::query_as::<_, (String, String, Option<String>, Option<i64>, Option<String>, String, String, String, String, String)>(
        "SELECT id, spec_id, interrupt_id, last_wave, last_task, files_touched, decisions, open_risks, next_steps, created_at FROM handoff_snapshots WHERE project_dir = ? AND id = ?"
    )
    .bind(project_dir)
    .bind(id)
    .fetch_optional(pool)
    .await?;

    Ok(row.map(
        |(
            id,
            spec_id,
            interrupt_id,
            last_wave,
            last_task,
            files_touched,
            decisions,
            open_risks,
            next_steps,
            created_at,
        )| HandoffSnapshot {
            id,
            spec_id,
            interrupt_id,
            last_wave,
            last_task,
            files_touched,
            decisions,
            open_risks,
            next_steps,
            created_at,
        },
    ))
}

pub async fn list_handoff_snapshots(
    pool: &SqlitePool,
    project_dir: &str,
    spec_filter: Option<&str>,
) -> Result<Vec<HandoffSnapshot>> {
    let rows = if let Some(spec) = spec_filter {
        sqlx::query_as::<_, (String, String, Option<String>, Option<i64>, Option<String>, String, String, String, String, String)>(
            "SELECT id, spec_id, interrupt_id, last_wave, last_task, files_touched, decisions, open_risks, next_steps, created_at FROM handoff_snapshots WHERE project_dir = ? AND spec_id = ? ORDER BY created_at DESC, id"
        )
        .bind(project_dir)
        .bind(spec)
        .fetch_all(pool)
        .await?
    } else {
        sqlx::query_as::<_, (String, String, Option<String>, Option<i64>, Option<String>, String, String, String, String, String)>(
            "SELECT id, spec_id, interrupt_id, last_wave, last_task, files_touched, decisions, open_risks, next_steps, created_at FROM handoff_snapshots WHERE project_dir = ? ORDER BY created_at DESC, id"
        )
        .bind(project_dir)
        .fetch_all(pool)
        .await?
    };

    Ok(rows
        .into_iter()
        .map(
            |(
                id,
                spec_id,
                interrupt_id,
                last_wave,
                last_task,
                files_touched,
                decisions,
                open_risks,
                next_steps,
                created_at,
            )| HandoffSnapshot {
                id,
                spec_id,
                interrupt_id,
                last_wave,
                last_task,
                files_touched,
                decisions,
                open_risks,
                next_steps,
                created_at,
            },
        )
        .collect())
}
