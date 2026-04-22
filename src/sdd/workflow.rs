use anyhow::{anyhow, Result};
use chrono::Utc;
use sqlx::{Sqlite, SqlitePool, Transaction};

use crate::sdd::{
    event::emit_event_tx,
    spec::{get_spec, Spec},
    task::{get_task, Task},
};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LifecycleKind {
    Spec,
    Task,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LifecycleTransition {
    pub kind: LifecycleKind,
    pub entity_id: String,
    pub from_status: String,
    pub to_status: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct InvariantCheck {
    pub name: &'static str,
    pub passed: bool,
    pub detail: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct InvariantReport {
    pub checks: Vec<InvariantCheck>,
}

impl InvariantReport {
    #[cfg(test)]
    pub fn all_passed(&self) -> bool {
        self.checks.iter().all(|check| check.passed)
    }

    pub fn first_failure(&self) -> Option<&InvariantCheck> {
        self.checks.iter().find(|check| !check.passed)
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SpecStatusUpdatePlan {
    pub transition: LifecycleTransition,
    pub updated_by: String,
    pub invariants: InvariantReport,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TaskStatusUpdatePlan {
    pub transition: LifecycleTransition,
    pub spec: String,
    pub invariants: InvariantReport,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct OpenTaskSummary {
    count: usize,
    sample_ids: Vec<String>,
}

#[derive(Debug, Clone, Copy)]
pub struct SpecStatusUpdateRequest<'a> {
    pub id: &'a str,
    pub current_status: &'a str,
    pub new_status: &'a str,
    pub updated_by: &'a str,
    pub ac_total: i64,
    pub ac_passed: i64,
}

#[derive(Debug, Clone, Copy)]
pub struct TaskStatusUpdateRequest<'a> {
    pub id: &'a str,
    pub spec: &'a str,
    pub current_status: &'a str,
    pub new_status: &'a str,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct LifecycleEvent<'a> {
    pub event_type: &'a str,
    pub payload_json: &'a str,
}

pub fn validate_spec_transition(from: &str, to: &str) -> Result<LifecycleTransition> {
    let valid = matches!(
        (from, to),
        ("draft", "approved")
            | ("approved", "in_progress")
            | ("in_progress", "done")
            | ("in_progress", "paused")
            | ("paused", "in_progress")
    );

    if !valid {
        return Err(anyhow!("Invalid transition: {} -> {}", from, to));
    }

    Ok(LifecycleTransition {
        kind: LifecycleKind::Spec,
        entity_id: String::new(),
        from_status: from.to_string(),
        to_status: to.to_string(),
    })
}

pub fn validate_task_transition(from: &str, to: &str) -> Result<LifecycleTransition> {
    let valid = matches!(
        (from, to),
        ("pending", "in_progress")
            | ("in_progress", "done")
            | ("in_progress", "failed")
            | ("failed", "pending")
    );

    if !valid {
        return Err(anyhow!("Invalid task transition: {} -> {}", from, to));
    }

    Ok(LifecycleTransition {
        kind: LifecycleKind::Task,
        entity_id: String::new(),
        from_status: from.to_string(),
        to_status: to.to_string(),
    })
}

fn evaluate_spec_invariants(
    spec_id: &str,
    current_status: &str,
    new_status: &str,
    ac_total: i64,
    ac_passed: i64,
    open_tasks: Option<&OpenTaskSummary>,
) -> InvariantReport {
    let mut checks = Vec::new();

    if new_status == "done" {
        checks.push(InvariantCheck {
            name: "spec_done_requires_acceptance_criteria",
            passed: ac_total > 0,
            detail: if ac_total > 0 {
                format!(
                    "Spec '{}' has {} acceptance criteria defined",
                    spec_id, ac_total
                )
            } else {
                format!(
                    "Cannot mark spec '{}' as done: ac_total is 0 (no acceptance criteria defined)",
                    spec_id
                )
            },
        });
        checks.push(InvariantCheck {
            name: "spec_done_requires_all_acceptance_criteria_passed",
            passed: ac_total > 0 && ac_passed == ac_total,
            detail: if ac_total > 0 && ac_passed == ac_total {
                format!(
                    "Spec '{}' acceptance criteria complete: {}/{}",
                    spec_id, ac_passed, ac_total
                )
            } else {
                format!(
                    "Cannot mark spec '{}' as done: ac_passed ({}) != ac_total ({})",
                    spec_id, ac_passed, ac_total
                )
            },
        });
        if let Some(open_tasks) = open_tasks {
            let open_task_detail = if open_tasks.count == 0 {
                format!("Spec '{}' has no open tasks remaining", spec_id)
            } else {
                let task_list = open_tasks.sample_ids.join(", ");
                let suffix = if open_tasks.count > open_tasks.sample_ids.len() {
                    format!(" (showing {}, more remain)", task_list)
                } else {
                    format!(": {}", task_list)
                };
                format!(
                    "Cannot mark spec '{}' as done: {} task(s) are still open{}",
                    spec_id, open_tasks.count, suffix
                )
            };
            checks.push(InvariantCheck {
                name: "spec_done_requires_all_tasks_done",
                passed: open_tasks.count == 0,
                detail: open_task_detail,
            });
        }
    } else {
        checks.push(InvariantCheck {
            name: "spec_transition_has_no_additional_done_gate",
            passed: true,
            detail: format!(
                "No additional invariants for spec transition {} -> {}",
                current_status, new_status
            ),
        });
    }

    InvariantReport { checks }
}

pub fn evaluate_task_invariants(
    _task_id: &str,
    current_status: &str,
    new_status: &str,
) -> InvariantReport {
    InvariantReport {
        checks: vec![InvariantCheck {
            name: "task_transition_has_no_additional_invariants_yet",
            passed: true,
            detail: format!(
                "No additional task invariants for transition {} -> {}",
                current_status, new_status
            ),
        }],
    }
}

pub fn plan_spec_status_update(
    request: SpecStatusUpdateRequest<'_>,
) -> Result<SpecStatusUpdatePlan> {
    let mut transition = validate_spec_transition(request.current_status, request.new_status)?;
    transition.entity_id = request.id.to_string();

    let invariants = evaluate_spec_invariants(
        request.id,
        request.current_status,
        request.new_status,
        request.ac_total,
        request.ac_passed,
        None,
    );

    if let Some(failure) = invariants.first_failure() {
        return Err(anyhow!(failure.detail.clone()));
    }

    Ok(SpecStatusUpdatePlan {
        transition,
        updated_by: request.updated_by.to_string(),
        invariants,
    })
}

pub fn plan_task_status_update(
    request: TaskStatusUpdateRequest<'_>,
) -> Result<TaskStatusUpdatePlan> {
    let mut transition = validate_task_transition(request.current_status, request.new_status)?;
    transition.entity_id = request.id.to_string();

    let invariants =
        evaluate_task_invariants(request.id, request.current_status, request.new_status);
    if let Some(failure) = invariants.first_failure() {
        return Err(anyhow!(failure.detail.clone()));
    }

    Ok(TaskStatusUpdatePlan {
        transition,
        spec: request.spec.to_string(),
        invariants,
    })
}

pub async fn apply_spec_status_update_with_event(
    pool: &SqlitePool,
    id: &str,
    new_status: &str,
    updated_by: &str,
    event: LifecycleEvent<'_>,
) -> Result<Spec> {
    apply_spec_status_update_with_event_inner(pool, id, new_status, updated_by, event, false).await
}

#[allow(dead_code)]
#[doc(hidden)]
pub async fn apply_spec_status_update_with_event_test_hook(
    pool: &SqlitePool,
    id: &str,
    new_status: &str,
    updated_by: &str,
    event: LifecycleEvent<'_>,
    inject_failure_after_status_write: bool,
) -> Result<Spec> {
    apply_spec_status_update_with_event_inner(
        pool,
        id,
        new_status,
        updated_by,
        event,
        inject_failure_after_status_write,
    )
    .await
}

pub async fn apply_task_status_update_with_event(
    pool: &SqlitePool,
    id: &str,
    new_status: &str,
    event: LifecycleEvent<'_>,
) -> Result<Task> {
    apply_task_status_update_with_event_inner(pool, id, new_status, event, false).await
}

#[allow(dead_code)]
#[doc(hidden)]
pub async fn apply_task_status_update_with_event_test_hook(
    pool: &SqlitePool,
    id: &str,
    new_status: &str,
    event: LifecycleEvent<'_>,
    inject_failure_after_status_write: bool,
) -> Result<Task> {
    apply_task_status_update_with_event_inner(
        pool,
        id,
        new_status,
        event,
        inject_failure_after_status_write,
    )
    .await
}

pub async fn approve_spec(pool: &SqlitePool, id: &str, updated_by: &str) -> Result<Spec> {
    apply_spec_status_update_with_event(
        pool,
        id,
        "approved",
        updated_by,
        LifecycleEvent {
            event_type: "SpecApproved",
            payload_json: "{}",
        },
    )
    .await
}

pub async fn start_spec(pool: &SqlitePool, id: &str, updated_by: &str) -> Result<Spec> {
    apply_spec_status_update_with_event(
        pool,
        id,
        "in_progress",
        updated_by,
        LifecycleEvent {
            event_type: "SpecStarted",
            payload_json: "{}",
        },
    )
    .await
}

pub async fn complete_spec(pool: &SqlitePool, id: &str, updated_by: &str) -> Result<Spec> {
    apply_spec_status_update_with_event(
        pool,
        id,
        "done",
        updated_by,
        LifecycleEvent {
            event_type: "SpecCompleted",
            payload_json: "{}",
        },
    )
    .await
}

pub async fn apply_legacy_spec_status_update(
    pool: &SqlitePool,
    id: &str,
    new_status: &str,
    updated_by: &str,
) -> Result<Spec> {
    let spec = get_spec(pool, id)
        .await?
        .ok_or_else(|| anyhow!("Spec '{}' not found", id))?;
    let event_type = legacy_spec_status_event_type(&spec.status, new_status)?;

    apply_spec_status_update_with_event(
        pool,
        id,
        new_status,
        updated_by,
        LifecycleEvent {
            event_type,
            payload_json: "{}",
        },
    )
    .await
}

pub async fn start_task(pool: &SqlitePool, id: &str) -> Result<Task> {
    let payload = task_event_payload(id);
    apply_task_status_update_with_event(
        pool,
        id,
        "in_progress",
        LifecycleEvent {
            event_type: "TaskStarted",
            payload_json: &payload,
        },
    )
    .await
}

pub async fn complete_task(pool: &SqlitePool, id: &str) -> Result<Task> {
    let payload = task_event_payload(id);
    apply_task_status_update_with_event(
        pool,
        id,
        "done",
        LifecycleEvent {
            event_type: "TaskCompleted",
            payload_json: &payload,
        },
    )
    .await
}

pub async fn fail_task(pool: &SqlitePool, id: &str) -> Result<Task> {
    let payload = task_event_payload(id);
    apply_task_status_update_with_event(
        pool,
        id,
        "failed",
        LifecycleEvent {
            event_type: "TaskFailed",
            payload_json: &payload,
        },
    )
    .await
}

pub async fn apply_legacy_task_status_update(
    pool: &SqlitePool,
    id: &str,
    new_status: &str,
) -> Result<Task> {
    let task = get_task(pool, id)
        .await?
        .ok_or_else(|| anyhow!("Task '{}' not found", id))?;
    let payload = task_event_payload(&task.id);
    let event_type = legacy_task_status_event_type(&task.status, new_status)?;

    apply_task_status_update_with_event(
        pool,
        id,
        new_status,
        LifecycleEvent {
            event_type,
            payload_json: &payload,
        },
    )
    .await
}

fn task_event_payload(task_id: &str) -> String {
    format!(r#"{{"task":"{}"}}"#, task_id)
}

fn legacy_spec_status_event_type(current_status: &str, new_status: &str) -> Result<&'static str> {
    match (current_status, new_status) {
        ("draft", "approved") => Ok("SpecApproved"),
        ("approved", "in_progress") => Ok("SpecStarted"),
        ("in_progress", "paused") => Ok("SpecPaused"),
        ("paused", "in_progress") => Ok("SpecResumed"),
        ("in_progress", "done") => Ok("SpecCompleted"),
        _ => Err(anyhow!(
            "Legacy spec status update route does not support transition {} -> {}",
            current_status,
            new_status
        )),
    }
}

fn legacy_task_status_event_type(current_status: &str, new_status: &str) -> Result<&'static str> {
    match (current_status, new_status) {
        ("pending", "in_progress") => Ok("TaskStarted"),
        ("in_progress", "done") => Ok("TaskCompleted"),
        ("in_progress", "failed") => Ok("TaskFailed"),
        ("failed", "pending") => Ok("TaskReplanned"),
        _ => Err(anyhow!(
            "Legacy task status update route does not support transition {} -> {}",
            current_status,
            new_status
        )),
    }
}

async fn apply_spec_status_update_with_event_inner(
    pool: &SqlitePool,
    id: &str,
    new_status: &str,
    updated_by: &str,
    event: LifecycleEvent<'_>,
    inject_failure_after_status_write: bool,
) -> Result<Spec> {
    let mut tx = pool.begin().await?;
    let spec = load_spec_for_update(&mut tx, id).await?;

    let plan = plan_spec_status_update(SpecStatusUpdateRequest {
        id: &spec.id,
        current_status: &spec.status,
        new_status,
        updated_by,
        ac_total: spec.ac_total,
        ac_passed: spec.ac_passed,
    })?;
    enforce_spec_done_gate_in_tx(
        &mut tx,
        &spec.id,
        &plan.transition.to_status,
        spec.ac_total,
        spec.ac_passed,
    )
    .await?;

    let now = Utc::now().to_rfc3339();
    let result = sqlx::query(
        "UPDATE specs SET status = ?, updated_at = ?, updated_by = ? WHERE id = ? AND status = ?",
    )
    .bind(&plan.transition.to_status)
    .bind(&now)
    .bind(&plan.updated_by)
    .bind(id)
    .bind(&plan.transition.from_status)
    .execute(&mut *tx)
    .await?;

    if result.rows_affected() == 0 {
        return Err(anyhow!(
            "Spec '{}' status changed concurrently (expected '{}', no longer matches)",
            id,
            plan.transition.from_status
        ));
    }

    if inject_failure_after_status_write {
        return Err(anyhow!(
            "Injected lifecycle event persistence failure for spec '{}'",
            id
        ));
    }

    emit_event_tx(
        &mut tx,
        event.event_type,
        Some(id),
        Some(&plan.updated_by),
        event.payload_json,
    )
    .await?;

    tx.commit().await?;

    get_spec(pool, id)
        .await?
        .ok_or_else(|| anyhow!("Spec '{}' not found after transactional update", id))
}

async fn apply_task_status_update_with_event_inner(
    pool: &SqlitePool,
    id: &str,
    new_status: &str,
    event: LifecycleEvent<'_>,
    inject_failure_after_status_write: bool,
) -> Result<Task> {
    let mut tx = pool.begin().await?;
    let task = load_task_for_update(&mut tx, id).await?;

    let plan = plan_task_status_update(TaskStatusUpdateRequest {
        id: &task.id,
        spec: &task.spec,
        current_status: &task.status,
        new_status,
    })?;
    enforce_task_update_gate_in_tx(&mut tx, &task.id, &task.spec, &plan.transition.to_status)
        .await?;

    let now = Utc::now().to_rfc3339();
    let result =
        sqlx::query("UPDATE tasks SET status = ?, updated_at = ? WHERE id = ? AND status = ?")
            .bind(&plan.transition.to_status)
            .bind(&now)
            .bind(id)
            .bind(&plan.transition.from_status)
            .execute(&mut *tx)
            .await?;

    if result.rows_affected() == 0 {
        return Err(anyhow!(
            "Task '{}' status changed concurrently (expected '{}', no longer matches)",
            id,
            plan.transition.from_status
        ));
    }

    if inject_failure_after_status_write {
        return Err(anyhow!(
            "Injected lifecycle event persistence failure for task '{}'",
            id
        ));
    }

    emit_event_tx(
        &mut tx,
        event.event_type,
        Some(&plan.spec),
        Some(&task.agent),
        event.payload_json,
    )
    .await?;

    tx.commit().await?;

    get_task(pool, id)
        .await?
        .ok_or_else(|| anyhow!("Task '{}' not found after transactional update", id))
}

async fn load_spec_for_update(tx: &mut Transaction<'_, Sqlite>, id: &str) -> Result<Spec> {
    sqlx::query_as::<_, Spec>(
        "SELECT id, title, status, priority, depends_on, agents, ac_total, ac_passed, created_at, updated_at, updated_by \
         FROM specs WHERE id = ?",
    )
    .bind(id)
    .fetch_optional(&mut **tx)
    .await?
    .ok_or_else(|| anyhow!("Spec '{}' not found", id))
}

async fn load_task_for_update(tx: &mut Transaction<'_, Sqlite>, id: &str) -> Result<Task> {
    sqlx::query_as::<_, Task>(
        "SELECT id, spec, title, agent, status, inputs, output_artifact, created_at, updated_at \
         FROM tasks WHERE id = ?",
    )
    .bind(id)
    .fetch_optional(&mut **tx)
    .await?
    .ok_or_else(|| anyhow!("Task '{}' not found", id))
}

#[allow(dead_code)]
pub async fn enforce_spec_done_gate(
    pool: &SqlitePool,
    spec_id: &str,
    new_status: &str,
    ac_total: i64,
    ac_passed: i64,
) -> Result<()> {
    let mut tx = pool.begin().await?;
    let result =
        enforce_spec_done_gate_in_tx(&mut tx, spec_id, new_status, ac_total, ac_passed).await;
    tx.rollback().await?;
    result
}

pub async fn enforce_spec_ac_update_gate(
    pool: &SqlitePool,
    spec_id: &str,
    spec_status: &str,
    ac_total: i64,
    ac_passed: i64,
) -> Result<()> {
    let mut tx = pool.begin().await?;
    let result =
        enforce_spec_ac_update_gate_in_tx(&mut tx, spec_id, spec_status, ac_total, ac_passed).await;
    tx.rollback().await?;
    result
}

#[allow(dead_code)]
pub async fn enforce_task_update_gate(
    pool: &SqlitePool,
    task_id: &str,
    spec_id: &str,
    new_status: &str,
) -> Result<()> {
    let mut tx = pool.begin().await?;
    let result = enforce_task_update_gate_in_tx(&mut tx, task_id, spec_id, new_status).await;
    tx.rollback().await?;
    result
}

async fn enforce_spec_done_gate_in_tx(
    tx: &mut Transaction<'_, Sqlite>,
    spec_id: &str,
    new_status: &str,
    ac_total: i64,
    ac_passed: i64,
) -> Result<()> {
    if new_status != "done" {
        return Ok(());
    }

    let open_tasks = load_open_task_summary(tx, spec_id).await?;
    let invariants = evaluate_spec_invariants(
        spec_id,
        "in_progress",
        new_status,
        ac_total,
        ac_passed,
        Some(&open_tasks),
    );

    if let Some(failure) = invariants.first_failure() {
        return Err(anyhow!(failure.detail.clone()));
    }

    Ok(())
}

async fn enforce_spec_ac_update_gate_in_tx(
    tx: &mut Transaction<'_, Sqlite>,
    spec_id: &str,
    spec_status: &str,
    ac_total: i64,
    ac_passed: i64,
) -> Result<()> {
    if spec_status != "done" {
        return Ok(());
    }

    enforce_spec_done_gate_in_tx(tx, spec_id, "done", ac_total, ac_passed).await
}

async fn enforce_task_update_gate_in_tx(
    tx: &mut Transaction<'_, Sqlite>,
    task_id: &str,
    spec_id: &str,
    new_status: &str,
) -> Result<()> {
    if new_status == "done" {
        return Ok(());
    }

    let spec = load_spec_for_update(tx, spec_id).await?;
    if spec.status == "done" {
        return Err(anyhow!(
            "Cannot move task '{}' to '{}' because spec '{}' is already done",
            task_id,
            new_status,
            spec_id
        ));
    }

    Ok(())
}

async fn load_open_task_summary(
    tx: &mut Transaction<'_, Sqlite>,
    spec_id: &str,
) -> Result<OpenTaskSummary> {
    let open_task_ids = sqlx::query_scalar::<_, String>(
        "SELECT id FROM tasks WHERE spec = ? AND status != 'done' ORDER BY id LIMIT 5",
    )
    .bind(spec_id)
    .fetch_all(&mut **tx)
    .await?;

    let open_task_count = sqlx::query_scalar::<_, i64>(
        "SELECT COUNT(1) FROM tasks WHERE spec = ? AND status != 'done'",
    )
    .bind(spec_id)
    .fetch_one(&mut **tx)
    .await?;

    Ok(OpenTaskSummary {
        count: open_task_count as usize,
        sample_ids: open_task_ids,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::sdd::{
        event::query_events,
        spec::{create_spec, update_spec_ac},
        task::create_task,
        test_helpers::make_pool,
    };

    #[test]
    fn spec_plan_accepts_valid_transition_without_done_gate() {
        let plan = plan_spec_status_update(SpecStatusUpdateRequest {
            id: "SPEC-001",
            current_status: "draft",
            new_status: "approved",
            updated_by: "human",
            ac_total: 0,
            ac_passed: 0,
        })
        .unwrap();

        assert_eq!(plan.transition.kind, LifecycleKind::Spec);
        assert_eq!(plan.transition.entity_id, "SPEC-001");
        assert_eq!(plan.transition.from_status, "draft");
        assert_eq!(plan.transition.to_status, "approved");
        assert!(plan.invariants.all_passed());
    }

    #[test]
    fn spec_plan_rejects_done_without_acceptance_criteria() {
        let err = plan_spec_status_update(SpecStatusUpdateRequest {
            id: "SPEC-002",
            current_status: "in_progress",
            new_status: "done",
            updated_by: "agent",
            ac_total: 0,
            ac_passed: 0,
        })
        .unwrap_err();

        assert!(err.to_string().contains("ac_total is 0"));
    }

    #[test]
    fn spec_invariant_report_exposes_failure_details() {
        let report = evaluate_spec_invariants("SPEC-003", "in_progress", "done", 3, 2, None);

        assert!(!report.all_passed());
        assert_eq!(report.checks.len(), 2);
        assert!(report
            .first_failure()
            .unwrap()
            .detail
            .contains("ac_passed (2) != ac_total (3)"));
    }

    #[test]
    fn task_plan_accepts_replan_transition() {
        let plan = plan_task_status_update(TaskStatusUpdateRequest {
            id: "T010",
            spec: "SPEC-002",
            current_status: "failed",
            new_status: "pending",
        })
        .unwrap();

        assert_eq!(plan.transition.kind, LifecycleKind::Task);
        assert_eq!(plan.transition.entity_id, "T010");
        assert_eq!(plan.spec, "SPEC-002");
        assert!(plan.invariants.all_passed());
    }

    #[test]
    fn task_plan_rejects_invalid_transition() {
        let err = plan_task_status_update(TaskStatusUpdateRequest {
            id: "T011",
            spec: "SPEC-002",
            current_status: "pending",
            new_status: "done",
        })
        .unwrap_err();

        assert!(err
            .to_string()
            .contains("Invalid task transition: pending -> done"));
    }

    #[tokio::test]
    async fn transactional_spec_update_commits_state_and_event_together() {
        let pool = make_pool().await;
        create_spec(&pool, "SPEC-TX-1", "Transactional spec", "P0", &[])
            .await
            .unwrap();

        let updated = apply_spec_status_update_with_event(
            &pool,
            "SPEC-TX-1",
            "approved",
            "human",
            LifecycleEvent {
                event_type: "SpecApproved",
                payload_json: "{}",
            },
        )
        .await
        .unwrap();

        assert_eq!(updated.status, "approved");
        let events = query_events(
            &pool,
            Some("SpecApproved"),
            Some("SPEC-TX-1"),
            None,
            None,
            None,
            None,
            None,
        )
        .await
        .unwrap();
        assert_eq!(events.len(), 1);
    }

    #[tokio::test]
    async fn semantic_spec_wrapper_preserves_status_and_event_contract() {
        let pool = make_pool().await;
        create_spec(&pool, "SPEC-WRAP-1", "Wrapped spec", "P0", &[])
            .await
            .unwrap();

        let updated = approve_spec(&pool, "SPEC-WRAP-1", "human").await.unwrap();

        assert_eq!(updated.status, "approved");
        let events = query_events(
            &pool,
            Some("SpecApproved"),
            Some("SPEC-WRAP-1"),
            None,
            None,
            None,
            None,
            None,
        )
        .await
        .unwrap();
        assert_eq!(events.len(), 1);
    }

    #[tokio::test]
    async fn legacy_spec_wrapper_routes_paused_resume_transition_through_workflow() {
        let pool = make_pool().await;
        create_spec(&pool, "SPEC-LEGACY-1", "Legacy spec", "P0", &[])
            .await
            .unwrap();

        apply_legacy_spec_status_update(&pool, "SPEC-LEGACY-1", "approved", "human")
            .await
            .unwrap();
        apply_legacy_spec_status_update(&pool, "SPEC-LEGACY-1", "in_progress", "human")
            .await
            .unwrap();
        apply_legacy_spec_status_update(&pool, "SPEC-LEGACY-1", "paused", "human")
            .await
            .unwrap();
        let updated =
            apply_legacy_spec_status_update(&pool, "SPEC-LEGACY-1", "in_progress", "human")
                .await
                .unwrap();

        assert_eq!(updated.status, "in_progress");
        let events = query_events(
            &pool,
            Some("SpecResumed"),
            Some("SPEC-LEGACY-1"),
            Some("human"),
            None,
            None,
            None,
            None,
        )
        .await
        .unwrap();
        assert_eq!(events.len(), 1);
    }

    #[tokio::test]
    async fn transactional_spec_update_rolls_back_on_injected_event_failure() {
        let pool = make_pool().await;
        create_spec(&pool, "SPEC-TX-ROLLBACK", "Rollback spec", "P0", &[])
            .await
            .unwrap();

        let err = apply_spec_status_update_with_event_inner(
            &pool,
            "SPEC-TX-ROLLBACK",
            "approved",
            "human",
            LifecycleEvent {
                event_type: "SpecApproved",
                payload_json: "{}",
            },
            true,
        )
        .await
        .unwrap_err();

        assert!(err
            .to_string()
            .contains("Injected lifecycle event persistence failure"));

        let spec = get_spec(&pool, "SPEC-TX-ROLLBACK").await.unwrap().unwrap();
        assert_eq!(spec.status, "draft");
        let events = query_events(
            &pool,
            Some("SpecApproved"),
            Some("SPEC-TX-ROLLBACK"),
            None,
            None,
            None,
            None,
            None,
        )
        .await
        .unwrap();
        assert!(events.is_empty());
    }

    #[tokio::test]
    async fn transactional_task_update_rolls_back_on_injected_event_failure() {
        let pool = make_pool().await;
        create_spec(&pool, "SPEC-TX-2", "Task rollback spec", "P0", &[])
            .await
            .unwrap();
        create_task(
            &pool,
            "TASK-TX-ROLLBACK",
            "SPEC-TX-2",
            "Transactional task",
            "builder",
            &[],
            None,
        )
        .await
        .unwrap();

        let err = apply_task_status_update_with_event_inner(
            &pool,
            "TASK-TX-ROLLBACK",
            "in_progress",
            LifecycleEvent {
                event_type: "TaskStarted",
                payload_json: r#"{"task":"TASK-TX-ROLLBACK"}"#,
            },
            true,
        )
        .await
        .unwrap_err();

        assert!(err
            .to_string()
            .contains("Injected lifecycle event persistence failure"));

        let task = get_task(&pool, "TASK-TX-ROLLBACK").await.unwrap().unwrap();
        assert_eq!(task.status, "pending");
        let events = query_events(
            &pool,
            Some("TaskStarted"),
            Some("SPEC-TX-2"),
            None,
            None,
            None,
            None,
            None,
        )
        .await
        .unwrap();
        assert!(events.is_empty());
    }

    #[tokio::test]
    async fn semantic_task_wrapper_preserves_status_and_event_contract() {
        let pool = make_pool().await;
        create_spec(&pool, "SPEC-WRAP-2", "Wrapped task spec", "P0", &[])
            .await
            .unwrap();
        create_task(
            &pool,
            "TASK-WRAP-1",
            "SPEC-WRAP-2",
            "Wrapped task",
            "builder",
            &[],
            None,
        )
        .await
        .unwrap();

        let updated = start_task(&pool, "TASK-WRAP-1").await.unwrap();

        assert_eq!(updated.status, "in_progress");
        let events = query_events(
            &pool,
            Some("TaskStarted"),
            Some("SPEC-WRAP-2"),
            None,
            None,
            None,
            None,
            None,
        )
        .await
        .unwrap();
        assert_eq!(events.len(), 1);
        assert_eq!(events[0].payload, r#"{"task":"TASK-WRAP-1"}"#);
    }

    #[tokio::test]
    async fn legacy_task_wrapper_routes_replan_transition_through_workflow() {
        let pool = make_pool().await;
        create_spec(&pool, "SPEC-LEGACY-2", "Legacy task spec", "P0", &[])
            .await
            .unwrap();
        create_task(
            &pool,
            "TASK-LEGACY-1",
            "SPEC-LEGACY-2",
            "Legacy task",
            "builder",
            &[],
            None,
        )
        .await
        .unwrap();

        apply_legacy_task_status_update(&pool, "TASK-LEGACY-1", "in_progress")
            .await
            .unwrap();
        apply_legacy_task_status_update(&pool, "TASK-LEGACY-1", "failed")
            .await
            .unwrap();
        let updated = apply_legacy_task_status_update(&pool, "TASK-LEGACY-1", "pending")
            .await
            .unwrap();

        assert_eq!(updated.status, "pending");
        let events = query_events(
            &pool,
            Some("TaskReplanned"),
            Some("SPEC-LEGACY-2"),
            Some("builder"),
            None,
            None,
            None,
            None,
        )
        .await
        .unwrap();
        assert_eq!(events.len(), 1);
        assert_eq!(events[0].payload, r#"{"task":"TASK-LEGACY-1"}"#);
    }

    #[tokio::test]
    async fn transactional_spec_done_path_preserves_existing_done_gate() {
        let pool = make_pool().await;
        create_spec(&pool, "SPEC-TX-DONE", "Done gate spec", "P0", &[])
            .await
            .unwrap();
        apply_spec_status_update_with_event(
            &pool,
            "SPEC-TX-DONE",
            "approved",
            "human",
            LifecycleEvent {
                event_type: "SpecApproved",
                payload_json: "{}",
            },
        )
        .await
        .unwrap();
        apply_spec_status_update_with_event(
            &pool,
            "SPEC-TX-DONE",
            "in_progress",
            "human",
            LifecycleEvent {
                event_type: "SpecStarted",
                payload_json: "{}",
            },
        )
        .await
        .unwrap();
        update_spec_ac(&pool, "SPEC-TX-DONE", 2, 2).await.unwrap();

        let updated = apply_spec_status_update_with_event(
            &pool,
            "SPEC-TX-DONE",
            "done",
            "human",
            LifecycleEvent {
                event_type: "SpecCompleted",
                payload_json: "{}",
            },
        )
        .await
        .unwrap();

        assert_eq!(updated.status, "done");
    }

    #[tokio::test]
    async fn transactional_spec_done_blocks_when_tasks_are_still_open() {
        let pool = make_pool().await;
        create_spec(&pool, "SPEC-TX-OPEN", "Open task gate", "P0", &[])
            .await
            .unwrap();
        apply_spec_status_update_with_event(
            &pool,
            "SPEC-TX-OPEN",
            "approved",
            "human",
            LifecycleEvent {
                event_type: "SpecApproved",
                payload_json: "{}",
            },
        )
        .await
        .unwrap();
        apply_spec_status_update_with_event(
            &pool,
            "SPEC-TX-OPEN",
            "in_progress",
            "human",
            LifecycleEvent {
                event_type: "SpecStarted",
                payload_json: "{}",
            },
        )
        .await
        .unwrap();
        create_task(
            &pool,
            "TASK-OPEN-1",
            "SPEC-TX-OPEN",
            "Still open",
            "builder",
            &[],
            None,
        )
        .await
        .unwrap();
        update_spec_ac(&pool, "SPEC-TX-OPEN", 1, 1).await.unwrap();

        let err = apply_spec_status_update_with_event(
            &pool,
            "SPEC-TX-OPEN",
            "done",
            "human",
            LifecycleEvent {
                event_type: "SpecCompleted",
                payload_json: "{}",
            },
        )
        .await
        .unwrap_err();

        assert!(err.to_string().contains("task(s) are still open"));
        assert!(err.to_string().contains("TASK-OPEN-1"));
    }

    #[tokio::test]
    async fn task_update_is_blocked_when_parent_spec_is_done() {
        let pool = make_pool().await;
        create_spec(&pool, "SPEC-TX-TASK-GATE", "Task gate", "P0", &[])
            .await
            .unwrap();
        create_task(
            &pool,
            "TASK-TX-TASK-GATE",
            "SPEC-TX-TASK-GATE",
            "Drifted task",
            "builder",
            &[],
            None,
        )
        .await
        .unwrap();
        sqlx::query("UPDATE specs SET status = 'done' WHERE id = ?")
            .bind("SPEC-TX-TASK-GATE")
            .execute(&pool)
            .await
            .unwrap();

        let err = apply_task_status_update_with_event(
            &pool,
            "TASK-TX-TASK-GATE",
            "in_progress",
            LifecycleEvent {
                event_type: "TaskStarted",
                payload_json: r#"{"task":"TASK-TX-TASK-GATE"}"#,
            },
        )
        .await
        .unwrap_err();

        assert!(err.to_string().contains("already done"));
    }

    #[tokio::test]
    async fn done_spec_ac_updates_must_preserve_done_gate() {
        let pool = make_pool().await;
        create_spec(&pool, "SPEC-TX-AC-GATE", "AC gate", "P0", &[])
            .await
            .unwrap();
        update_spec_ac(&pool, "SPEC-TX-AC-GATE", 2, 2)
            .await
            .unwrap();
        sqlx::query("UPDATE specs SET status = 'done' WHERE id = ?")
            .bind("SPEC-TX-AC-GATE")
            .execute(&pool)
            .await
            .unwrap();

        let err = enforce_spec_ac_update_gate(&pool, "SPEC-TX-AC-GATE", "done", 2, 1)
            .await
            .unwrap_err();

        assert!(err.to_string().contains("ac_passed (1) != ac_total (2)"));
    }
}
