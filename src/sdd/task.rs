use anyhow::{anyhow, Result};
use chrono::Utc;
use serde::{Deserialize, Serialize};
use sqlx::SqlitePool;

use crate::sdd::workflow::apply_legacy_task_status_update;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub enum TaskStatus {
    Pending,
    InProgress,
    Done,
    Failed,
}

impl TaskStatus {
    pub fn from_str(s: &str) -> Option<Self> {
        match s {
            "pending" => Some(Self::Pending),
            "in_progress" => Some(Self::InProgress),
            "done" => Some(Self::Done),
            "failed" => Some(Self::Failed),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, sqlx::FromRow)]
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

pub async fn list_tasks(
    pool: &SqlitePool,
    spec_filter: Option<&str>,
    limit: Option<i64>,
    offset: Option<i64>,
) -> Result<Vec<Task>> {
    let mut qb = sqlx::QueryBuilder::<sqlx::Sqlite>::new(
        "SELECT id, spec, title, agent, status, inputs, output_artifact, created_at, updated_at \
         FROM tasks",
    );
    if let Some(spec) = spec_filter {
        qb.push(" WHERE spec = ");
        qb.push_bind(spec);
        qb.push(" ORDER BY id");
    } else {
        qb.push(" ORDER BY spec, id");
    }
    if let Some(lim) = limit {
        qb.push(" LIMIT ");
        qb.push_bind(lim);
        if let Some(off) = offset {
            qb.push(" OFFSET ");
            qb.push_bind(off);
        }
    }

    let tasks: Vec<Task> = qb.build_query_as().fetch_all(pool).await?;
    Ok(tasks)
}

pub async fn update_task_status(pool: &SqlitePool, id: &str, new_status: &str) -> Result<Task> {
    apply_legacy_task_status_update(pool, id, new_status).await
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::sdd::spec::create_spec;
    use crate::sdd::test_helpers::make_pool;
    use crate::sdd::workflow::validate_task_transition;

    // Helper: create a spec and a task in one call.
    async fn setup_task(pool: &SqlitePool, task_id: &str) -> Task {
        create_spec(pool, "SPEC-001", "Test Spec", "P1", &[])
            .await
            .unwrap();
        create_task(pool, task_id, "SPEC-001", "Test Task", "builder", &[], None)
            .await
            .unwrap()
    }

    // TC-01: create_task returns Task with status="pending" and correct fields.
    #[tokio::test]
    async fn tc01_create_task_returns_pending_task_with_correct_fields() {
        let pool = make_pool().await;
        create_spec(&pool, "SPEC-001", "Test Spec", "P1", &[])
            .await
            .unwrap();

        let task = create_task(
            &pool,
            "TASK-001",
            "SPEC-001",
            "Implement feature",
            "sdd-builder",
            &[],
            None,
        )
        .await
        .unwrap();

        assert_eq!(task.id, "TASK-001", "task id must match");
        assert_eq!(task.spec, "SPEC-001", "task spec must match");
        assert_eq!(task.title, "Implement feature", "task title must match");
        assert_eq!(task.agent, "sdd-builder", "task agent must match");
        assert_eq!(task.status, "pending", "new task must have status pending");
        assert!(
            task.output_artifact.is_none(),
            "output_artifact must be None on creation"
        );
    }

    // TC-02: get_task returns Some for existing task, None for non-existing.
    #[tokio::test]
    async fn tc02_get_task_returns_some_for_existing_none_for_missing() {
        let pool = make_pool().await;
        setup_task(&pool, "TASK-001").await;

        let found = get_task(&pool, "TASK-001").await.unwrap();
        assert!(
            found.is_some(),
            "get_task must return Some for existing task"
        );

        let missing = get_task(&pool, "TASK-GHOST").await.unwrap();
        assert!(
            missing.is_none(),
            "get_task must return None for non-existing task"
        );
    }

    // TC-03: list_tasks with no filter returns all tasks.
    #[tokio::test]
    async fn tc03_list_tasks_no_filter_returns_all() {
        let pool = make_pool().await;
        create_spec(&pool, "SPEC-001", "Spec One", "P1", &[])
            .await
            .unwrap();
        create_spec(&pool, "SPEC-002", "Spec Two", "P2", &[])
            .await
            .unwrap();

        create_task(&pool, "TASK-A", "SPEC-001", "Task A", "agent", &[], None)
            .await
            .unwrap();
        create_task(&pool, "TASK-B", "SPEC-002", "Task B", "agent", &[], None)
            .await
            .unwrap();

        let all = list_tasks(&pool, None, None, None).await.unwrap();
        assert_eq!(
            all.len(),
            2,
            "list_tasks with no filter must return all 2 tasks"
        );
    }

    // TC-04: list_tasks with spec filter returns only matching tasks.
    #[tokio::test]
    async fn tc04_list_tasks_spec_filter_returns_only_matching() {
        let pool = make_pool().await;
        create_spec(&pool, "SPEC-001", "Spec One", "P1", &[])
            .await
            .unwrap();
        create_spec(&pool, "SPEC-002", "Spec Two", "P2", &[])
            .await
            .unwrap();

        create_task(&pool, "TASK-A", "SPEC-001", "Task A", "agent", &[], None)
            .await
            .unwrap();
        create_task(&pool, "TASK-B", "SPEC-001", "Task B", "agent", &[], None)
            .await
            .unwrap();
        create_task(&pool, "TASK-C", "SPEC-002", "Task C", "agent", &[], None)
            .await
            .unwrap();

        let filtered = list_tasks(&pool, Some("SPEC-001"), None, None)
            .await
            .unwrap();
        assert_eq!(
            filtered.len(),
            2,
            "spec filter must return only 2 tasks for SPEC-001"
        );
        assert!(
            filtered.iter().all(|t| t.spec == "SPEC-001"),
            "all filtered tasks must belong to SPEC-001"
        );
    }

    // TC-05: valid transition pending → in_progress succeeds.
    #[tokio::test]
    async fn tc05_valid_transition_pending_to_in_progress() {
        let pool = make_pool().await;
        setup_task(&pool, "TASK-001").await;

        let updated = update_task_status(&pool, "TASK-001", "in_progress")
            .await
            .unwrap();
        assert_eq!(
            updated.status, "in_progress",
            "status must be in_progress after transition"
        );
    }

    // TC-06: valid transition in_progress → done succeeds.
    #[tokio::test]
    async fn tc06_valid_transition_in_progress_to_done() {
        let pool = make_pool().await;
        setup_task(&pool, "TASK-001").await;
        update_task_status(&pool, "TASK-001", "in_progress")
            .await
            .unwrap();

        let updated = update_task_status(&pool, "TASK-001", "done").await.unwrap();
        assert_eq!(
            updated.status, "done",
            "status must be done after in_progress→done transition"
        );
    }

    // TC-07: valid transition in_progress → failed succeeds.
    #[tokio::test]
    async fn tc07_valid_transition_in_progress_to_failed() {
        let pool = make_pool().await;
        setup_task(&pool, "TASK-001").await;
        update_task_status(&pool, "TASK-001", "in_progress")
            .await
            .unwrap();

        let updated = update_task_status(&pool, "TASK-001", "failed")
            .await
            .unwrap();
        assert_eq!(
            updated.status, "failed",
            "status must be failed after in_progress→failed transition"
        );
    }

    // TC-08: invalid transition pending → done is rejected.
    #[tokio::test]
    async fn tc08_invalid_transition_pending_to_done_is_rejected() {
        let pool = make_pool().await;
        setup_task(&pool, "TASK-001").await;

        let result = update_task_status(&pool, "TASK-001", "done").await;
        assert!(
            result.is_err(),
            "pending→done must be rejected as invalid transition"
        );
    }

    // TC-09: invalid transition pending → failed is rejected.
    #[tokio::test]
    async fn tc09_invalid_transition_pending_to_failed_is_rejected() {
        let pool = make_pool().await;
        setup_task(&pool, "TASK-001").await;

        let result = update_task_status(&pool, "TASK-001", "failed").await;
        assert!(
            result.is_err(),
            "pending→failed must be rejected as invalid transition"
        );
    }

    // TC-10: invalid transition done → pending is rejected.
    #[tokio::test]
    async fn tc10_invalid_transition_done_to_pending_is_rejected() {
        let pool = make_pool().await;
        setup_task(&pool, "TASK-001").await;
        update_task_status(&pool, "TASK-001", "in_progress")
            .await
            .unwrap();
        update_task_status(&pool, "TASK-001", "done").await.unwrap();

        let result = update_task_status(&pool, "TASK-001", "pending").await;
        assert!(
            result.is_err(),
            "done→pending must be rejected as invalid transition"
        );
    }

    // TC-11: invalid transition failed → in_progress is rejected.
    #[tokio::test]
    async fn tc11_invalid_transition_failed_to_in_progress_is_rejected() {
        let pool = make_pool().await;
        setup_task(&pool, "TASK-001").await;
        update_task_status(&pool, "TASK-001", "in_progress")
            .await
            .unwrap();
        update_task_status(&pool, "TASK-001", "failed")
            .await
            .unwrap();

        let result = update_task_status(&pool, "TASK-001", "in_progress").await;
        assert!(
            result.is_err(),
            "failed→in_progress must be rejected as invalid transition"
        );
    }

    // TC-12: invalid transition done → in_progress is rejected.
    #[tokio::test]
    async fn tc12_invalid_transition_done_to_in_progress_is_rejected() {
        let pool = make_pool().await;
        setup_task(&pool, "TASK-001").await;
        update_task_status(&pool, "TASK-001", "in_progress")
            .await
            .unwrap();
        update_task_status(&pool, "TASK-001", "done").await.unwrap();

        let result = update_task_status(&pool, "TASK-001", "in_progress").await;
        assert!(
            result.is_err(),
            "done→in_progress must be rejected as invalid transition"
        );
    }

    // TC-13: update_task_status on non-existent task returns error.
    #[tokio::test]
    async fn tc13_update_status_on_nonexistent_task_returns_error() {
        let pool = make_pool().await;

        let result = update_task_status(&pool, "TASK-GHOST", "in_progress").await;
        assert!(
            result.is_err(),
            "update_task_status on non-existent task must return error"
        );
    }

    // TC-14: update_task_output_artifact sets the output_artifact field.
    #[tokio::test]
    async fn tc14_update_output_artifact_sets_field() {
        let pool = make_pool().await;
        setup_task(&pool, "TASK-001").await;

        let updated = update_task_output_artifact(&pool, "TASK-001", "src/lib.rs")
            .await
            .unwrap();

        assert_eq!(
            updated.output_artifact,
            Some("src/lib.rs".to_string()),
            "output_artifact must be updated to src/lib.rs"
        );
    }

    // TC-15: validate_task_transition direct call — valid transitions return Ok.
    #[tokio::test]
    async fn tc15_validate_task_transition_valid_cases_return_ok() {
        validate_task_transition("pending", "in_progress").unwrap();
        validate_task_transition("in_progress", "done").unwrap();
        validate_task_transition("in_progress", "failed").unwrap();
        validate_task_transition("failed", "pending").unwrap();
    }

    // TC-16: create_task stores inputs as JSON array.
    #[tokio::test]
    async fn tc16_create_task_stores_inputs_as_json() {
        let pool = make_pool().await;
        create_spec(&pool, "SPEC-001", "Test Spec", "P1", &[])
            .await
            .unwrap();

        let inputs = vec!["artifact-a".to_string(), "artifact-b".to_string()];
        let task = create_task(
            &pool,
            "TASK-001",
            "SPEC-001",
            "Task with inputs",
            "agent",
            &inputs,
            None,
        )
        .await
        .unwrap();

        let parsed: Vec<String> = serde_json::from_str(&task.inputs).unwrap();
        assert_eq!(
            parsed, inputs,
            "inputs must be stored and retrieved as JSON array"
        );
    }

    // TC-17: failed → pending (replan) succeeds via update_task_status.
    #[tokio::test]
    async fn tc17_valid_transition_failed_to_pending_replan() {
        let pool = make_pool().await;
        setup_task(&pool, "TASK-001").await;
        update_task_status(&pool, "TASK-001", "in_progress")
            .await
            .unwrap();
        update_task_status(&pool, "TASK-001", "failed")
            .await
            .unwrap();

        let updated = update_task_status(&pool, "TASK-001", "pending")
            .await
            .unwrap();
        assert_eq!(
            updated.status, "pending",
            "status must be pending after failed→pending replan"
        );
    }

    // TC-18: full round-trip: pending → in_progress → failed → pending → in_progress → done.
    #[tokio::test]
    async fn tc18_full_replan_round_trip() {
        let pool = make_pool().await;
        setup_task(&pool, "TASK-001").await;

        update_task_status(&pool, "TASK-001", "in_progress")
            .await
            .unwrap();
        update_task_status(&pool, "TASK-001", "failed")
            .await
            .unwrap();
        update_task_status(&pool, "TASK-001", "pending")
            .await
            .unwrap();
        update_task_status(&pool, "TASK-001", "in_progress")
            .await
            .unwrap();

        let updated = update_task_status(&pool, "TASK-001", "done").await.unwrap();
        assert_eq!(
            updated.status, "done",
            "task must reach done after replan round-trip"
        );
    }
}
