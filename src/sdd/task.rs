use anyhow::{anyhow, Result};
use chrono::Utc;
use serde::{Deserialize, Serialize};
use sqlx::SqlitePool;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Task {
    pub id: String,
    pub spec: String,
    pub title: String,
    pub agent: String,
    pub status: String,
    pub inputs: String, // JSON
    pub output_artifact: Option<String>,
    pub created_at: String,
    pub updated_at: String,
}

pub async fn create_task(
    pool: &SqlitePool,
    id: &str,
    spec: &str,
    title: &str,
    agent: &str,
    inputs: &[String],
    output_artifact: Option<&str>,
) -> Result<Task> {
    let now = Utc::now().to_rfc3339();
    let inputs_json = serde_json::to_string(inputs)?;

    sqlx::query(
        "INSERT INTO tasks (id, spec, title, agent, status, inputs, output_artifact, created_at, updated_at) \
         VALUES (?, ?, ?, ?, 'pending', ?, ?, ?, ?)",
    )
    .bind(id)
    .bind(spec)
    .bind(title)
    .bind(agent)
    .bind(&inputs_json)
    .bind(output_artifact)
    .bind(&now)
    .bind(&now)
    .execute(pool)
    .await?;

    get_task(pool, id)
        .await?
        .ok_or_else(|| anyhow!("Failed to create task '{}'", id))
}

pub async fn get_task(pool: &SqlitePool, id: &str) -> Result<Option<Task>> {
    let row = sqlx::query_as::<_, (String, String, String, String, String, String, Option<String>, String, String)>(
        "SELECT id, spec, title, agent, status, inputs, output_artifact, created_at, updated_at FROM tasks WHERE id = ?",
    )
    .bind(id)
    .fetch_optional(pool)
    .await?;

    Ok(row.map(
        |(id, spec, title, agent, status, inputs, output_artifact, created_at, updated_at)| Task {
            id,
            spec,
            title,
            agent,
            status,
            inputs,
            output_artifact,
            created_at,
            updated_at,
        },
    ))
}

pub async fn list_tasks(pool: &SqlitePool, spec_filter: Option<&str>) -> Result<Vec<Task>> {
    let rows = if let Some(spec) = spec_filter {
        sqlx::query_as::<_, (String, String, String, String, String, String, Option<String>, String, String)>(
            "SELECT id, spec, title, agent, status, inputs, output_artifact, created_at, updated_at \
             FROM tasks WHERE spec = ? ORDER BY id",
        )
        .bind(spec)
        .fetch_all(pool)
        .await?
    } else {
        sqlx::query_as::<_, (String, String, String, String, String, String, Option<String>, String, String)>(
            "SELECT id, spec, title, agent, status, inputs, output_artifact, created_at, updated_at \
             FROM tasks ORDER BY spec, id",
        )
        .fetch_all(pool)
        .await?
    };

    Ok(rows
        .into_iter()
        .map(
            |(id, spec, title, agent, status, inputs, output_artifact, created_at, updated_at)| {
                Task {
                    id,
                    spec,
                    title,
                    agent,
                    status,
                    inputs,
                    output_artifact,
                    created_at,
                    updated_at,
                }
            },
        )
        .collect())
}

fn validate_task_transition(from: &str, to: &str) -> Result<()> {
    let valid = matches!(
        (from, to),
        ("pending", "in_progress") | ("in_progress", "done") | ("in_progress", "failed")
    );
    if !valid {
        return Err(anyhow!("Invalid task transition: {} -> {}", from, to));
    }
    Ok(())
}

pub async fn update_task_status(pool: &SqlitePool, id: &str, new_status: &str) -> Result<Task> {
    let task = get_task(pool, id)
        .await?
        .ok_or_else(|| anyhow!("Task '{}' not found", id))?;

    validate_task_transition(&task.status, new_status)?;

    let now = Utc::now().to_rfc3339();
    sqlx::query("UPDATE tasks SET status = ?, updated_at = ? WHERE id = ?")
        .bind(new_status)
        .bind(&now)
        .bind(id)
        .execute(pool)
        .await?;

    get_task(pool, id)
        .await?
        .ok_or_else(|| anyhow!("Task '{}' not found after update", id))
}

pub async fn update_task_output_artifact(
    pool: &SqlitePool,
    id: &str,
    output_artifact: &str,
) -> Result<Task> {
    let now = Utc::now().to_rfc3339();
    sqlx::query("UPDATE tasks SET output_artifact = ?, updated_at = ? WHERE id = ?")
        .bind(output_artifact)
        .bind(&now)
        .bind(id)
        .execute(pool)
        .await?;

    get_task(pool, id)
        .await?
        .ok_or_else(|| anyhow!("Task '{}' not found", id))
}
