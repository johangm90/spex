#![allow(dead_code)]

#[path = "../src/config.rs"]
mod config;
#[path = "../src/sdd/mod.rs"]
mod sdd;
#[path = "../src/webhooks.rs"]
mod webhooks;

use sdd::{
    artifact::register_artifact,
    event::query_events,
    evidence::{
        attach_artifact_to_evidence_bundle, attach_validation_run_to_evidence_bundle,
        create_evidence_bundle, record_validation_run, EvidenceArtifactRole, EvidenceBundleStatus,
        EvidenceRef, NewEvidenceBundle, RecordedValidationRun, ValidationCommandAlias,
        ValidationRequirementLevel, ValidationRunSource,
    },
    spec::{create_spec, get_spec, update_spec_ac},
    task::{create_task, get_task},
    workflow::{
        apply_spec_status_update_with_event_test_hook,
        apply_task_status_update_with_event_test_hook, approve_spec, complete_spec, complete_task,
        fail_task, start_spec, start_task, validate_spec_transition, validate_task_transition,
        LifecycleEvent, LifecycleKind,
    },
};
use serde_json::json;
use sqlx::SqlitePool;

async fn make_pool() -> SqlitePool {
    let pool = SqlitePool::connect(":memory:")
        .await
        .expect("failed to open in-memory SQLite");
    sqlx::migrate!("./migrations")
        .run(&pool)
        .await
        .expect("failed to run migrations");
    pool
}

async fn record_task_completion_evidence(pool: &SqlitePool, spec_id: &str, task_id: &str) {
    create_evidence_bundle(
        pool,
        NewEvidenceBundle {
            id: "bundle-workflow-test",
            reference: EvidenceRef::for_task(spec_id, task_id),
            status: EvidenceBundleStatus::Submitted,
            summary: Some("Recorded workflow evidence"),
            behavior_change: false,
            metadata_json: json!({}),
            created_by: Some("builder"),
            updated_by: Some("builder"),
        },
    )
    .await
    .unwrap();
    let ran_at = chrono::Utc::now().to_rfc3339();
    record_validation_run(
        pool,
        RecordedValidationRun {
            id: "validation-workflow-test",
            evidence_bundle_id: None,
            reference: EvidenceRef::for_task(spec_id, task_id),
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
        pool,
        "bundle-workflow-test",
        "validation-workflow-test",
        ValidationRequirementLevel::Primary,
    )
    .await
    .unwrap();
}

async fn record_spec_completion_evidence(pool: &SqlitePool, spec_id: &str, task_id: &str) {
    register_artifact(
        pool,
        "artifact-workflow-spec",
        Some(spec_id),
        Some(task_id),
        "builder",
        "source",
        Some("src/sdd/workflow.rs"),
        Some("Workflow invariant artifact"),
        None,
    )
    .await
    .unwrap();
    create_evidence_bundle(
        pool,
        NewEvidenceBundle {
            id: "bundle-workflow-spec",
            reference: EvidenceRef::for_spec(spec_id),
            status: EvidenceBundleStatus::Submitted,
            summary: Some("Recorded workflow spec evidence"),
            behavior_change: false,
            metadata_json: json!({}),
            created_by: Some("builder"),
            updated_by: Some("builder"),
        },
    )
    .await
    .unwrap();
    attach_artifact_to_evidence_bundle(
        pool,
        "bundle-workflow-spec",
        "artifact-workflow-spec",
        EvidenceArtifactRole::PrimaryOutput,
    )
    .await
    .unwrap();
    let ran_at = chrono::Utc::now().to_rfc3339();
    record_validation_run(
        pool,
        RecordedValidationRun {
            id: "validation-workflow-spec",
            evidence_bundle_id: None,
            reference: EvidenceRef::for_spec(spec_id),
            command_alias: ValidationCommandAlias::Full,
            command: "cargo fmt --all -- --check && cargo clippy --all-targets --all-features -- -D warnings && cargo build --all-targets && cargo test --all-targets",
            source: ValidationRunSource::Recorded,
            exit_code: Some(0),
            success: true,
            ran_at: &ran_at,
            recorded_by: Some("builder"),
            output_summary: Some("all checks passed"),
            metadata_json: json!({"recorded_only": true}),
        },
    )
    .await
    .unwrap();
    attach_validation_run_to_evidence_bundle(
        pool,
        "bundle-workflow-spec",
        "validation-workflow-spec",
        ValidationRequirementLevel::Full,
    )
    .await
    .unwrap();
}

#[test]
fn valid_lifecycle_transitions_are_accepted() {
    let spec_transition = validate_spec_transition("approved", "in_progress").unwrap();
    let task_transition = validate_task_transition("failed", "pending").unwrap();

    assert_eq!(spec_transition.kind, LifecycleKind::Spec);
    assert_eq!(spec_transition.from_status, "approved");
    assert_eq!(spec_transition.to_status, "in_progress");
    assert_eq!(task_transition.kind, LifecycleKind::Task);
    assert_eq!(task_transition.from_status, "failed");
    assert_eq!(task_transition.to_status, "pending");
}

#[test]
fn invalid_lifecycle_transitions_are_rejected() {
    let spec_err = validate_spec_transition("draft", "done").unwrap_err();
    let task_err = validate_task_transition("pending", "done").unwrap_err();

    assert!(spec_err
        .to_string()
        .contains("Invalid transition: draft -> done"));
    assert!(task_err
        .to_string()
        .contains("Invalid task transition: pending -> done"));
}

#[tokio::test]
async fn spec_completion_is_blocked_when_tasks_remain_open() {
    let pool = make_pool().await;
    create_spec(&pool, "SPEC-WI-OPEN", "Workflow invariant open", "P0", &[])
        .await
        .unwrap();
    approve_spec(&pool, "SPEC-WI-OPEN", "human", None)
        .await
        .unwrap();
    start_spec(&pool, "SPEC-WI-OPEN", "human").await.unwrap();
    create_task(
        &pool,
        "TASK-WI-OPEN",
        "SPEC-WI-OPEN",
        "Still open",
        "builder",
        &[],
        None,
    )
    .await
    .unwrap();
    update_spec_ac(&pool, "SPEC-WI-OPEN", 1, 1).await.unwrap();

    let err = complete_spec(&pool, "SPEC-WI-OPEN", "human", None)
        .await
        .unwrap_err();

    assert!(err.to_string().contains("task(s) are still open"));
    assert!(err.to_string().contains("TASK-WI-OPEN"));
    assert_eq!(
        get_spec(&pool, "SPEC-WI-OPEN")
            .await
            .unwrap()
            .unwrap()
            .status,
        "in_progress"
    );
    assert!(query_events(
        &pool,
        Some("SpecCompleted"),
        Some("SPEC-WI-OPEN"),
        None,
        None,
        None,
        None,
        None,
    )
    .await
    .unwrap()
    .is_empty());
}

#[tokio::test]
async fn spec_completion_succeeds_when_done_conditions_are_met() {
    let pool = make_pool().await;
    create_spec(&pool, "SPEC-WI-DONE", "Workflow invariant done", "P0", &[])
        .await
        .unwrap();
    approve_spec(&pool, "SPEC-WI-DONE", "human", None)
        .await
        .unwrap();
    start_spec(&pool, "SPEC-WI-DONE", "human").await.unwrap();
    create_task(
        &pool,
        "TASK-WI-DONE",
        "SPEC-WI-DONE",
        "Closable task",
        "builder",
        &[],
        None,
    )
    .await
    .unwrap();
    start_task(&pool, "TASK-WI-DONE", "test-agent")
        .await
        .unwrap();
    record_task_completion_evidence(&pool, "SPEC-WI-DONE", "TASK-WI-DONE").await;
    complete_task(&pool, "TASK-WI-DONE", "test-agent", None)
        .await
        .unwrap();
    update_spec_ac(&pool, "SPEC-WI-DONE", 2, 2).await.unwrap();
    record_spec_completion_evidence(&pool, "SPEC-WI-DONE", "TASK-WI-DONE").await;

    let spec = complete_spec(&pool, "SPEC-WI-DONE", "human", None)
        .await
        .unwrap();

    assert_eq!(spec.status, "done");
    let events = query_events(
        &pool,
        Some("SpecCompleted"),
        Some("SPEC-WI-DONE"),
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
async fn transactional_spec_helper_rolls_back_without_partial_persistence() {
    let pool = make_pool().await;
    create_spec(
        &pool,
        "SPEC-WI-ROLLBACK",
        "Workflow invariant rollback",
        "P0",
        &[],
    )
    .await
    .unwrap();

    let err = apply_spec_status_update_with_event_test_hook(
        &pool,
        "SPEC-WI-ROLLBACK",
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
    assert_eq!(
        get_spec(&pool, "SPEC-WI-ROLLBACK")
            .await
            .unwrap()
            .unwrap()
            .status,
        "draft"
    );
    assert!(query_events(
        &pool,
        Some("SpecApproved"),
        Some("SPEC-WI-ROLLBACK"),
        None,
        None,
        None,
        None,
        None,
    )
    .await
    .unwrap()
    .is_empty());
}

#[tokio::test]
async fn transactional_task_helper_rolls_back_without_partial_persistence() {
    let pool = make_pool().await;
    create_spec(
        &pool,
        "SPEC-WI-TASK-ROLLBACK",
        "Workflow invariant task rollback",
        "P0",
        &[],
    )
    .await
    .unwrap();
    create_task(
        &pool,
        "TASK-WI-ROLLBACK",
        "SPEC-WI-TASK-ROLLBACK",
        "Rollback task",
        "builder",
        &[],
        None,
    )
    .await
    .unwrap();

    let err = apply_task_status_update_with_event_test_hook(
        &pool,
        "TASK-WI-ROLLBACK",
        "in_progress",
        LifecycleEvent {
            event_type: "TaskStarted",
            payload_json: r#"{"task":"TASK-WI-ROLLBACK"}"#,
        },
        true,
    )
    .await
    .unwrap_err();

    assert!(err
        .to_string()
        .contains("Injected lifecycle event persistence failure"));
    assert_eq!(
        get_task(&pool, "TASK-WI-ROLLBACK")
            .await
            .unwrap()
            .unwrap()
            .status,
        "pending"
    );
    assert!(query_events(
        &pool,
        Some("TaskStarted"),
        Some("SPEC-WI-TASK-ROLLBACK"),
        None,
        None,
        None,
        None,
        None,
    )
    .await
    .unwrap()
    .is_empty());
}

#[tokio::test]
async fn task_failure_and_recovery_paths_remain_valid_domain_transitions() {
    let pool = make_pool().await;
    create_spec(&pool, "SPEC-WI-TASK", "Workflow invariant task", "P0", &[])
        .await
        .unwrap();
    approve_spec(&pool, "SPEC-WI-TASK", "human", None)
        .await
        .unwrap();
    start_spec(&pool, "SPEC-WI-TASK", "human").await.unwrap();
    create_task(
        &pool,
        "TASK-WI-TASK",
        "SPEC-WI-TASK",
        "Task lifecycle",
        "builder",
        &[],
        None,
    )
    .await
    .unwrap();

    start_task(&pool, "TASK-WI-TASK", "test-agent")
        .await
        .unwrap();
    let failed = fail_task(&pool, "TASK-WI-TASK").await.unwrap();
    assert_eq!(failed.status, "failed");

    let replanned = sdd::task::update_task_status(&pool, "TASK-WI-TASK", "pending")
        .await
        .unwrap();
    assert_eq!(replanned.status, "pending");

    let replan_events = query_events(
        &pool,
        Some("TaskReplanned"),
        Some("SPEC-WI-TASK"),
        None,
        None,
        None,
        None,
        None,
    )
    .await
    .unwrap();
    assert_eq!(replan_events.len(), 1);
}
