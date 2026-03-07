use anyhow::{anyhow, Result};
use chrono::Utc;
use serde::{Deserialize, Serialize};
use sqlx::{FromRow, SqlitePool};

#[derive(Debug, Clone, Serialize, Deserialize, FromRow)]
pub struct Task {
    pub id: String,
    pub spec: String,
    pub title: String,
    pub agent: String,
    pub status: String,
    pub inputs: String,
    pub depends_on: String,
    pub conflicts_with: String,
    pub lock_set: String,
    pub lock_requirements: String,
    pub priority: i64,
    pub risk_level: String,
    pub execution_bucket: String,
    pub estimate_points: i64,
    pub unblock_value: i64,
    pub plan_version: Option<String>,
    pub output_artifact: Option<String>,
    pub created_at: String,
    pub updated_at: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct TaskLockRequirement {
    pub lock_type: String,
    pub resource: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TaskRuntimeMetadata {
    pub depends_on: Vec<String>,
    pub conflicts_with: Vec<String>,
    pub lock_set: Vec<String>,
    pub lock_requirements: Vec<TaskLockRequirement>,
    pub priority: i64,
    pub risk_level: String,
    pub execution_bucket: String,
    pub estimate_points: i64,
    pub unblock_value: i64,
    pub plan_version: Option<String>,
}

fn normalize_lock_requirements(
    lock_set: &[String],
    lock_requirements: &[TaskLockRequirement],
) -> Vec<TaskLockRequirement> {
    if !lock_requirements.is_empty() {
        return lock_requirements.to_vec();
    }
    lock_set
        .iter()
        .filter_map(|entry| {
            let (lock_type, resource) = entry.split_once(':')?;
            Some(TaskLockRequirement {
                lock_type: lock_type.to_string(),
                resource: resource.to_string(),
            })
        })
        .collect()
}

#[allow(clippy::too_many_arguments)]
pub async fn create_task(
    pool: &SqlitePool,
    id: &str,
    spec: &str,
    title: &str,
    agent: &str,
    inputs: &[String],
    depends_on: &[String],
    conflicts_with: &[String],
    lock_set: &[String],
    lock_requirements: &[TaskLockRequirement],
    priority: i64,
    risk_level: &str,
    execution_bucket: &str,
    estimate_points: i64,
    unblock_value: i64,
    plan_version: Option<&str>,
    output_artifact: Option<&str>,
) -> Result<Task> {
    let now = Utc::now().to_rfc3339();
    let inputs_json = serde_json::to_string(inputs)?;
    let depends_json = serde_json::to_string(depends_on)?;
    let conflicts_json = serde_json::to_string(conflicts_with)?;
    let lock_set_json = serde_json::to_string(lock_set)?;
    let normalized_locks = normalize_lock_requirements(lock_set, lock_requirements);
    let lock_requirements_json = serde_json::to_string(&normalized_locks)?;

    sqlx::query(
        "INSERT INTO tasks (id, spec, title, agent, status, inputs, depends_on, conflicts_with, lock_set, lock_requirements, priority, risk_level, execution_bucket, estimate_points, unblock_value, plan_version, output_artifact, created_at, updated_at) \
         VALUES (?, ?, ?, ?, 'ready', ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)"
    )
    .bind(id)
    .bind(spec)
    .bind(title)
    .bind(agent)
    .bind(&inputs_json)
    .bind(&depends_json)
    .bind(&conflicts_json)
    .bind(&lock_set_json)
    .bind(&lock_requirements_json)
    .bind(priority)
    .bind(risk_level)
    .bind(execution_bucket)
    .bind(estimate_points)
    .bind(unblock_value)
    .bind(plan_version)
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
    let row = sqlx::query_as::<_, Task>(
        "SELECT id, spec, title, agent, status, inputs, depends_on, conflicts_with, lock_set, lock_requirements, priority, risk_level, execution_bucket, estimate_points, unblock_value, plan_version, output_artifact, created_at, updated_at FROM tasks WHERE id = ?"
    )
    .bind(id)
    .fetch_optional(pool)
    .await?;
    Ok(row)
}

pub async fn list_tasks(pool: &SqlitePool, spec_filter: Option<&str>) -> Result<Vec<Task>> {
    let rows = if let Some(spec) = spec_filter {
        sqlx::query_as::<_, Task>(
            "SELECT id, spec, title, agent, status, inputs, depends_on, conflicts_with, lock_set, lock_requirements, priority, risk_level, execution_bucket, estimate_points, unblock_value, plan_version, output_artifact, created_at, updated_at FROM tasks WHERE spec = ? ORDER BY priority ASC, unblock_value DESC, estimate_points ASC, id"
        )
        .bind(spec)
        .fetch_all(pool)
        .await?
    } else {
        sqlx::query_as::<_, Task>(
            "SELECT id, spec, title, agent, status, inputs, depends_on, conflicts_with, lock_set, lock_requirements, priority, risk_level, execution_bucket, estimate_points, unblock_value, plan_version, output_artifact, created_at, updated_at FROM tasks ORDER BY spec, priority ASC, unblock_value DESC, estimate_points ASC, id"
        )
        .fetch_all(pool)
        .await?
    };
    Ok(rows)
}

pub fn task_runtime_metadata(task: &Task) -> TaskRuntimeMetadata {
    let lock_set: Vec<String> = serde_json::from_str(&task.lock_set).unwrap_or_default();
    let lock_requirements: Vec<TaskLockRequirement> = serde_json::from_str(&task.lock_requirements)
        .unwrap_or_else(|_| normalize_lock_requirements(&lock_set, &[]));
    TaskRuntimeMetadata {
        depends_on: serde_json::from_str(&task.depends_on).unwrap_or_default(),
        conflicts_with: serde_json::from_str(&task.conflicts_with).unwrap_or_default(),
        lock_set,
        lock_requirements,
        priority: task.priority,
        risk_level: task.risk_level.clone(),
        execution_bucket: task.execution_bucket.clone(),
        estimate_points: task.estimate_points,
        unblock_value: task.unblock_value,
        plan_version: task.plan_version.clone(),
    }
}

fn validate_task_transition(from: &str, to: &str) -> Result<()> {
    let valid = matches!(
        (from, to),
        ("ready", "claimed")
            | ("ready", "blocked")
            | ("ready", "cancelled")
            | ("claimed", "running")
            | ("claimed", "blocked")
            | ("claimed", "cancelled")
            | ("running", "awaiting_review")
            | ("running", "blocked")
            | ("running", "failed")
            | ("running", "cancelled")
            | ("awaiting_review", "verifying")
            | ("awaiting_review", "running")
            | ("awaiting_review", "blocked")
            | ("awaiting_review", "cancelled")
            | ("verifying", "done")
            | ("verifying", "running")
            | ("verifying", "blocked")
            | ("verifying", "failed")
            | ("verifying", "cancelled")
            | ("blocked", "ready")
            | ("blocked", "cancelled")
            | ("failed", "ready")
            | ("failed", "cancelled")
            | ("done", "superseded")
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

#[allow(clippy::too_many_arguments)]
pub async fn update_task_metadata(
    pool: &SqlitePool,
    id: &str,
    depends_on: Option<&[String]>,
    conflicts_with: Option<&[String]>,
    lock_set: Option<&[String]>,
    lock_requirements: Option<&[TaskLockRequirement]>,
    priority: Option<i64>,
    risk_level: Option<&str>,
    execution_bucket: Option<&str>,
    estimate_points: Option<i64>,
    unblock_value: Option<i64>,
    plan_version: Option<Option<&str>>,
) -> Result<Task> {
    let task = get_task(pool, id)
        .await?
        .ok_or_else(|| anyhow!("Task '{}' not found", id))?;
    let current = task_runtime_metadata(&task);
    let depends_json = serde_json::to_string(depends_on.unwrap_or(&current.depends_on))?;
    let conflicts_json = serde_json::to_string(conflicts_with.unwrap_or(&current.conflicts_with))?;
    let next_lock_set = lock_set.unwrap_or(&current.lock_set);
    let lock_set_json = serde_json::to_string(next_lock_set)?;
    let normalized_locks = normalize_lock_requirements(
        next_lock_set,
        lock_requirements.unwrap_or(&current.lock_requirements),
    );
    let lock_requirements_json = serde_json::to_string(&normalized_locks)?;
    let now = Utc::now().to_rfc3339();
    sqlx::query(
        "UPDATE tasks SET depends_on = ?, conflicts_with = ?, lock_set = ?, lock_requirements = ?, priority = ?, risk_level = ?, execution_bucket = ?, estimate_points = ?, unblock_value = ?, plan_version = ?, updated_at = ? WHERE id = ?"
    )
    .bind(&depends_json)
    .bind(&conflicts_json)
    .bind(&lock_set_json)
    .bind(&lock_requirements_json)
    .bind(priority.unwrap_or(current.priority))
    .bind(risk_level.unwrap_or(&current.risk_level))
    .bind(execution_bucket.unwrap_or(&current.execution_bucket))
    .bind(estimate_points.unwrap_or(current.estimate_points))
    .bind(unblock_value.unwrap_or(current.unblock_value))
    .bind(plan_version.unwrap_or(task.plan_version.as_deref()))
    .bind(&now)
    .bind(id)
    .execute(pool)
    .await?;
    get_task(pool, id)
        .await?
        .ok_or_else(|| anyhow!("Task '{}' not found after metadata update", id))
}
