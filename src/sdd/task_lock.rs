use anyhow::{anyhow, Result};
use chrono::Utc;
use serde::{Deserialize, Serialize};
use sqlx::SqlitePool;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TaskLock {
    pub id: String,
    pub task_id: String,
    pub spec_id: String,
    pub lock_type: String,
    pub resource: String,
    pub status: String,
    pub acquired_at: String,
    pub released_at: Option<String>,
}

pub async fn query_task_locks(
    pool: &SqlitePool,
    spec: Option<&str>,
    task: Option<&str>,
    active_only: bool,
) -> Result<Vec<TaskLock>> {
    let mut query = String::from("SELECT id, task_id, spec_id, lock_type, resource, status, acquired_at, released_at FROM task_locks WHERE 1=1");
    if spec.is_some() {
        query.push_str(" AND spec_id = ?");
    }
    if task.is_some() {
        query.push_str(" AND task_id = ?");
    }
    if active_only {
        query.push_str(" AND status = 'active'");
    }
    query.push_str(" ORDER BY acquired_at DESC");
    let mut q = sqlx::query_as::<
        _,
        (
            String,
            String,
            String,
            String,
            String,
            String,
            String,
            Option<String>,
        ),
    >(&query);
    if let Some(spec) = spec {
        q = q.bind(spec);
    }
    if let Some(task) = task {
        q = q.bind(task);
    }
    let rows = q.fetch_all(pool).await?;
    Ok(rows
        .into_iter()
        .map(
            |(id, task_id, spec_id, lock_type, resource, status, acquired_at, released_at)| {
                TaskLock {
                    id,
                    task_id,
                    spec_id,
                    lock_type,
                    resource,
                    status,
                    acquired_at,
                    released_at,
                }
            },
        )
        .collect())
}

pub async fn acquire_task_locks(
    pool: &SqlitePool,
    task_id: &str,
    spec_id: &str,
    locks: &[(String, String)],
) -> Result<Vec<TaskLock>> {
    let now = Utc::now().to_rfc3339();
    let mut created = Vec::new();
    for (lock_type, resource) in locks {
        let conflict = sqlx::query_as::<_, (String, String, String)>(
            "SELECT id, task_id, resource FROM task_locks WHERE spec_id = ? AND lock_type = ? AND resource = ? AND status = 'active' AND task_id != ? LIMIT 1"
        )
        .bind(spec_id)
        .bind(lock_type)
        .bind(resource)
        .bind(task_id)
        .fetch_optional(pool)
        .await?;
        if let Some((_, other_task, resource)) = conflict {
            return Err(anyhow!(
                "Lock conflict for resource '{}' with task '{}'",
                resource,
                other_task
            ));
        }
        let id = format!("LOCK-{}-{}-{}", task_id, lock_type, created.len() + 1);
        sqlx::query("INSERT INTO task_locks (id, task_id, spec_id, lock_type, resource, status, acquired_at) VALUES (?, ?, ?, ?, ?, 'active', ?)")
            .bind(&id)
            .bind(task_id)
            .bind(spec_id)
            .bind(lock_type)
            .bind(resource)
            .bind(&now)
            .execute(pool)
            .await?;
        created.push(TaskLock {
            id,
            task_id: task_id.to_string(),
            spec_id: spec_id.to_string(),
            lock_type: lock_type.clone(),
            resource: resource.clone(),
            status: "active".to_string(),
            acquired_at: now.clone(),
            released_at: None,
        });
    }
    Ok(created)
}

pub async fn release_task_locks(pool: &SqlitePool, task_id: &str) -> Result<Vec<TaskLock>> {
    let now = Utc::now().to_rfc3339();
    sqlx::query("UPDATE task_locks SET status = 'released', released_at = ? WHERE task_id = ? AND status = 'active'")
        .bind(&now)
        .bind(task_id)
        .execute(pool)
        .await?;
    query_task_locks(pool, None, Some(task_id), false).await
}
