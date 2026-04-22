#![allow(dead_code)]

#[path = "../src/config.rs"]
mod config;
#[path = "../src/sdd/mod.rs"]
mod sdd;
#[path = "../src/webhooks.rs"]
mod webhooks;

use sdd::{
    evidence::{
        create_evidence_bundle, get_evidence_bundle_for_entity, EvidenceBundleStatus, EvidenceRef,
        NewEvidenceBundle, RecordedValidationRun, ValidationCommandAlias, ValidationRunSource,
    },
    policy::{
        create_approval, create_policy_config, decide_approval, resolve_effective_policy,
        ApprovalDecision, ApprovalEntityKind, ApprovalStatus, CreateApproval, CreatePolicyConfig,
        EnforcementMode, PolicyScopeKind, PROJECT_SCOPE_REF,
    },
    spec::{create_spec, update_spec_ac},
    task::create_task,
    workflow::{approve_spec, complete_spec, complete_task, start_spec, start_task},
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

// ─── Policy resolution ────────────────────────────────────────────────────────

#[tokio::test]
async fn default_policy_has_no_requirements() {
    let pool = make_pool().await;
    // Use a draft spec so fail_closed = false → relaxed defaults
    let policy = resolve_effective_policy(&pool, Some("SPEC-X"), Some("T-X"), None, "draft")
        .await
        .unwrap();

    assert!(!policy.task_completion.require_evidence_bundle);
    assert!(!policy.task_completion.require_approval);
    assert!(!policy.spec_completion.require_evidence_bundle);
    assert!(!policy.spec_completion.require_approval);
}

#[tokio::test]
async fn spec_level_policy_applies_to_tasks_under_that_spec() {
    let pool = make_pool().await;
    create_policy_config(
        &pool,
        CreatePolicyConfig {
            id: "pcfg-spec-level",
            scope_kind: PolicyScopeKind::Spec,
            scope_ref: "SPEC-PE-001",
            agent: None,
            enabled: true,
            enforcement_mode: EnforcementMode::Enforced,
            rules_json: &json!({"require_evidence_bundle": true}),
            rationale: None,
            created_by: Some("test"),
        },
    )
    .await
    .unwrap();

    // Resolve for a task under that spec (approved → fail_closed = true)
    let policy = resolve_effective_policy(
        &pool,
        Some("SPEC-PE-001"),
        Some("T-PE-001"),
        None,
        "in_progress",
    )
    .await
    .unwrap();

    assert!(policy.task_completion.require_evidence_bundle);
}

#[tokio::test]
async fn task_level_policy_overrides_spec_level_policy() {
    let pool = make_pool().await;
    // Spec-level: require evidence
    create_policy_config(
        &pool,
        CreatePolicyConfig {
            id: "pcfg-spec-override",
            scope_kind: PolicyScopeKind::Spec,
            scope_ref: "SPEC-PE-002",
            agent: None,
            enabled: true,
            enforcement_mode: EnforcementMode::Enforced,
            rules_json: &json!({"require_evidence_bundle": true}),
            rationale: None,
            created_by: Some("test"),
        },
    )
    .await
    .unwrap();
    // Task-level: override to NOT require evidence
    create_policy_config(
        &pool,
        CreatePolicyConfig {
            id: "pcfg-task-override",
            scope_kind: PolicyScopeKind::Task,
            scope_ref: "T-PE-002",
            agent: None,
            enabled: true,
            enforcement_mode: EnforcementMode::Enforced,
            rules_json: &json!({"require_evidence_bundle": false}),
            rationale: None,
            created_by: Some("test"),
        },
    )
    .await
    .unwrap();

    let policy = resolve_effective_policy(
        &pool,
        Some("SPEC-PE-002"),
        Some("T-PE-002"),
        None,
        "in_progress",
    )
    .await
    .unwrap();

    // Task-level overlay applied last → overrides spec-level
    assert!(!policy.task_completion.require_evidence_bundle);
}

#[tokio::test]
async fn agent_level_policy_is_independent_of_spec_and_task() {
    let pool = make_pool().await;
    // Agent-scoped policy at project level
    create_policy_config(
        &pool,
        CreatePolicyConfig {
            id: "pcfg-agent-level",
            scope_kind: PolicyScopeKind::Project,
            scope_ref: PROJECT_SCOPE_REF,
            agent: Some("sdd-builder"),
            enabled: true,
            enforcement_mode: EnforcementMode::Enforced,
            rules_json: &json!({"require_approval": true}),
            rationale: None,
            created_by: Some("test"),
        },
    )
    .await
    .unwrap();

    // Resolving for a different agent should NOT pick up the agent-scoped policy
    let policy_other_agent = resolve_effective_policy(
        &pool,
        Some("SPEC-PE-003"),
        Some("T-PE-003"),
        Some("other-agent"),
        "in_progress",
    )
    .await
    .unwrap();
    assert!(!policy_other_agent.task_completion.require_approval);

    // Resolving for the matching agent SHOULD pick it up
    let policy_builder = resolve_effective_policy(
        &pool,
        Some("SPEC-PE-003"),
        Some("T-PE-003"),
        Some("sdd-builder"),
        "in_progress",
    )
    .await
    .unwrap();
    assert!(policy_builder.task_completion.require_approval);
}

// ─── Evidence ─────────────────────────────────────────────────────────────────

#[tokio::test]
async fn submit_evidence_for_task_can_be_retrieved() {
    let pool = make_pool().await;
    create_spec(&pool, "SPEC-EV-001", "Evidence spec", "P0", &[])
        .await
        .unwrap();
    create_task(
        &pool,
        "T-EV-001",
        "SPEC-EV-001",
        "Evidence task",
        "builder",
        &[],
        None,
    )
    .await
    .unwrap();
    create_evidence_bundle(
        &pool,
        NewEvidenceBundle {
            id: "bundle-task-1",
            reference: EvidenceRef::for_task("SPEC-EV-001", "T-EV-001"),
            status: EvidenceBundleStatus::Submitted,
            summary: Some("Task evidence"),
            behavior_change: false,
            metadata_json: json!({}),
            created_by: Some("builder"),
            updated_by: Some("builder"),
        },
    )
    .await
    .unwrap();

    let bundle =
        get_evidence_bundle_for_entity(&pool, &EvidenceRef::for_task("SPEC-EV-001", "T-EV-001"))
            .await
            .unwrap();

    assert!(bundle.is_some());
    assert_eq!(bundle.unwrap().id, "bundle-task-1");
}

#[tokio::test]
async fn submit_evidence_for_spec_can_be_retrieved() {
    let pool = make_pool().await;
    create_spec(&pool, "SPEC-EV-002", "Evidence spec 2", "P0", &[])
        .await
        .unwrap();
    create_evidence_bundle(
        &pool,
        NewEvidenceBundle {
            id: "bundle-spec-1",
            reference: EvidenceRef::for_spec("SPEC-EV-002"),
            status: EvidenceBundleStatus::Submitted,
            summary: Some("Spec evidence"),
            behavior_change: false,
            metadata_json: json!({}),
            created_by: Some("builder"),
            updated_by: Some("builder"),
        },
    )
    .await
    .unwrap();

    let bundle = get_evidence_bundle_for_entity(&pool, &EvidenceRef::for_spec("SPEC-EV-002"))
        .await
        .unwrap();

    assert!(bundle.is_some());
    assert_eq!(bundle.unwrap().id, "bundle-spec-1");
}

#[tokio::test]
async fn evidence_with_passed_false_is_stored_correctly() {
    let pool = make_pool().await;
    create_spec(&pool, "SPEC-EV-003", "Evidence spec 3", "P0", &[])
        .await
        .unwrap();
    create_task(
        &pool,
        "T-EV-003",
        "SPEC-EV-003",
        "Evidence task 3",
        "builder",
        &[],
        None,
    )
    .await
    .unwrap();
    // Record a failed validation run (success=false)
    let ran_at = chrono::Utc::now().to_rfc3339();
    sdd::evidence::record_validation_run(
        &pool,
        RecordedValidationRun {
            id: "vrun-failed",
            evidence_bundle_id: None,
            reference: EvidenceRef::for_task("SPEC-EV-003", "T-EV-003"),
            command_alias: ValidationCommandAlias::Primary,
            command: "cargo test",
            source: ValidationRunSource::Recorded,
            exit_code: Some(1),
            success: false,
            ran_at: &ran_at,
            recorded_by: Some("builder"),
            output_summary: Some("tests failed"),
            metadata_json: json!({}),
        },
    )
    .await
    .unwrap();

    let runs = sdd::evidence::list_validation_runs(
        &pool,
        Some("SPEC-EV-003"),
        Some("T-EV-003"),
        None,
        None,
    )
    .await
    .unwrap();

    assert_eq!(runs.len(), 1);
    assert!(!runs[0].success);
}

#[tokio::test]
async fn multiple_evidence_entries_for_same_task_all_returned() {
    let pool = make_pool().await;
    create_spec(&pool, "SPEC-EV-004", "Evidence spec 4", "P0", &[])
        .await
        .unwrap();
    create_task(
        &pool,
        "T-EV-004",
        "SPEC-EV-004",
        "Evidence task 4",
        "builder",
        &[],
        None,
    )
    .await
    .unwrap();
    let ran_at = chrono::Utc::now().to_rfc3339();
    for i in 0..3u8 {
        sdd::evidence::record_validation_run(
            &pool,
            RecordedValidationRun {
                id: &format!("vrun-multi-{}", i),
                evidence_bundle_id: None,
                reference: EvidenceRef::for_task("SPEC-EV-004", "T-EV-004"),
                command_alias: ValidationCommandAlias::Primary,
                command: "cargo test",
                source: ValidationRunSource::Recorded,
                exit_code: Some(0),
                success: true,
                ran_at: &ran_at,
                recorded_by: Some("builder"),
                output_summary: None,
                metadata_json: json!({}),
            },
        )
        .await
        .unwrap();
    }

    let runs = sdd::evidence::list_validation_runs(
        &pool,
        Some("SPEC-EV-004"),
        Some("T-EV-004"),
        None,
        None,
    )
    .await
    .unwrap();

    assert_eq!(runs.len(), 3);
}

// ─── Approval workflow ────────────────────────────────────────────────────────

#[tokio::test]
async fn request_approval_status_is_pending() {
    let pool = make_pool().await;
    let approval = create_approval(
        &pool,
        CreateApproval {
            id: "appr-pending-1",
            entity_kind: ApprovalEntityKind::Operation,
            entity_id: "appr-pending-1",
            spec: None,
            task: None,
            operation_kind: "complete_task",
            policy_config_id: None,
            evidence_bundle_id: None,
            requested_by: "builder",
            request_context_json: &json!({}),
            expires_at: None,
        },
    )
    .await
    .unwrap();

    assert_eq!(approval.status, ApprovalStatus::Pending);
}

#[tokio::test]
async fn approve_pending_request_status_becomes_approved() {
    let pool = make_pool().await;
    create_approval(
        &pool,
        CreateApproval {
            id: "appr-approve-1",
            entity_kind: ApprovalEntityKind::Operation,
            entity_id: "appr-approve-1",
            spec: None,
            task: None,
            operation_kind: "complete_task",
            policy_config_id: None,
            evidence_bundle_id: None,
            requested_by: "builder",
            request_context_json: &json!({}),
            expires_at: None,
        },
    )
    .await
    .unwrap();

    let decided = decide_approval(
        &pool,
        "appr-approve-1",
        ApprovalDecision {
            status: ApprovalStatus::Approved,
            decided_by: "human",
            decision_reason: Some("LGTM"),
        },
    )
    .await
    .unwrap();

    assert_eq!(decided.status, ApprovalStatus::Approved);
    assert_eq!(decided.decided_by.as_deref(), Some("human"));
}

#[tokio::test]
async fn reject_pending_request_status_becomes_rejected() {
    let pool = make_pool().await;
    create_approval(
        &pool,
        CreateApproval {
            id: "appr-reject-1",
            entity_kind: ApprovalEntityKind::Operation,
            entity_id: "appr-reject-1",
            spec: None,
            task: None,
            operation_kind: "complete_task",
            policy_config_id: None,
            evidence_bundle_id: None,
            requested_by: "builder",
            request_context_json: &json!({}),
            expires_at: None,
        },
    )
    .await
    .unwrap();

    let decided = decide_approval(
        &pool,
        "appr-reject-1",
        ApprovalDecision {
            status: ApprovalStatus::Rejected,
            decided_by: "human",
            decision_reason: Some("Not ready"),
        },
    )
    .await
    .unwrap();

    assert_eq!(decided.status, ApprovalStatus::Rejected);
}

#[tokio::test]
async fn cannot_decide_on_already_decided_approval() {
    let pool = make_pool().await;
    create_approval(
        &pool,
        CreateApproval {
            id: "appr-double-decide",
            entity_kind: ApprovalEntityKind::Operation,
            entity_id: "appr-double-decide",
            spec: None,
            task: None,
            operation_kind: "complete_task",
            policy_config_id: None,
            evidence_bundle_id: None,
            requested_by: "builder",
            request_context_json: &json!({}),
            expires_at: None,
        },
    )
    .await
    .unwrap();

    // First decision succeeds
    decide_approval(
        &pool,
        "appr-double-decide",
        ApprovalDecision {
            status: ApprovalStatus::Approved,
            decided_by: "human",
            decision_reason: None,
        },
    )
    .await
    .unwrap();

    // Second decision on already-decided approval should error
    let err = decide_approval(
        &pool,
        "appr-double-decide",
        ApprovalDecision {
            status: ApprovalStatus::Rejected,
            decided_by: "human",
            decision_reason: None,
        },
    )
    .await
    .unwrap_err();

    assert!(err.to_string().contains("cannot transition"));
}

// ─── Gate enforcement via workflow ────────────────────────────────────────────

async fn setup_spec_and_task(pool: &SqlitePool, spec_id: &str, task_id: &str) {
    create_spec(pool, spec_id, "Policy gate spec", "P0", &[])
        .await
        .unwrap();
    approve_spec(pool, spec_id, "human", None).await.unwrap();
    start_spec(pool, spec_id, "human").await.unwrap();
    create_task(
        pool,
        task_id,
        spec_id,
        "Policy gate task",
        "builder",
        &[],
        None,
    )
    .await
    .unwrap();
    start_task(pool, task_id, "test-agent").await.unwrap();
}

#[tokio::test]
async fn complete_task_fails_when_evidence_required_but_missing() {
    let pool = make_pool().await;
    setup_spec_and_task(&pool, "SPEC-GATE-001", "T-GATE-001").await;

    // Set a task-level policy requiring evidence
    create_policy_config(
        &pool,
        CreatePolicyConfig {
            id: "pcfg-gate-task",
            scope_kind: PolicyScopeKind::Task,
            scope_ref: "T-GATE-001",
            agent: None,
            enabled: true,
            enforcement_mode: EnforcementMode::Enforced,
            rules_json: &json!({"require_evidence_bundle": true, "require_rationale": false}),
            rationale: None,
            created_by: Some("test"),
        },
    )
    .await
    .unwrap();

    let err = complete_task(&pool, "T-GATE-001", "test-agent", None)
        .await
        .unwrap_err();
    assert!(
        err.to_string().contains("evidence bundle"),
        "expected evidence bundle error, got: {}",
        err
    );
}

#[tokio::test]
async fn complete_task_succeeds_when_evidence_submitted() {
    let pool = make_pool().await;
    setup_spec_and_task(&pool, "SPEC-GATE-002", "T-GATE-002").await;

    // Set a task-level policy requiring only an evidence bundle (no validation)
    create_policy_config(
        &pool,
        CreatePolicyConfig {
            id: "pcfg-gate-task-ok",
            scope_kind: PolicyScopeKind::Task,
            scope_ref: "T-GATE-002",
            agent: None,
            enabled: true,
            enforcement_mode: EnforcementMode::Enforced,
            rules_json: &json!({
                "require_evidence_bundle": true,
                "require_rationale": false
            }),
            rationale: None,
            created_by: Some("test"),
        },
    )
    .await
    .unwrap();

    // Submit evidence bundle
    create_evidence_bundle(
        &pool,
        NewEvidenceBundle {
            id: "bundle-gate-002",
            reference: EvidenceRef::for_task("SPEC-GATE-002", "T-GATE-002"),
            status: EvidenceBundleStatus::Submitted,
            summary: Some("done"),
            behavior_change: false,
            metadata_json: json!({}),
            created_by: Some("builder"),
            updated_by: Some("builder"),
        },
    )
    .await
    .unwrap();

    // Also record a passing primary validation run (required by strict default for in_progress spec)
    let ran_at = chrono::Utc::now().to_rfc3339();
    sdd::evidence::record_validation_run(
        &pool,
        RecordedValidationRun {
            id: "vrun-gate-002",
            evidence_bundle_id: None,
            reference: EvidenceRef::for_task("SPEC-GATE-002", "T-GATE-002"),
            command_alias: ValidationCommandAlias::Primary,
            command: "cargo test",
            source: ValidationRunSource::Recorded,
            exit_code: Some(0),
            success: true,
            ran_at: &ran_at,
            recorded_by: Some("builder"),
            output_summary: Some("all tests passed"),
            metadata_json: json!({}),
        },
    )
    .await
    .unwrap();

    let task = complete_task(&pool, "T-GATE-002", "test-agent", None)
        .await
        .unwrap();
    assert_eq!(task.status, "done");
}

#[tokio::test]
async fn complete_spec_fails_when_approval_required_but_not_granted() {
    let pool = make_pool().await;
    setup_spec_and_task(&pool, "SPEC-GATE-003", "T-GATE-003").await;

    // Set the policy BEFORE completing the task so it applies to both task and spec completion
    // Relax all evidence requirements but require approval for spec completion
    create_policy_config(
        &pool,
        CreatePolicyConfig {
            id: "pcfg-gate-spec-approval",
            scope_kind: PolicyScopeKind::Spec,
            scope_ref: "SPEC-GATE-003",
            agent: None,
            enabled: true,
            enforcement_mode: EnforcementMode::Enforced,
            rules_json: &json!({
                "require_evidence_bundle": false,
                "require_rationale": false,
                "task_completion": {
                    "require_evidence_bundle": false,
                    "require_rationale": false
                },
                "spec_completion": {
                    "require_approval": true,
                    "require_evidence_bundle": false,
                    "require_rationale": false,
                    "require_validation": "primary"
                }
            }),
            rationale: None,
            created_by: Some("test"),
        },
    )
    .await
    .unwrap();

    // Now complete the task (policy relaxes evidence requirements, but validation still needed)
    // Record a passing primary validation run for the task
    let ran_at = chrono::Utc::now().to_rfc3339();
    sdd::evidence::record_validation_run(
        &pool,
        RecordedValidationRun {
            id: "vrun-gate-003-task",
            evidence_bundle_id: None,
            reference: EvidenceRef::for_task("SPEC-GATE-003", "T-GATE-003"),
            command_alias: ValidationCommandAlias::Primary,
            command: "cargo test",
            source: ValidationRunSource::Recorded,
            exit_code: Some(0),
            success: true,
            ran_at: &ran_at,
            recorded_by: Some("builder"),
            output_summary: Some("all tests passed"),
            metadata_json: json!({}),
        },
    )
    .await
    .unwrap();
    complete_task(&pool, "T-GATE-003", "test-agent", None)
        .await
        .unwrap();
    update_spec_ac(&pool, "SPEC-GATE-003", 1, 1).await.unwrap();

    // Provide spec-level evidence bundle + artifact + primary validation run
    // so the only remaining blocker is the approval gate
    sdd::artifact::register_artifact(
        &pool,
        "artifact-gate-003",
        Some("SPEC-GATE-003"),
        None,
        "builder",
        "source",
        Some("src/lib.rs"),
        Some("Gate test artifact"),
        None,
    )
    .await
    .unwrap();
    let spec_bundle = create_evidence_bundle(
        &pool,
        NewEvidenceBundle {
            id: "bundle-gate-003-spec",
            reference: EvidenceRef::for_spec("SPEC-GATE-003"),
            status: EvidenceBundleStatus::Submitted,
            summary: Some("spec done"),
            behavior_change: false,
            metadata_json: json!({}),
            created_by: Some("builder"),
            updated_by: Some("builder"),
        },
    )
    .await
    .unwrap();
    sdd::evidence::attach_artifact_to_evidence_bundle(
        &pool,
        &spec_bundle.id,
        "artifact-gate-003",
        sdd::evidence::EvidenceArtifactRole::PrimaryOutput,
    )
    .await
    .unwrap();
    let ran_at2 = chrono::Utc::now().to_rfc3339();
    sdd::evidence::record_validation_run(
        &pool,
        RecordedValidationRun {
            id: "vrun-gate-003-spec",
            evidence_bundle_id: None,
            reference: EvidenceRef::for_spec("SPEC-GATE-003"),
            command_alias: ValidationCommandAlias::Primary,
            command: "cargo test",
            source: ValidationRunSource::Recorded,
            exit_code: Some(0),
            success: true,
            ran_at: &ran_at2,
            recorded_by: Some("builder"),
            output_summary: Some("all tests passed"),
            metadata_json: json!({}),
        },
    )
    .await
    .unwrap();

    let err = complete_spec(&pool, "SPEC-GATE-003", "human", None)
        .await
        .unwrap_err();
    assert!(
        err.to_string().contains("approval") || err.to_string().contains("requires"),
        "expected approval error, got: {}",
        err
    );
}
