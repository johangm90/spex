use anyhow::{anyhow, Result};
use chrono::{Duration, Utc};
use serde::{Deserialize, Serialize};
use sqlx::SqlitePool;

use crate::sdd::task::{get_task, update_task_status};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TaskLease {
    pub task_id: String,
    pub agent_id: String,
    pub status: String,
    pub lease_expires_at: String,
    pub heartbeat_at: String,
    pub attempt_count: i64,
    pub created_at: String,
    pub updated_at: String,
}

pub async fn get_task_lease(
    pool: &SqlitePool,
    project_dir: &str,
    task_id: &str,
) -> Result<Option<TaskLease>> {
    let row = sqlx::query_as::<_, (String, String, String, String, String, i64, String, String)>(
        "SELECT task_id, agent_id, status, lease_expires_at, heartbeat_at, attempt_count, created_at, updated_at FROM task_leases WHERE project_dir = ? AND task_id = ?",
    )
    .bind(project_dir)
    .bind(task_id)
    .fetch_optional(pool)
    .await?;
    Ok(row.map(
        |(
            task_id,
            agent_id,
            status,
            lease_expires_at,
            heartbeat_at,
            attempt_count,
            created_at,
            updated_at,
        )| TaskLease {
            task_id,
            agent_id,
            status,
            lease_expires_at,
            heartbeat_at,
            attempt_count,
            created_at,
            updated_at,
        },
    ))
}

pub async fn list_task_leases(
    pool: &SqlitePool,
    project_dir: &str,
    status_filter: Option<&str>,
) -> Result<Vec<TaskLease>> {
    let rows = if let Some(status) = status_filter {
        sqlx::query_as::<_, (String, String, String, String, String, i64, String, String)>(
            "SELECT task_id, agent_id, status, lease_expires_at, heartbeat_at, attempt_count, created_at, updated_at FROM task_leases WHERE project_dir = ? AND status = ? ORDER BY updated_at DESC",
        )
        .bind(project_dir)
        .bind(status)
        .fetch_all(pool)
        .await?
    } else {
        sqlx::query_as::<_, (String, String, String, String, String, i64, String, String)>(
            "SELECT task_id, agent_id, status, lease_expires_at, heartbeat_at, attempt_count, created_at, updated_at FROM task_leases WHERE project_dir = ? ORDER BY updated_at DESC",
        )
        .bind(project_dir)
        .fetch_all(pool)
        .await?
    };
    Ok(rows
        .into_iter()
        .map(
            |(
                task_id,
                agent_id,
                status,
                lease_expires_at,
                heartbeat_at,
                attempt_count,
                created_at,
                updated_at,
            )| TaskLease {
                task_id,
                agent_id,
                status,
                lease_expires_at,
                heartbeat_at,
                attempt_count,
                created_at,
                updated_at,
            },
        )
        .collect())
}

pub async fn claim_task_lease(
    pool: &SqlitePool,
    project_dir: &str,
    task_id: &str,
    agent_id: &str,
    ttl_seconds: i64,
) -> Result<TaskLease> {
    let task = get_task(pool, project_dir, task_id)
        .await?
        .ok_or_else(|| anyhow!("Task '{}' not found", task_id))?;
    if task.status != "ready" {
        return Err(anyhow!(
            "Task '{}' is not ready; current status is {}",
            task_id,
            task.status
        ));
    }
    let now = Utc::now();
    let lease_expires_at = (now + Duration::seconds(ttl_seconds)).to_rfc3339();
    let now_str = now.to_rfc3339();

    if let Some(existing) = get_task_lease(pool, project_dir, task_id).await? {
        if existing.status == "claimed" || existing.status == "running" {
            return Err(anyhow!("Task '{}' already has an active lease", task_id));
        }
        sqlx::query(
            "UPDATE task_leases SET agent_id = ?, status = 'claimed', lease_expires_at = ?, heartbeat_at = ?, attempt_count = attempt_count + 1, updated_at = ? WHERE project_dir = ? AND task_id = ?",
        )
        .bind(agent_id)
        .bind(&lease_expires_at)
        .bind(&now_str)
        .bind(&now_str)
        .bind(project_dir)
        .bind(task_id)
        .execute(pool)
        .await?;
    } else {
        sqlx::query(
            "INSERT INTO task_leases (project_dir, task_id, agent_id, status, lease_expires_at, heartbeat_at, attempt_count, created_at, updated_at) VALUES (?, ?, ?, 'claimed', ?, ?, 1, ?, ?)",
        )
        .bind(project_dir)
        .bind(task_id)
        .bind(agent_id)
        .bind(&lease_expires_at)
        .bind(&now_str)
        .bind(&now_str)
        .bind(&now_str)
        .execute(pool)
        .await?;
    }

    update_task_status(pool, project_dir, task_id, "claimed").await?;
    get_task_lease(pool, project_dir, task_id)
        .await?
        .ok_or_else(|| anyhow!("Task lease '{}' not found", task_id))
}

pub async fn heartbeat_task_lease(
    pool: &SqlitePool,
    project_dir: &str,
    task_id: &str,
    ttl_seconds: i64,
    progress_status: Option<&str>,
) -> Result<TaskLease> {
    let lease = get_task_lease(pool, project_dir, task_id)
        .await?
        .ok_or_else(|| anyhow!("Task lease '{}' not found", task_id))?;
    if lease.status != "claimed" && lease.status != "running" {
        return Err(anyhow!("Task lease '{}' is not active", task_id));
    }
    let now = Utc::now();
    let lease_expires_at = (now + Duration::seconds(ttl_seconds)).to_rfc3339();
    let now_str = now.to_rfc3339();
    let next_status = progress_status.unwrap_or("running");
    sqlx::query(
        "UPDATE task_leases SET status = 'running', lease_expires_at = ?, heartbeat_at = ?, updated_at = ? WHERE project_dir = ? AND task_id = ?",
    )
    .bind(&lease_expires_at)
    .bind(&now_str)
    .bind(&now_str)
    .bind(project_dir)
    .bind(task_id)
    .execute(pool)
    .await?;
    let task = get_task(pool, project_dir, task_id)
        .await?
        .ok_or_else(|| anyhow!("Task '{}' not found", task_id))?;
    if task.status == "claimed" && next_status == "running" {
        update_task_status(pool, project_dir, task_id, "running").await?;
    }
    get_task_lease(pool, project_dir, task_id)
        .await?
        .ok_or_else(|| anyhow!("Task lease '{}' not found", task_id))
}

pub async fn release_task_lease(
    pool: &SqlitePool,
    project_dir: &str,
    task_id: &str,
    final_status: Option<&str>,
) -> Result<TaskLease> {
    let now = Utc::now().to_rfc3339();
    sqlx::query(
        "UPDATE task_leases SET status = 'released', updated_at = ? WHERE project_dir = ? AND task_id = ?",
    )
    .bind(&now)
    .bind(project_dir)
    .bind(task_id)
    .execute(pool)
    .await?;
    if let Some(status) = final_status {
        let task = get_task(pool, project_dir, task_id)
            .await?
            .ok_or_else(|| anyhow!("Task '{}' not found", task_id))?;
        if task.status != status {
            update_task_status(pool, project_dir, task_id, status).await?;
        }
    }
    get_task_lease(pool, project_dir, task_id)
        .await?
        .ok_or_else(|| anyhow!("Task lease '{}' not found", task_id))
}

pub async fn expire_stale_task_leases(
    pool: &SqlitePool,
    project_dir: &str,
) -> Result<Vec<TaskLease>> {
    let now = Utc::now().to_rfc3339();
    let stale = sqlx::query_as::<_, (String, String, String, String, String, i64, String, String)>(
        "SELECT task_id, agent_id, status, lease_expires_at, heartbeat_at, attempt_count, created_at, updated_at FROM task_leases WHERE project_dir = ? AND (status = 'claimed' OR status = 'running') AND lease_expires_at < ?",
    )
    .bind(project_dir)
    .bind(&now)
    .fetch_all(pool)
    .await?;

    let mut expired = Vec::new();
    for (
        task_id,
        agent_id,
        status,
        lease_expires_at,
        heartbeat_at,
        attempt_count,
        created_at,
        _updated_at,
    ) in stale
    {
        sqlx::query(
            "UPDATE task_leases SET status = 'expired', updated_at = ? WHERE project_dir = ? AND task_id = ?",
        )
        .bind(&now)
        .bind(project_dir)
        .bind(&task_id)
        .execute(pool)
        .await?;
        let task = get_task(pool, project_dir, &task_id)
            .await?
            .ok_or_else(|| anyhow!("Task '{}' not found", task_id))?;
        if task.status == "claimed" || task.status == "running" {
            sqlx::query(
                "UPDATE tasks SET status = 'ready', updated_at = ? WHERE project_dir = ? AND id = ?",
            )
            .bind(&now)
            .bind(project_dir)
            .bind(&task_id)
            .execute(pool)
            .await?;
        }
        expired.push(TaskLease {
            task_id,
            agent_id,
            status,
            lease_expires_at,
            heartbeat_at,
            attempt_count,
            created_at,
            updated_at: now.clone(),
        });
    }
    Ok(expired)
}
