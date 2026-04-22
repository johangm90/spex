use anyhow::{anyhow, Result};
use chrono::Utc;
use serde_json::{json, Value};
use sqlx::{Sqlite, SqlitePool, Transaction};
use std::collections::BTreeSet;

use crate::sdd::{
    artifact::query_artifacts,
    event::emit_event_tx,
    evidence::{
        get_evidence_bundle_details, get_evidence_bundle_for_entity, list_validation_runs,
        EvidenceBundle, EvidenceBundleDetails, EvidenceRef, ValidationRequirementLevel,
        ValidationRun,
    },
    policy::{
        create_approval_tx, decide_approval_tx, get_approval, resolve_effective_policy,
        ApprovalDecision, ApprovalEntityKind, ApprovalRecord, ApprovalState, CompletionEvidence,
        CompletionPolicy, CreateApproval, RiskyOperationEvaluation, RiskyOperationKind,
        RiskyOperationOutcome, RiskyOperationRequest,
    },
    spec::{get_spec, Spec},
    task::{get_task, Task},
};

#[allow(dead_code)]
const POLICY_APPROVAL_REQUESTED_EVENT: &str = "PolicyApprovalRequested";
#[allow(dead_code)]
const POLICY_APPROVAL_DECIDED_EVENT: &str = "PolicyApprovalDecided";

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

#[allow(dead_code)]
#[derive(Debug, Clone)]
pub struct RiskyOperationApprovalRequest<'a> {
    pub approval_id: &'a str,
    pub operation: RiskyOperationRequest<'a>,
    pub requested_by: &'a str,
    pub request_context_json: &'a Value,
    pub evidence_bundle_id: Option<&'a str>,
    pub expires_at: Option<&'a str>,
}

#[allow(dead_code)]
#[derive(Debug, Clone)]
pub struct RiskyOperationApprovalDecisionRequest<'a> {
    pub approval_id: &'a str,
    pub decision: ApprovalDecision<'a>,
}

#[allow(dead_code)]
#[derive(Debug, Clone, PartialEq)]
pub struct ApprovalRequestWorkflowResult {
    pub created: bool,
    pub approval: ApprovalRecord,
    pub evaluation: RiskyOperationEvaluation,
}

#[allow(dead_code)]
#[derive(Debug, Clone, PartialEq)]
pub struct ApprovalDecisionWorkflowResult {
    pub approval: ApprovalRecord,
    pub evaluation: RiskyOperationEvaluation,
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

pub async fn approve_spec(
    pool: &SqlitePool,
    id: &str,
    updated_by: &str,
    config: Option<&crate::config::SpexConfig>,
) -> Result<Spec> {
    if updated_by.trim().is_empty() {
        anyhow::bail!("updated_by is required");
    }
    let spec = apply_spec_status_update_with_event(
        pool,
        id,
        "approved",
        updated_by,
        LifecycleEvent {
            event_type: "SpecApproved",
            payload_json: "{}",
        },
    )
    .await?;
    crate::webhooks::fire(
        config.and_then(|cfg| cfg.webhooks.as_ref()),
        "SpecApproved",
        json!({
            "spec_id": spec.id,
            "updated_by": updated_by,
        }),
    )
    .await;
    Ok(spec)
}

pub async fn start_spec(pool: &SqlitePool, id: &str, updated_by: &str) -> Result<Spec> {
    if updated_by.trim().is_empty() {
        anyhow::bail!("updated_by is required");
    }
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

pub async fn complete_spec(
    pool: &SqlitePool,
    id: &str,
    updated_by: &str,
    config: Option<&crate::config::SpexConfig>,
) -> Result<Spec> {
    if updated_by.trim().is_empty() {
        anyhow::bail!("updated_by is required");
    }
    let spec = apply_spec_status_update_with_event(
        pool,
        id,
        "done",
        updated_by,
        LifecycleEvent {
            event_type: "SpecCompleted",
            payload_json: "{}",
        },
    )
    .await?;
    crate::webhooks::fire(
        config.and_then(|cfg| cfg.webhooks.as_ref()),
        "SpecDone",
        json!({
            "spec_id": spec.id,
            "updated_by": updated_by,
        }),
    )
    .await;
    Ok(spec)
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

pub async fn start_task(pool: &SqlitePool, id: &str, updated_by: &str) -> Result<Task> {
    if updated_by.trim().is_empty() {
        anyhow::bail!("updated_by is required");
    }
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

pub async fn complete_task(
    pool: &SqlitePool,
    id: &str,
    updated_by: &str,
    config: Option<&crate::config::SpexConfig>,
) -> Result<Task> {
    if updated_by.trim().is_empty() {
        anyhow::bail!("updated_by is required");
    }
    let payload = task_event_payload(id);
    let task = apply_task_status_update_with_event(
        pool,
        id,
        "done",
        LifecycleEvent {
            event_type: "TaskCompleted",
            payload_json: &payload,
        },
    )
    .await?;
    crate::webhooks::fire(
        config.and_then(|cfg| cfg.webhooks.as_ref()),
        "TaskDone",
        json!({
            "task_id": task.id,
            "updated_by": updated_by,
        }),
    )
    .await;
    Ok(task)
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

#[allow(dead_code)]
pub async fn request_risky_operation_approval(
    pool: &SqlitePool,
    request: RiskyOperationApprovalRequest<'_>,
    config: Option<&crate::config::SpexConfig>,
) -> Result<ApprovalRequestWorkflowResult> {
    let evaluation = crate::sdd::policy::evaluate_risky_operation(pool, request.operation).await?;
    if evaluation.outcome != RiskyOperationOutcome::ApprovalRequired {
        return Err(anyhow!(
            "{} is not currently awaiting approval: {}",
            request.operation.operation.as_str(),
            evaluation.reason
        ));
    }

    match evaluation.approval_state.as_ref() {
        Some(ApprovalState::Pending(existing)) => {
            return Ok(ApprovalRequestWorkflowResult {
                created: false,
                approval: existing.clone(),
                evaluation,
            });
        }
        Some(ApprovalState::Approved(_))
        | Some(ApprovalState::Rejected(_))
        | Some(ApprovalState::Cancelled(_))
        | Some(ApprovalState::Expired(_)) => {
            return Err(anyhow!(
                "{} already has a terminal approval decision for ({}, {}, {})",
                request.operation.operation.as_str(),
                request.operation.entity_kind.as_str(),
                request.operation.entity_id,
                request.operation.operation.as_str()
            ));
        }
        Some(ApprovalState::NotRequested) | None => {}
    }

    let mut tx = pool.begin().await?;
    let approval = create_approval_tx(
        &mut tx,
        CreateApproval {
            id: request.approval_id,
            entity_kind: request.operation.entity_kind,
            entity_id: request.operation.entity_id,
            spec: request.operation.spec_ref,
            task: request.operation.task_ref,
            operation_kind: request.operation.operation.as_str(),
            policy_config_id: evaluation
                .effective_policy
                .sources
                .last()
                .map(|source| source.id.as_str()),
            evidence_bundle_id: request.evidence_bundle_id,
            requested_by: request.requested_by,
            request_context_json: request.request_context_json,
            expires_at: request.expires_at,
        },
    )
    .await?;

    emit_policy_approval_event_tx(
        &mut tx,
        POLICY_APPROVAL_REQUESTED_EVENT,
        approval.spec.as_deref(),
        Some(request.requested_by),
        &approval,
        approval_request_event_payload(&approval, &evaluation.reason)?,
    )
    .await?;

    tx.commit().await?;

    crate::webhooks::fire(
        config.and_then(|cfg| cfg.webhooks.as_ref()),
        "ApprovalRequested",
        json!({
            "approval_id": approval.id,
            "updated_by": request.requested_by,
        }),
    )
    .await;

    let evaluation = crate::sdd::policy::evaluate_risky_operation(pool, request.operation).await?;
    Ok(ApprovalRequestWorkflowResult {
        created: true,
        approval,
        evaluation,
    })
}

#[allow(dead_code)]
pub async fn decide_risky_operation_approval(
    pool: &SqlitePool,
    request: RiskyOperationApprovalDecisionRequest<'_>,
) -> Result<ApprovalDecisionWorkflowResult> {
    let approval_before = get_approval(pool, request.approval_id)
        .await?
        .ok_or_else(|| anyhow!("Approval '{}' not found", request.approval_id))?;
    let request_context = approval_before.request_context()?;
    let operation_request =
        risky_operation_request_from_approval_context(&approval_before, &request_context)?;

    let mut tx = pool.begin().await?;
    let approval = decide_approval_tx(&mut tx, request.approval_id, request.decision).await?;
    emit_policy_approval_event_tx(
        &mut tx,
        POLICY_APPROVAL_DECIDED_EVENT,
        approval.spec.as_deref(),
        approval.decided_by.as_deref(),
        &approval,
        approval_decision_event_payload(&approval)?,
    )
    .await?;
    tx.commit().await?;

    let evaluation = crate::sdd::policy::evaluate_risky_operation(pool, operation_request).await?;
    Ok(ApprovalDecisionWorkflowResult {
        approval,
        evaluation,
    })
}

fn task_event_payload(task_id: &str) -> String {
    format!(r#"{{"task":"{}"}}"#, task_id)
}

#[allow(dead_code)]
fn approval_request_event_payload(
    approval: &ApprovalRecord,
    evaluation_reason: &str,
) -> Result<String> {
    Ok(serde_json::to_string(&json!({
        "approval_id": approval.id,
        "entity_kind": approval.entity_kind.as_str(),
        "entity_id": approval.entity_id,
        "operation_kind": approval.operation_kind,
        "status": approval.status.as_str(),
        "requested_by": approval.requested_by,
        "policy_config_id": approval.policy_config_id,
        "evidence_bundle_id": approval.evidence_bundle_id,
        "reason": evaluation_reason,
        "request_context": approval.request_context()?,
    }))?)
}

#[allow(dead_code)]
fn approval_decision_event_payload(approval: &ApprovalRecord) -> Result<String> {
    Ok(serde_json::to_string(&json!({
        "approval_id": approval.id,
        "entity_kind": approval.entity_kind.as_str(),
        "entity_id": approval.entity_id,
        "operation_kind": approval.operation_kind,
        "status": approval.status.as_str(),
        "decided_by": approval.decided_by,
        "decision_reason": approval.decision_reason,
    }))?)
}

#[allow(dead_code)]
async fn emit_policy_approval_event_tx(
    tx: &mut Transaction<'_, Sqlite>,
    event_type: &str,
    spec: Option<&str>,
    agent: Option<&str>,
    approval: &ApprovalRecord,
    payload_json: String,
) -> Result<()> {
    emit_event_tx(tx, event_type, spec, agent, &payload_json).await?;
    let event_id = sqlx::query_scalar::<_, i64>("SELECT last_insert_rowid()")
        .fetch_one(&mut **tx)
        .await?;

    insert_policy_audit_ref_tx(tx, event_id, "approval", &approval.id, "subject").await?;
    if let Some(spec_id) = approval.spec.as_deref() {
        insert_policy_audit_ref_tx(tx, event_id, "spec", spec_id, "subject").await?;
    }
    if let Some(task_id) = approval.task.as_deref() {
        insert_policy_audit_ref_tx(tx, event_id, "task", task_id, "subject").await?;
    }
    if let Some(policy_config_id) = approval.policy_config_id.as_deref() {
        insert_policy_audit_ref_tx(tx, event_id, "policy_config", policy_config_id, "blocking")
            .await?;
    }
    if let Some(evidence_bundle_id) = approval.evidence_bundle_id.as_deref() {
        insert_policy_audit_ref_tx(tx, event_id, "evidence_bundle", evidence_bundle_id, "input")
            .await?;
    }

    Ok(())
}

#[allow(dead_code)]
async fn insert_policy_audit_ref_tx(
    tx: &mut Transaction<'_, Sqlite>,
    event_id: i64,
    ref_kind: &str,
    ref_id: &str,
    relation: &str,
) -> Result<()> {
    sqlx::query(
        "INSERT INTO policy_audit_refs (event_id, ref_kind, ref_id, relation) VALUES (?, ?, ?, ?)",
    )
    .bind(event_id)
    .bind(ref_kind)
    .bind(ref_id)
    .bind(relation)
    .execute(&mut **tx)
    .await?;
    Ok(())
}

#[allow(dead_code)]
fn risky_operation_request_from_approval_context<'a>(
    approval: &'a ApprovalRecord,
    context: &'a Value,
) -> Result<RiskyOperationRequest<'a>> {
    let spec_status = context
        .get("spec_status")
        .and_then(Value::as_str)
        .ok_or_else(|| {
            anyhow!(
                "approval '{}' is missing request_context_json.spec_status",
                approval.id
            )
        })?;
    let write_path = context.get("write_path").and_then(Value::as_str);
    let agent = context.get("agent").and_then(Value::as_str);
    let completion_evidence = context
        .get("completion_evidence")
        .map(parse_completion_evidence)
        .transpose()?;

    Ok(RiskyOperationRequest {
        spec_status,
        spec_ref: approval.spec.as_deref(),
        task_ref: approval.task.as_deref(),
        agent,
        entity_kind: approval.entity_kind,
        entity_id: &approval.entity_id,
        operation: RiskyOperationKind::from_str(&approval.operation_kind)
            .ok_or_else(|| anyhow!("unknown risky operation kind '{}'", approval.operation_kind))?,
        write_path,
        completion_evidence,
    })
}

#[allow(dead_code)]
fn parse_completion_evidence(value: &Value) -> Result<CompletionEvidence> {
    let object = value
        .as_object()
        .ok_or_else(|| anyhow!("completion_evidence must be a JSON object"))?;

    let satisfied_validation = object
        .get("satisfied_validation")
        .and_then(Value::as_str)
        .map(|value| {
            parse_validation_requirement_alias(value).ok_or_else(|| {
                anyhow!(
                    "completion_evidence.satisfied_validation must be one of fast|primary|full|custom"
                )
            })
        })
        .transpose()?;

    Ok(CompletionEvidence {
        has_evidence_bundle: object
            .get("has_evidence_bundle")
            .and_then(Value::as_bool)
            .unwrap_or(false),
        has_rationale: object
            .get("has_rationale")
            .and_then(Value::as_bool)
            .unwrap_or(false),
        satisfied_validation,
    })
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
    if plan.transition.to_status == "done" {
        enforce_spec_completion_gate(pool, &spec, updated_by).await?;
    }

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
    enforce_task_update_gate_in_tx(pool, &mut tx, &task, &plan.transition.to_status).await?;

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
    _spec_id: &str,
    new_status: &str,
) -> Result<()> {
    let mut tx = pool.begin().await?;
    let task = load_task_for_update(&mut tx, task_id).await?;
    let result = enforce_task_update_gate_in_tx(pool, &mut tx, &task, new_status).await;
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
    pool: &SqlitePool,
    tx: &mut Transaction<'_, Sqlite>,
    task: &Task,
    new_status: &str,
) -> Result<()> {
    let spec = load_spec_for_update(tx, &task.spec).await?;

    if new_status == "done" {
        return enforce_task_completion_gate(pool, task, &spec.status).await;
    }

    if spec.status == "done" {
        return Err(anyhow!(
            "Cannot move task '{}' to '{}' because spec '{}' is already done",
            task.id,
            new_status,
            task.spec
        ));
    }

    Ok(())
}

async fn enforce_task_completion_gate(
    pool: &SqlitePool,
    task: &Task,
    spec_status: &str,
) -> Result<()> {
    let effective_policy = resolve_effective_policy(
        pool,
        Some(&task.spec),
        Some(&task.id),
        Some(&task.agent),
        spec_status,
    )
    .await
    .map_err(|error| {
        anyhow!(
            "Cannot mark task '{}' as done: invalid completion policy configuration: {}",
            task.id,
            error
        )
    })?;

    if !effective_policy.fail_closed {
        return Ok(());
    }

    let evidence_bundle =
        get_evidence_bundle_for_entity(pool, &EvidenceRef::for_task(&task.spec, &task.id)).await?;
    let successful_validations =
        list_validation_runs(pool, Some(&task.spec), Some(&task.id), None, Some(true)).await?;
    let missing = collect_missing_task_completion_evidence(
        task,
        evidence_bundle.as_ref(),
        &successful_validations,
        &effective_policy.task_completion,
    );
    let evaluation = crate::sdd::policy::evaluate_risky_operation(
        pool,
        RiskyOperationRequest {
            spec_status,
            spec_ref: Some(&task.spec),
            task_ref: Some(&task.id),
            agent: Some(&task.agent),
            entity_kind: ApprovalEntityKind::Task,
            entity_id: &task.id,
            operation: RiskyOperationKind::CompleteTask,
            write_path: None,
            completion_evidence: Some(build_completion_evidence(
                evidence_bundle.as_ref(),
                &successful_validations,
            )),
        },
    )
    .await?;

    let mut issues = missing;
    append_completion_policy_issue(
        &mut issues,
        &task.id,
        "task",
        RiskyOperationKind::CompleteTask,
        &evaluation,
    );

    if issues.is_empty() {
        Ok(())
    } else {
        Err(anyhow!(
            "Cannot mark task '{}' as done: {}",
            task.id,
            issues.join("; ")
        ))
    }
}

async fn enforce_spec_completion_gate(
    pool: &SqlitePool,
    spec: &Spec,
    updated_by: &str,
) -> Result<()> {
    let effective_policy =
        resolve_effective_policy(pool, Some(&spec.id), None, Some(updated_by), &spec.status)
            .await
            .map_err(|error| {
                anyhow!(
                    "Cannot mark spec '{}' as done: invalid completion policy configuration: {}",
                    spec.id,
                    error
                )
            })?;

    if !effective_policy.fail_closed {
        return Ok(());
    }

    let evidence_details =
        get_evidence_bundle_details_for_entity(pool, &EvidenceRef::for_spec(&spec.id)).await?;
    let successful_validations =
        list_validation_runs(pool, Some(&spec.id), None, None, Some(true)).await?;
    let spec_artifacts = query_artifacts(pool, Some(&spec.id), None, None, None).await?;

    let mut issues = collect_missing_spec_completion_evidence(
        spec,
        evidence_details.as_ref(),
        &successful_validations,
        spec_artifacts
            .iter()
            .map(|artifact| artifact.id.as_str())
            .collect(),
        &effective_policy.spec_completion,
    );

    let evaluation = crate::sdd::policy::evaluate_risky_operation(
        pool,
        RiskyOperationRequest {
            spec_status: &spec.status,
            spec_ref: Some(&spec.id),
            task_ref: None,
            agent: Some(updated_by),
            entity_kind: ApprovalEntityKind::Spec,
            entity_id: &spec.id,
            operation: RiskyOperationKind::CompleteSpec,
            write_path: None,
            completion_evidence: Some(build_completion_evidence(
                evidence_details.as_ref().map(|details| &details.bundle),
                &successful_validations,
            )),
        },
    )
    .await?;

    append_completion_policy_issue(
        &mut issues,
        &spec.id,
        "spec",
        RiskyOperationKind::CompleteSpec,
        &evaluation,
    );

    if issues.is_empty() {
        Ok(())
    } else {
        Err(anyhow!(
            "Cannot mark spec '{}' as done: {}",
            spec.id,
            issues.join("; ")
        ))
    }
}

fn collect_missing_task_completion_evidence(
    task: &Task,
    evidence_bundle: Option<&EvidenceBundle>,
    successful_validations: &[ValidationRun],
    completion_policy: &CompletionPolicy,
) -> Vec<String> {
    let mut missing = Vec::new();

    if completion_policy.require_evidence_bundle && evidence_bundle.is_none() {
        missing.push(format!(
            "missing evidence bundle for task '{}' (record a bundle before completing the task)",
            task.id
        ));
    }

    if completion_policy.require_rationale {
        match evidence_bundle {
            Some(bundle) if bundle_has_summary(bundle) => {}
            Some(bundle) => missing.push(format!(
                "evidence bundle '{}' is missing a non-empty summary",
                bundle.id
            )),
            None if !completion_policy.require_evidence_bundle => missing.push(
                "missing completion summary evidence (record a non-empty evidence bundle summary)"
                    .to_string(),
            ),
            None => {}
        }
    }

    if let Some(required_validation) = completion_policy.require_validation {
        if !successful_validations
            .iter()
            .any(|run| validation_alias_satisfies(&run.command_alias, required_validation))
        {
            let available_aliases = collect_successful_validation_aliases(successful_validations);
            let available_detail = if available_aliases.is_empty() {
                "no passing validation aliases are recorded".to_string()
            } else {
                format!(
                    "available passing aliases: {}",
                    available_aliases.join(", ")
                )
            };
            missing.push(format!(
                "missing successful '{}' validation evidence for task '{}' (record a passing validation run with alias '{}'; {})",
                required_validation.as_str(),
                task.id,
                required_validation.as_str(),
                available_detail
            ));
        }
    }

    missing
}

fn collect_missing_spec_completion_evidence(
    spec: &Spec,
    evidence_details: Option<&EvidenceBundleDetails>,
    successful_validations: &[ValidationRun],
    available_artifact_ids: Vec<&str>,
    completion_policy: &CompletionPolicy,
) -> Vec<String> {
    let mut missing = Vec::new();
    let evidence_bundle = evidence_details.map(|details| &details.bundle);

    if completion_policy.require_evidence_bundle && evidence_bundle.is_none() {
        missing.push(format!(
            "missing evidence bundle for spec '{}' (record a spec evidence bundle before completing the spec)",
            spec.id
        ));
    }

    if completion_policy.require_rationale {
        match evidence_bundle {
            Some(bundle) if bundle_has_summary(bundle) => {}
            Some(bundle) => missing.push(format!(
                "evidence bundle '{}' is missing a non-empty summary",
                bundle.id
            )),
            None if !completion_policy.require_evidence_bundle => missing.push(
                "missing completion summary evidence (record a non-empty spec evidence bundle summary)"
                    .to_string(),
            ),
            None => {}
        }
    }

    match evidence_details {
        Some(details) if !details.artifacts.is_empty() => {}
        Some(details) => {
            let available_detail = if available_artifact_ids.is_empty() {
                format!(
                    "no artifacts are registered for spec '{}' yet; register at least one artifact and link it to evidence bundle '{}'",
                    spec.id, details.bundle.id
                )
            } else {
                format!(
                    "link at least one registered artifact to evidence bundle '{}' (available spec artifacts: {})",
                    details.bundle.id,
                    available_artifact_ids.join(", ")
                )
            };
            missing.push(format!(
                "evidence bundle '{}' is missing artifact links ({})",
                details.bundle.id, available_detail
            ));
        }
        None => {
            let available_detail = if available_artifact_ids.is_empty() {
                format!(
                    "register at least one artifact for spec '{}' and link it from the spec evidence bundle",
                    spec.id
                )
            } else {
                format!(
                    "link at least one registered artifact from the future spec evidence bundle (available spec artifacts: {})",
                    available_artifact_ids.join(", ")
                )
            };
            missing.push(format!(
                "missing artifact links for spec completion ({})",
                available_detail
            ));
        }
    }

    if let Some(required_validation) = completion_policy.require_validation {
        if !successful_validations
            .iter()
            .any(|run| validation_alias_satisfies(&run.command_alias, required_validation))
        {
            let available_aliases = collect_successful_validation_aliases(successful_validations);
            let available_detail = if available_aliases.is_empty() {
                "no passing validation aliases are recorded".to_string()
            } else {
                format!(
                    "available passing aliases: {}",
                    available_aliases.join(", ")
                )
            };
            missing.push(format!(
                "missing successful '{}' validation evidence for spec '{}' (record a passing validation run with alias '{}'; {})",
                required_validation.as_str(),
                spec.id,
                required_validation.as_str(),
                available_detail
            ));
        }
    }

    missing
}

async fn get_evidence_bundle_details_for_entity(
    pool: &SqlitePool,
    evidence_ref: &EvidenceRef,
) -> Result<Option<EvidenceBundleDetails>> {
    let Some(bundle) = get_evidence_bundle_for_entity(pool, evidence_ref).await? else {
        return Ok(None);
    };
    get_evidence_bundle_details(pool, &bundle.id).await
}

fn build_completion_evidence(
    evidence_bundle: Option<&EvidenceBundle>,
    successful_validations: &[ValidationRun],
) -> CompletionEvidence {
    CompletionEvidence {
        has_evidence_bundle: evidence_bundle.is_some(),
        has_rationale: evidence_bundle.is_some_and(bundle_has_summary),
        satisfied_validation: highest_recorded_validation_level(successful_validations),
    }
}

fn highest_recorded_validation_level(
    successful_validations: &[ValidationRun],
) -> Option<ValidationRequirementLevel> {
    successful_validations
        .iter()
        .filter_map(|run| parse_validation_requirement_alias(&run.command_alias))
        .max_by_key(|level| validation_requirement_rank(*level))
}

fn append_completion_policy_issue(
    issues: &mut Vec<String>,
    entity_id: &str,
    entity_kind: &str,
    operation: RiskyOperationKind,
    evaluation: &RiskyOperationEvaluation,
) {
    match evaluation.outcome {
        RiskyOperationOutcome::Allowed => {}
        RiskyOperationOutcome::ApprovalRequired | RiskyOperationOutcome::Denied => {
            if let Some(message) =
                approval_gate_issue(entity_id, entity_kind, operation, evaluation)
            {
                issues.push(message);
            } else if issues.is_empty() {
                issues.push(evaluation.reason.clone());
            }
        }
    }
}

fn approval_gate_issue(
    entity_id: &str,
    entity_kind: &str,
    operation: RiskyOperationKind,
    evaluation: &RiskyOperationEvaluation,
) -> Option<String> {
    match evaluation.approval_state.as_ref()? {
        ApprovalState::NotRequested => Some(format!(
            "{} '{}' requires approval for '{}' (create an approval request before retrying completion)",
            entity_kind,
            entity_id,
            operation.as_str()
        )),
        ApprovalState::Pending(approval) => Some(format!(
            "{} '{}' is waiting on approval '{}' for '{}'",
            entity_kind,
            entity_id,
            approval.id,
            operation.as_str()
        )),
        ApprovalState::Rejected(approval) => Some(format!(
            "approval '{}' for {} '{}' was rejected{}",
            approval.id,
            entity_kind,
            entity_id,
            approval
                .decision_reason
                .as_deref()
                .map(|reason| format!(": {}", reason))
                .unwrap_or_default()
        )),
        ApprovalState::Cancelled(approval) => Some(format!(
            "approval '{}' for {} '{}' was cancelled",
            approval.id, entity_kind, entity_id
        )),
        ApprovalState::Expired(approval) => Some(format!(
            "approval '{}' for {} '{}' expired (request a new approval before retrying '{}')",
            approval.id,
            entity_kind,
            entity_id,
            operation.as_str()
        )),
        ApprovalState::Approved(_) => None,
    }
}

fn bundle_has_summary(bundle: &EvidenceBundle) -> bool {
    bundle
        .summary
        .as_deref()
        .is_some_and(|summary| !summary.trim().is_empty())
}

fn collect_successful_validation_aliases(successful_validations: &[ValidationRun]) -> Vec<String> {
    successful_validations
        .iter()
        .map(|run| run.command_alias.trim())
        .filter(|alias| !alias.is_empty())
        .map(str::to_string)
        .collect::<BTreeSet<_>>()
        .into_iter()
        .collect()
}

fn validation_alias_satisfies(alias: &str, required: ValidationRequirementLevel) -> bool {
    match (parse_validation_requirement_alias(alias), required) {
        (Some(ValidationRequirementLevel::Custom), ValidationRequirementLevel::Custom) => true,
        (Some(ValidationRequirementLevel::Custom), _) => false,
        (Some(actual), ValidationRequirementLevel::Custom) => {
            actual == ValidationRequirementLevel::Custom
        }
        (Some(actual), required) => {
            validation_requirement_rank(actual) >= validation_requirement_rank(required)
        }
        (None, _) => false,
    }
}

fn parse_validation_requirement_alias(alias: &str) -> Option<ValidationRequirementLevel> {
    match alias.trim() {
        "fast" => Some(ValidationRequirementLevel::Fast),
        "primary" => Some(ValidationRequirementLevel::Primary),
        "full" => Some(ValidationRequirementLevel::Full),
        "custom" => Some(ValidationRequirementLevel::Custom),
        _ => None,
    }
}

fn validation_requirement_rank(level: ValidationRequirementLevel) -> u8 {
    match level {
        ValidationRequirementLevel::Fast => 1,
        ValidationRequirementLevel::Primary => 2,
        ValidationRequirementLevel::Full => 3,
        ValidationRequirementLevel::Custom => 4,
    }
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
        artifact::register_artifact,
        event::query_events,
        evidence::{
            attach_artifact_to_evidence_bundle, attach_validation_run_to_evidence_bundle,
            create_evidence_bundle, record_validation_run, EvidenceArtifactRole,
            EvidenceBundleStatus, EvidenceRef, NewEvidenceBundle, RecordedValidationRun,
            ValidationCommandAlias, ValidationRunSource,
        },
        policy::{
            create_policy_config, ApprovalEntityKind, ApprovalStatus, EnforcementMode,
            PolicyScopeKind,
        },
        spec::{create_spec, update_spec_ac},
        task::create_task,
        test_helpers::make_pool,
    };
    use serde_json::json;

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

    async fn seed_governed_task(pool: &SqlitePool, spec_id: &str, task_id: &str) {
        create_spec(pool, spec_id, "Governed task spec", "P0", &[])
            .await
            .unwrap();
        approve_spec(pool, spec_id, "human", None).await.unwrap();
        start_spec(pool, spec_id, "human").await.unwrap();
        create_task(
            pool,
            task_id,
            spec_id,
            "Governed task",
            "builder",
            &[],
            None,
        )
        .await
        .unwrap();
        start_task(pool, task_id, "test-agent").await.unwrap();
    }

    async fn seed_approval_required_task(pool: &SqlitePool, spec_id: &str, task_id: &str) {
        seed_governed_task(pool, spec_id, task_id).await;
        let policy_id = format!("policy-{task_id}");
        create_policy_config(
            pool,
            crate::sdd::policy::CreatePolicyConfig {
                id: &policy_id,
                scope_kind: PolicyScopeKind::Task,
                scope_ref: task_id,
                agent: Some("builder"),
                enabled: true,
                enforcement_mode: EnforcementMode::Enforced,
                rules_json: &json!({
                    "task_completion": {
                        "require_approval": true,
                        "require_evidence_bundle": false,
                        "require_rationale": false,
                        "require_validation": "fast"
                    }
                }),
                rationale: Some("Human approval required for governed completions"),
                created_by: Some("architect"),
            },
        )
        .await
        .unwrap();
    }

    #[allow(clippy::too_many_arguments)]
    async fn record_task_validation(
        pool: &SqlitePool,
        spec_id: &str,
        task_id: &str,
        bundle_id: &str,
        validation_id: &str,
        alias: ValidationCommandAlias,
        command: &str,
        requirement: ValidationRequirementLevel,
    ) {
        let ran_at = Utc::now().to_rfc3339();
        record_validation_run(
            pool,
            RecordedValidationRun {
                id: validation_id,
                evidence_bundle_id: None,
                reference: EvidenceRef::for_task(spec_id, task_id),
                command_alias: alias,
                command,
                source: ValidationRunSource::Recorded,
                exit_code: Some(0),
                success: true,
                ran_at: &ran_at,
                recorded_by: Some("builder"),
                output_summary: Some("validation passed"),
                metadata_json: json!({"recorded_only": true}),
            },
        )
        .await
        .unwrap();
        attach_validation_run_to_evidence_bundle(pool, bundle_id, validation_id, requirement)
            .await
            .unwrap();
    }

    async fn record_spec_validation(
        pool: &SqlitePool,
        spec_id: &str,
        bundle_id: &str,
        validation_id: &str,
        alias: ValidationCommandAlias,
        command: &str,
        requirement: ValidationRequirementLevel,
    ) {
        let ran_at = Utc::now().to_rfc3339();
        record_validation_run(
            pool,
            RecordedValidationRun {
                id: validation_id,
                evidence_bundle_id: None,
                reference: EvidenceRef::for_spec(spec_id),
                command_alias: alias,
                command,
                source: ValidationRunSource::Recorded,
                exit_code: Some(0),
                success: true,
                ran_at: &ran_at,
                recorded_by: Some("builder"),
                output_summary: Some("validation passed"),
                metadata_json: json!({"recorded_only": true}),
            },
        )
        .await
        .unwrap();
        attach_validation_run_to_evidence_bundle(pool, bundle_id, validation_id, requirement)
            .await
            .unwrap();
    }

    async fn seed_spec_completion_candidate(pool: &SqlitePool, spec_id: &str, task_id: &str) {
        seed_governed_task(pool, spec_id, task_id).await;

        create_evidence_bundle(
            pool,
            NewEvidenceBundle {
                id: &format!("bundle-{task_id}"),
                reference: EvidenceRef::for_task(spec_id, task_id),
                status: EvidenceBundleStatus::Submitted,
                summary: Some("Task completion evidence"),
                behavior_change: true,
                metadata_json: json!({}),
                created_by: Some("builder"),
                updated_by: Some("builder"),
            },
        )
        .await
        .unwrap();
        record_task_validation(
            pool,
            spec_id,
            task_id,
            &format!("bundle-{task_id}"),
            &format!("validation-{task_id}"),
            ValidationCommandAlias::Primary,
            "cargo test --all-targets",
            ValidationRequirementLevel::Primary,
        )
        .await;
        complete_task(pool, task_id, "test-agent", None)
            .await
            .unwrap();
        update_spec_ac(pool, spec_id, 1, 1).await.unwrap();

        register_artifact(
            pool,
            &format!("artifact-{spec_id}"),
            Some(spec_id),
            Some(task_id),
            "builder",
            "source",
            Some("src/sdd/workflow.rs"),
            Some("Workflow implementation"),
            None,
        )
        .await
        .unwrap();
    }

    #[tokio::test]
    async fn governed_task_completion_blocks_when_evidence_bundle_is_missing() {
        let pool = make_pool().await;
        seed_governed_task(&pool, "SPEC-GOV-TASK-1", "TASK-GOV-1").await;

        let err = complete_task(&pool, "TASK-GOV-1", "test-agent", None)
            .await
            .unwrap_err();

        assert!(err.to_string().contains("missing evidence bundle"));
        assert!(err
            .to_string()
            .contains("missing successful 'primary' validation evidence"));
    }

    #[tokio::test]
    async fn governed_task_completion_blocks_when_bundle_summary_and_primary_validation_are_missing(
    ) {
        let pool = make_pool().await;
        seed_governed_task(&pool, "SPEC-GOV-TASK-2", "TASK-GOV-2").await;

        create_evidence_bundle(
            &pool,
            NewEvidenceBundle {
                id: "bundle-gov-2",
                reference: EvidenceRef::for_task("SPEC-GOV-TASK-2", "TASK-GOV-2"),
                status: EvidenceBundleStatus::Submitted,
                summary: Some("   "),
                behavior_change: false,
                metadata_json: json!({}),
                created_by: Some("builder"),
                updated_by: Some("builder"),
            },
        )
        .await
        .unwrap();

        let err = complete_task(&pool, "TASK-GOV-2", "test-agent", None)
            .await
            .unwrap_err();

        assert!(err
            .to_string()
            .contains("evidence bundle 'bundle-gov-2' is missing a non-empty summary"));
        assert!(err
            .to_string()
            .contains("missing successful 'primary' validation evidence"));
    }

    #[tokio::test]
    async fn governed_task_completion_allows_recorded_primary_validation_evidence() {
        let pool = make_pool().await;
        seed_governed_task(&pool, "SPEC-GOV-TASK-3", "TASK-GOV-3").await;

        create_evidence_bundle(
            &pool,
            NewEvidenceBundle {
                id: "bundle-gov-3",
                reference: EvidenceRef::for_task("SPEC-GOV-TASK-3", "TASK-GOV-3"),
                status: EvidenceBundleStatus::Submitted,
                summary: Some("Recorded evidence summary"),
                behavior_change: true,
                metadata_json: json!({"source": "test"}),
                created_by: Some("builder"),
                updated_by: Some("builder"),
            },
        )
        .await
        .unwrap();

        let ran_at = Utc::now().to_rfc3339();
        record_validation_run(
            &pool,
            RecordedValidationRun {
                id: "validation-gov-3",
                evidence_bundle_id: None,
                reference: EvidenceRef::for_task("SPEC-GOV-TASK-3", "TASK-GOV-3"),
                command_alias: ValidationCommandAlias::Primary,
                command: "cargo test --all-targets",
                source: ValidationRunSource::Recorded,
                exit_code: Some(0),
                success: true,
                ran_at: &ran_at,
                recorded_by: Some("builder"),
                output_summary: Some("all tests passed"),
                metadata_json: json!({"recorded_only": true}),
            },
        )
        .await
        .unwrap();
        attach_validation_run_to_evidence_bundle(
            &pool,
            "bundle-gov-3",
            "validation-gov-3",
            ValidationRequirementLevel::Primary,
        )
        .await
        .unwrap();

        let updated = complete_task(&pool, "TASK-GOV-3", "test-agent", None)
            .await
            .unwrap();

        assert_eq!(updated.status, "done");
        let events = query_events(
            &pool,
            Some("TaskCompleted"),
            Some("SPEC-GOV-TASK-3"),
            Some("builder"),
            None,
            None,
            None,
            None,
        )
        .await
        .unwrap();
        assert_eq!(events.len(), 1);
        assert_eq!(events[0].payload, r#"{"task":"TASK-GOV-3"}"#);
    }

    #[tokio::test]
    async fn governed_task_completion_surfaces_required_approval_and_allows_after_approval() {
        let pool = make_pool().await;
        seed_approval_required_task(&pool, "SPEC-GOV-TASK-4", "TASK-GOV-4").await;

        create_evidence_bundle(
            &pool,
            NewEvidenceBundle {
                id: "bundle-gov-4",
                reference: EvidenceRef::for_task("SPEC-GOV-TASK-4", "TASK-GOV-4"),
                status: EvidenceBundleStatus::Submitted,
                summary: Some("Task completion evidence"),
                behavior_change: false,
                metadata_json: json!({}),
                created_by: Some("builder"),
                updated_by: Some("builder"),
            },
        )
        .await
        .unwrap();
        record_task_validation(
            &pool,
            "SPEC-GOV-TASK-4",
            "TASK-GOV-4",
            "bundle-gov-4",
            "validation-gov-4",
            ValidationCommandAlias::Fast,
            "cargo clippy -- -D warnings",
            ValidationRequirementLevel::Fast,
        )
        .await;

        let err = complete_task(&pool, "TASK-GOV-4", "test-agent", None)
            .await
            .unwrap_err();
        assert!(err.to_string().contains("requires approval"));

        request_risky_operation_approval(
            &pool,
            RiskyOperationApprovalRequest {
                approval_id: "approval-gov-4",
                operation: RiskyOperationRequest {
                    spec_status: "in_progress",
                    spec_ref: Some("SPEC-GOV-TASK-4"),
                    task_ref: Some("TASK-GOV-4"),
                    agent: Some("builder"),
                    entity_kind: ApprovalEntityKind::Task,
                    entity_id: "TASK-GOV-4",
                    operation: RiskyOperationKind::CompleteTask,
                    write_path: None,
                    completion_evidence: Some(CompletionEvidence {
                        has_evidence_bundle: true,
                        has_rationale: true,
                        satisfied_validation: Some(ValidationRequirementLevel::Fast),
                    }),
                },
                requested_by: "builder",
                request_context_json: &json!({
                    "spec_status": "in_progress",
                    "agent": "builder",
                    "completion_evidence": {
                        "has_evidence_bundle": true,
                        "has_rationale": true,
                        "satisfied_validation": "fast"
                    }
                }),
                evidence_bundle_id: Some("bundle-gov-4"),
                expires_at: None,
            },
            None,
        )
        .await
        .unwrap();

        let pending = complete_task(&pool, "TASK-GOV-4", "test-agent", None)
            .await
            .unwrap_err();
        assert!(pending
            .to_string()
            .contains("waiting on approval 'approval-gov-4'"));

        decide_risky_operation_approval(
            &pool,
            RiskyOperationApprovalDecisionRequest {
                approval_id: "approval-gov-4",
                decision: ApprovalDecision {
                    status: ApprovalStatus::Approved,
                    decided_by: "reviewer",
                    decision_reason: Some("Looks good"),
                },
            },
        )
        .await
        .unwrap();

        let task = complete_task(&pool, "TASK-GOV-4", "test-agent", None)
            .await
            .unwrap();
        assert_eq!(task.status, "done");
    }

    #[tokio::test]
    async fn governed_spec_completion_reports_missing_bundle_artifacts_and_validation() {
        let pool = make_pool().await;
        seed_spec_completion_candidate(&pool, "SPEC-GOV-SPEC-1", "TASK-GOV-SPEC-1").await;

        let err = complete_spec(&pool, "SPEC-GOV-SPEC-1", "builder", None)
            .await
            .unwrap_err();

        let message = err.to_string();
        assert!(message.contains("missing evidence bundle for spec 'SPEC-GOV-SPEC-1'"));
        assert!(message.contains("missing artifact links for spec completion"));
        assert!(message.contains("missing successful 'full' validation evidence"));
    }

    #[tokio::test]
    async fn governed_spec_completion_requires_artifact_links_and_approval_before_allowing_done() {
        let pool = make_pool().await;
        seed_spec_completion_candidate(&pool, "SPEC-GOV-SPEC-2", "TASK-GOV-SPEC-2").await;
        create_policy_config(
            &pool,
            crate::sdd::policy::CreatePolicyConfig {
                id: "policy-spec-gov-2",
                scope_kind: PolicyScopeKind::Spec,
                scope_ref: "SPEC-GOV-SPEC-2",
                agent: Some("builder"),
                enabled: true,
                enforcement_mode: EnforcementMode::Enforced,
                rules_json: &json!({
                    "spec_completion": {
                        "require_approval": true
                    }
                }),
                rationale: Some("Manual approval required for spec completion"),
                created_by: Some("architect"),
            },
        )
        .await
        .unwrap();

        create_evidence_bundle(
            &pool,
            NewEvidenceBundle {
                id: "bundle-spec-gov-2",
                reference: EvidenceRef::for_spec("SPEC-GOV-SPEC-2"),
                status: EvidenceBundleStatus::Submitted,
                summary: Some("Spec completion evidence"),
                behavior_change: true,
                metadata_json: json!({}),
                created_by: Some("builder"),
                updated_by: Some("builder"),
            },
        )
        .await
        .unwrap();
        record_spec_validation(
            &pool,
            "SPEC-GOV-SPEC-2",
            "bundle-spec-gov-2",
            "validation-spec-gov-2",
            ValidationCommandAlias::Full,
            "cargo fmt --all -- --check && cargo clippy --all-targets --all-features -- -D warnings && cargo build --all-targets && cargo test --all-targets",
            ValidationRequirementLevel::Full,
        )
        .await;

        let missing_artifact_links = complete_spec(&pool, "SPEC-GOV-SPEC-2", "builder", None)
            .await
            .unwrap_err();
        assert!(missing_artifact_links
            .to_string()
            .contains("evidence bundle 'bundle-spec-gov-2' is missing artifact links"));

        attach_artifact_to_evidence_bundle(
            &pool,
            "bundle-spec-gov-2",
            "artifact-SPEC-GOV-SPEC-2",
            EvidenceArtifactRole::PrimaryOutput,
        )
        .await
        .unwrap();

        let approval_required = complete_spec(&pool, "SPEC-GOV-SPEC-2", "builder", None)
            .await
            .unwrap_err();
        assert!(approval_required.to_string().contains("requires approval"));

        request_risky_operation_approval(
            &pool,
            RiskyOperationApprovalRequest {
                approval_id: "approval-spec-gov-2",
                operation: RiskyOperationRequest {
                    spec_status: "in_progress",
                    spec_ref: Some("SPEC-GOV-SPEC-2"),
                    task_ref: None,
                    agent: Some("builder"),
                    entity_kind: ApprovalEntityKind::Spec,
                    entity_id: "SPEC-GOV-SPEC-2",
                    operation: RiskyOperationKind::CompleteSpec,
                    write_path: None,
                    completion_evidence: Some(CompletionEvidence {
                        has_evidence_bundle: true,
                        has_rationale: true,
                        satisfied_validation: Some(ValidationRequirementLevel::Full),
                    }),
                },
                requested_by: "builder",
                request_context_json: &json!({
                    "spec_status": "in_progress",
                    "agent": "builder",
                    "completion_evidence": {
                        "has_evidence_bundle": true,
                        "has_rationale": true,
                        "satisfied_validation": "full"
                    }
                }),
                evidence_bundle_id: Some("bundle-spec-gov-2"),
                expires_at: None,
            },
            None,
        )
        .await
        .unwrap();

        let pending = complete_spec(&pool, "SPEC-GOV-SPEC-2", "builder", None)
            .await
            .unwrap_err();
        assert!(pending
            .to_string()
            .contains("waiting on approval 'approval-spec-gov-2'"));

        decide_risky_operation_approval(
            &pool,
            RiskyOperationApprovalDecisionRequest {
                approval_id: "approval-spec-gov-2",
                decision: ApprovalDecision {
                    status: ApprovalStatus::Approved,
                    decided_by: "reviewer",
                    decision_reason: Some("Spec evidence looks complete"),
                },
            },
        )
        .await
        .unwrap();

        let spec = complete_spec(&pool, "SPEC-GOV-SPEC-2", "builder", None)
            .await
            .unwrap();
        assert_eq!(spec.status, "done");
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

        let updated = approve_spec(&pool, "SPEC-WRAP-1", "human", None)
            .await
            .unwrap();

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

        let updated = start_task(&pool, "TASK-WRAP-1", "test-agent")
            .await
            .unwrap();

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
        register_artifact(
            &pool,
            "artifact-spec-tx-done",
            Some("SPEC-TX-DONE"),
            None,
            "human",
            "source",
            Some("src/sdd/workflow.rs"),
            Some("Spec done evidence artifact"),
            None,
        )
        .await
        .unwrap();
        create_evidence_bundle(
            &pool,
            NewEvidenceBundle {
                id: "bundle-spec-tx-done",
                reference: EvidenceRef::for_spec("SPEC-TX-DONE"),
                status: EvidenceBundleStatus::Submitted,
                summary: Some("Spec completion evidence"),
                behavior_change: false,
                metadata_json: json!({}),
                created_by: Some("human"),
                updated_by: Some("human"),
            },
        )
        .await
        .unwrap();
        attach_artifact_to_evidence_bundle(
            &pool,
            "bundle-spec-tx-done",
            "artifact-spec-tx-done",
            EvidenceArtifactRole::PrimaryOutput,
        )
        .await
        .unwrap();
        record_spec_validation(
            &pool,
            "SPEC-TX-DONE",
            "bundle-spec-tx-done",
            "validation-spec-tx-done",
            ValidationCommandAlias::Full,
            "cargo fmt --all -- --check && cargo clippy --all-targets --all-features -- -D warnings && cargo build --all-targets && cargo test --all-targets",
            ValidationRequirementLevel::Full,
        )
        .await;

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
    async fn risky_operation_approval_request_creates_pending_record_and_audit_event() {
        let pool = make_pool().await;
        seed_approval_required_task(&pool, "SPEC-APPROVAL-1", "TASK-APPROVAL-1").await;

        let result = request_risky_operation_approval(
            &pool,
            RiskyOperationApprovalRequest {
                approval_id: "approval-workflow-1",
                operation: RiskyOperationRequest {
                    spec_status: "in_progress",
                    spec_ref: Some("SPEC-APPROVAL-1"),
                    task_ref: Some("TASK-APPROVAL-1"),
                    agent: Some("builder"),
                    entity_kind: ApprovalEntityKind::Task,
                    entity_id: "TASK-APPROVAL-1",
                    operation: RiskyOperationKind::CompleteTask,
                    write_path: None,
                    completion_evidence: Some(CompletionEvidence {
                        has_evidence_bundle: false,
                        has_rationale: false,
                        satisfied_validation: Some(ValidationRequirementLevel::Fast),
                    }),
                },
                requested_by: "builder",
                request_context_json: &json!({
                    "spec_status": "in_progress",
                    "agent": "builder",
                    "completion_evidence": {
                        "has_evidence_bundle": false,
                        "has_rationale": false,
                        "satisfied_validation": "fast"
                    }
                }),
                evidence_bundle_id: None,
                expires_at: None,
            },
            None,
        )
        .await
        .unwrap();

        assert!(result.created);
        assert_eq!(result.approval.status, ApprovalStatus::Pending);
        assert_eq!(
            result.evaluation.outcome,
            RiskyOperationOutcome::ApprovalRequired
        );
        assert!(matches!(
            result.evaluation.approval_state,
            Some(ApprovalState::Pending(_))
        ));

        let events = query_events(
            &pool,
            Some(POLICY_APPROVAL_REQUESTED_EVENT),
            Some("SPEC-APPROVAL-1"),
            Some("builder"),
            None,
            None,
            None,
            None,
        )
        .await
        .unwrap();
        assert_eq!(events.len(), 1);
        assert!(events[0].payload.contains("approval-workflow-1"));

        let audit_refs = sqlx::query_scalar::<_, i64>(
            "SELECT COUNT(1) FROM policy_audit_refs WHERE event_id = ?",
        )
        .bind(events[0].id)
        .fetch_one(&pool)
        .await
        .unwrap();
        assert_eq!(audit_refs, 4);
    }

    #[tokio::test]
    async fn risky_operation_approval_request_reuses_existing_pending_record() {
        let pool = make_pool().await;
        seed_approval_required_task(&pool, "SPEC-APPROVAL-2", "TASK-APPROVAL-2").await;

        let request = RiskyOperationApprovalRequest {
            approval_id: "approval-workflow-2",
            operation: RiskyOperationRequest {
                spec_status: "in_progress",
                spec_ref: Some("SPEC-APPROVAL-2"),
                task_ref: Some("TASK-APPROVAL-2"),
                agent: Some("builder"),
                entity_kind: ApprovalEntityKind::Task,
                entity_id: "TASK-APPROVAL-2",
                operation: RiskyOperationKind::CompleteTask,
                write_path: None,
                completion_evidence: Some(CompletionEvidence {
                    has_evidence_bundle: false,
                    has_rationale: false,
                    satisfied_validation: Some(ValidationRequirementLevel::Fast),
                }),
            },
            requested_by: "builder",
            request_context_json: &json!({
                "spec_status": "in_progress",
                "agent": "builder",
                "completion_evidence": {
                    "has_evidence_bundle": false,
                    "has_rationale": false,
                    "satisfied_validation": "fast"
                }
            }),
            evidence_bundle_id: None,
            expires_at: None,
        };

        request_risky_operation_approval(&pool, request.clone(), None)
            .await
            .unwrap();
        let second = request_risky_operation_approval(&pool, request, None)
            .await
            .unwrap();

        assert!(!second.created);
        assert_eq!(second.approval.id, "approval-workflow-2");

        let count =
            sqlx::query_scalar::<_, i64>("SELECT COUNT(1) FROM approvals WHERE entity_id = ?")
                .bind("TASK-APPROVAL-2")
                .fetch_one(&pool)
                .await
                .unwrap();
        assert_eq!(count, 1);
    }

    #[tokio::test]
    async fn risky_operation_approval_decision_rechecks_policy_outcome() {
        let pool = make_pool().await;
        seed_approval_required_task(&pool, "SPEC-APPROVAL-3", "TASK-APPROVAL-3").await;

        request_risky_operation_approval(
            &pool,
            RiskyOperationApprovalRequest {
                approval_id: "approval-workflow-3",
                operation: RiskyOperationRequest {
                    spec_status: "in_progress",
                    spec_ref: Some("SPEC-APPROVAL-3"),
                    task_ref: Some("TASK-APPROVAL-3"),
                    agent: Some("builder"),
                    entity_kind: ApprovalEntityKind::Task,
                    entity_id: "TASK-APPROVAL-3",
                    operation: RiskyOperationKind::CompleteTask,
                    write_path: None,
                    completion_evidence: Some(CompletionEvidence {
                        has_evidence_bundle: false,
                        has_rationale: false,
                        satisfied_validation: Some(ValidationRequirementLevel::Fast),
                    }),
                },
                requested_by: "builder",
                request_context_json: &json!({
                    "spec_status": "in_progress",
                    "agent": "builder",
                    "completion_evidence": {
                        "has_evidence_bundle": false,
                        "has_rationale": false,
                        "satisfied_validation": "fast"
                    }
                }),
                evidence_bundle_id: None,
                expires_at: None,
            },
            None,
        )
        .await
        .unwrap();

        let approved = decide_risky_operation_approval(
            &pool,
            RiskyOperationApprovalDecisionRequest {
                approval_id: "approval-workflow-3",
                decision: ApprovalDecision {
                    status: ApprovalStatus::Approved,
                    decided_by: "reviewer",
                    decision_reason: Some("Validated manually"),
                },
            },
        )
        .await
        .unwrap();

        assert_eq!(approved.approval.status, ApprovalStatus::Approved);
        assert_eq!(approved.evaluation.outcome, RiskyOperationOutcome::Allowed);
        assert!(matches!(
            approved.evaluation.approval_state,
            Some(ApprovalState::Approved(_))
        ));

        let decision_events = query_events(
            &pool,
            Some(POLICY_APPROVAL_DECIDED_EVENT),
            Some("SPEC-APPROVAL-3"),
            Some("reviewer"),
            None,
            None,
            None,
            None,
        )
        .await
        .unwrap();
        assert_eq!(decision_events.len(), 1);
        assert!(decision_events[0].payload.contains("approved"));

        let rejected_task = "TASK-APPROVAL-4";
        seed_approval_required_task(&pool, "SPEC-APPROVAL-4", rejected_task).await;
        request_risky_operation_approval(
            &pool,
            RiskyOperationApprovalRequest {
                approval_id: "approval-workflow-4",
                operation: RiskyOperationRequest {
                    spec_status: "in_progress",
                    spec_ref: Some("SPEC-APPROVAL-4"),
                    task_ref: Some(rejected_task),
                    agent: Some("builder"),
                    entity_kind: ApprovalEntityKind::Task,
                    entity_id: rejected_task,
                    operation: RiskyOperationKind::CompleteTask,
                    write_path: None,
                    completion_evidence: Some(CompletionEvidence {
                        has_evidence_bundle: false,
                        has_rationale: false,
                        satisfied_validation: Some(ValidationRequirementLevel::Fast),
                    }),
                },
                requested_by: "builder",
                request_context_json: &json!({
                    "spec_status": "in_progress",
                    "agent": "builder",
                    "completion_evidence": {
                        "has_evidence_bundle": false,
                        "has_rationale": false,
                        "satisfied_validation": "fast"
                    }
                }),
                evidence_bundle_id: None,
                expires_at: None,
            },
            None,
        )
        .await
        .unwrap();

        let rejected = decide_risky_operation_approval(
            &pool,
            RiskyOperationApprovalDecisionRequest {
                approval_id: "approval-workflow-4",
                decision: ApprovalDecision {
                    status: ApprovalStatus::Rejected,
                    decided_by: "reviewer",
                    decision_reason: Some("Need more evidence"),
                },
            },
        )
        .await
        .unwrap();

        assert_eq!(rejected.approval.status, ApprovalStatus::Rejected);
        assert_eq!(rejected.evaluation.outcome, RiskyOperationOutcome::Denied);
        assert!(matches!(
            rejected.evaluation.approval_state,
            Some(ApprovalState::Rejected(_))
        ));
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
