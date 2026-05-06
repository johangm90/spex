//! Integration tests for SPEC-007: Guided Sprint Workflows & Review Readiness
//!
//! Covers: phase persistence, review status semantics, readiness blockers,
//! and checkpoint restore.

#![allow(dead_code)]

#[path = "../src/config.rs"]
mod config;
#[path = "../src/sdd/mod.rs"]
mod sdd;
#[path = "../src/webhooks.rs"]
mod webhooks;

use chrono::Utc;
use sqlx::SqlitePool;

use sdd::readiness::{
    get_checkpoint_by_id, get_current_phase, get_latest_checkpoint, insert_review_requirement,
    insert_workflow_phase, list_phases, operator_readiness, review_complete,
    satisfy_review_requirement, save_checkpoint, spec_readiness, unsatisfied_requirements,
    ReviewRequirementKind, WorkflowPhaseKind,
};
use sdd::sessions::{start_session, NewSession};

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

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

async fn seed_spec(pool: &SqlitePool, spec_id: &str) {
    let now = Utc::now().to_rfc3339();
    sqlx::query(
        "INSERT OR IGNORE INTO specs (id, title, status, priority, created_at, updated_at)
         VALUES (?, 'Test Spec', 'draft', 'P1', ?, ?)",
    )
    .bind(spec_id)
    .bind(&now)
    .bind(&now)
    .execute(pool)
    .await
    .unwrap();
}

async fn seed_spec_with_acs(pool: &SqlitePool, spec_id: &str, ac_total: i64, ac_passed: i64) {
    let now = Utc::now().to_rfc3339();
    sqlx::query(
        "INSERT OR IGNORE INTO specs (id, title, status, priority, ac_total, ac_passed, created_at, updated_at)
         VALUES (?, 'Test Spec', 'draft', 'P1', ?, ?, ?, ?)",
    )
    .bind(spec_id)
    .bind(ac_total)
    .bind(ac_passed)
    .bind(&now)
    .bind(&now)
    .execute(pool)
    .await
    .unwrap();
}

async fn seed_session(pool: &SqlitePool, session_id: &str) -> String {
    start_session(
        pool,
        NewSession {
            id: session_id,
            agent: "test-agent",
            spec_id: None,
            task_id: None,
            host: None,
            notes: None,
        },
    )
    .await
    .unwrap();
    session_id.to_string()
}

// ---------------------------------------------------------------------------
// Phase persistence
// ---------------------------------------------------------------------------

/// 1. Insert phase, verify persisted, close it, verify exited_at set,
///    insert new phase, verify get_current_phase returns new one.
#[tokio::test]
async fn test_phase_lifecycle() {
    let pool = make_pool().await;
    seed_spec(&pool, "spec-ph-lc").await;

    // Insert first phase.
    let phase1 = insert_workflow_phase(
        &pool,
        "ph-lc-1",
        "spec-ph-lc",
        WorkflowPhaseKind::Planning,
        None,
        None,
    )
    .await
    .unwrap();
    assert_eq!(phase1.id, "ph-lc-1");
    assert_eq!(phase1.phase, "planning");
    assert!(
        phase1.exited_at.is_none(),
        "new phase should not have exited_at"
    );

    // Close it.
    let exit_time = Utc::now().to_rfc3339();
    sdd::readiness::close_workflow_phase(&pool, "ph-lc-1", &exit_time)
        .await
        .unwrap();

    // Verify exited_at is set.
    let phases = list_phases(&pool, "spec-ph-lc").await.unwrap();
    assert_eq!(phases.len(), 1);
    assert!(
        phases[0].exited_at.is_some(),
        "closed phase must have exited_at"
    );

    // Insert new phase.
    insert_workflow_phase(
        &pool,
        "ph-lc-2",
        "spec-ph-lc",
        WorkflowPhaseKind::InProgress,
        None,
        None,
    )
    .await
    .unwrap();

    // get_current_phase should return the new one.
    let current = get_current_phase(&pool, "spec-ph-lc").await.unwrap();
    assert!(current.is_some());
    assert_eq!(current.unwrap().id, "ph-lc-2");
}

/// 2. Insert 3 phases sequentially, verify list_phases returns all 3.
#[tokio::test]
async fn test_list_phases_ordered() {
    let pool = make_pool().await;
    seed_spec(&pool, "spec-ph-ord").await;

    for (id, kind) in [
        ("ph-ord-1", WorkflowPhaseKind::Planning),
        ("ph-ord-2", WorkflowPhaseKind::InProgress),
        ("ph-ord-3", WorkflowPhaseKind::Review),
    ] {
        insert_workflow_phase(&pool, id, "spec-ph-ord", kind, None, None)
            .await
            .unwrap();
    }

    let phases = list_phases(&pool, "spec-ph-ord").await.unwrap();
    assert_eq!(phases.len(), 3, "list_phases should return all 3 phases");
    assert_eq!(phases[0].id, "ph-ord-1");
    assert_eq!(phases[1].id, "ph-ord-2");
    assert_eq!(phases[2].id, "ph-ord-3");
}

// ---------------------------------------------------------------------------
// Review status semantics
// ---------------------------------------------------------------------------

/// 3. Insert requirement, verify satisfied=0, satisfy it, verify satisfied=1
///    and satisfied_at is set.
#[tokio::test]
async fn test_review_requirements_satisfied_flag() {
    let pool = make_pool().await;
    seed_spec(&pool, "spec-rr-sf").await;

    let req = insert_review_requirement(
        &pool,
        "req-sf-1",
        "spec-rr-sf",
        ReviewRequirementKind::TestPass,
        "All tests pass",
    )
    .await
    .unwrap();

    assert!(
        !req.satisfied,
        "newly inserted requirement must be unsatisfied"
    );
    assert!(req.satisfied_at.is_none());

    satisfy_review_requirement(&pool, "req-sf-1", Some("ci-agent"))
        .await
        .unwrap();

    let reqs = sdd::readiness::list_review_requirements(&pool, "spec-rr-sf")
        .await
        .unwrap();
    assert_eq!(reqs.len(), 1);
    assert!(
        reqs[0].satisfied,
        "requirement must be satisfied after satisfy call"
    );
    assert!(reqs[0].satisfied_at.is_some(), "satisfied_at must be set");
}

/// 4. Insert 3 requirements, satisfy 2, verify unsatisfied_requirements returns only 1.
#[tokio::test]
async fn test_unsatisfied_requirements_filter() {
    let pool = make_pool().await;
    seed_spec(&pool, "spec-rr-uf").await;

    for (id, kind) in [
        ("req-uf-1", ReviewRequirementKind::TestPass),
        ("req-uf-2", ReviewRequirementKind::LintPass),
        ("req-uf-3", ReviewRequirementKind::ReviewApproved),
    ] {
        insert_review_requirement(&pool, id, "spec-rr-uf", kind, "desc")
            .await
            .unwrap();
    }

    satisfy_review_requirement(&pool, "req-uf-1", None)
        .await
        .unwrap();
    satisfy_review_requirement(&pool, "req-uf-2", None)
        .await
        .unwrap();

    let unsatisfied = unsatisfied_requirements(&pool, "spec-rr-uf").await.unwrap();
    assert_eq!(
        unsatisfied.len(),
        1,
        "only 1 requirement should remain unsatisfied"
    );
    assert_eq!(unsatisfied[0].id, "req-uf-3");
}

/// 5. Satisfy all requirements, verify review_complete returns true.
#[tokio::test]
async fn test_review_complete_true_when_all_satisfied() {
    let pool = make_pool().await;
    seed_spec(&pool, "spec-rc-t").await;

    for (id, kind) in [
        ("req-rc-t-1", ReviewRequirementKind::TestPass),
        ("req-rc-t-2", ReviewRequirementKind::LintPass),
    ] {
        insert_review_requirement(&pool, id, "spec-rc-t", kind, "desc")
            .await
            .unwrap();
        satisfy_review_requirement(&pool, id, None).await.unwrap();
    }

    let complete = review_complete(&pool, "spec-rc-t").await.unwrap();
    assert!(
        complete,
        "review_complete should be true when all requirements satisfied"
    );
}

/// 6. Leave one unsatisfied, verify review_complete returns false.
#[tokio::test]
async fn test_review_complete_false_when_any_unsatisfied() {
    let pool = make_pool().await;
    seed_spec(&pool, "spec-rc-f").await;

    insert_review_requirement(
        &pool,
        "req-rc-f-1",
        "spec-rc-f",
        ReviewRequirementKind::TestPass,
        "Tests",
    )
    .await
    .unwrap();
    insert_review_requirement(
        &pool,
        "req-rc-f-2",
        "spec-rc-f",
        ReviewRequirementKind::LintPass,
        "Lint",
    )
    .await
    .unwrap();

    // Satisfy only the first.
    satisfy_review_requirement(&pool, "req-rc-f-1", None)
        .await
        .unwrap();

    let complete = review_complete(&pool, "spec-rc-f").await.unwrap();
    assert!(
        !complete,
        "review_complete should be false when any requirement is unsatisfied"
    );
}

// ---------------------------------------------------------------------------
// Readiness blockers
// ---------------------------------------------------------------------------

/// 7. Spec with no tasks, no ACs, no requirements → ready=true.
#[tokio::test]
async fn test_spec_readiness_no_blockers_empty_spec() {
    let pool = make_pool().await;
    seed_spec(&pool, "spec-rd-empty").await;

    let report = spec_readiness(&pool, "spec-rd-empty").await.unwrap();
    assert!(report.ready, "empty spec should be ready");
    assert!(
        report.blockers.is_empty(),
        "empty spec should have no blockers"
    );
}

/// 8. Add unsatisfied requirement → blockers contains "unsatisfied_requirement".
#[tokio::test]
async fn test_spec_readiness_blocker_unsatisfied_requirement() {
    let pool = make_pool().await;
    seed_spec(&pool, "spec-rd-req").await;

    insert_review_requirement(
        &pool,
        "req-rd-req",
        "spec-rd-req",
        ReviewRequirementKind::TestPass,
        "Tests must pass",
    )
    .await
    .unwrap();

    let report = spec_readiness(&pool, "spec-rd-req").await.unwrap();
    assert!(!report.ready);
    let blocker = report
        .blockers
        .iter()
        .find(|b| b.kind == "unsatisfied_requirement");
    assert!(
        blocker.is_some(),
        "should have unsatisfied_requirement blocker"
    );
    assert_eq!(blocker.unwrap().description, "Tests must pass");
}

/// 9. Spec with ac_total=5, ac_passed=3 → blockers contains "ac_gap".
#[tokio::test]
async fn test_spec_readiness_blocker_ac_gap() {
    let pool = make_pool().await;
    seed_spec_with_acs(&pool, "spec-rd-ac", 5, 3).await;

    let report = spec_readiness(&pool, "spec-rd-ac").await.unwrap();
    assert!(!report.ready);
    let blocker = report.blockers.iter().find(|b| b.kind == "ac_gap");
    assert!(blocker.is_some(), "should have ac_gap blocker");
    assert!(
        blocker.unwrap().description.contains("3/5"),
        "ac_gap description should mention 3/5 ACs passed"
    );
}

/// 10. 2 specs, one ready one not → ready_specs=1, blocked_specs=1.
#[tokio::test]
async fn test_operator_readiness_aggregates_multiple_specs() {
    let pool = make_pool().await;

    // Spec A: ready (no requirements, no tasks, no ACs).
    seed_spec(&pool, "spec-op-007-a").await;

    // Spec B: blocked by unsatisfied requirement.
    seed_spec(&pool, "spec-op-007-b").await;
    insert_review_requirement(
        &pool,
        "req-op-007-b",
        "spec-op-007-b",
        ReviewRequirementKind::LintPass,
        "Lint must pass",
    )
    .await
    .unwrap();

    let report = operator_readiness(&pool).await.unwrap();

    let spec_a = report.specs.iter().find(|s| s.spec_id == "spec-op-007-a");
    let spec_b = report.specs.iter().find(|s| s.spec_id == "spec-op-007-b");
    assert!(spec_a.is_some(), "spec-op-007-a must appear in report");
    assert!(spec_b.is_some(), "spec-op-007-b must appear in report");
    assert!(spec_a.unwrap().ready, "spec-op-007-a should be ready");
    assert!(!spec_b.unwrap().ready, "spec-op-007-b should be blocked");

    // In this isolated DB there are exactly 2 specs.
    assert_eq!(report.total_specs, 2);
    assert_eq!(report.ready_specs, 1);
    assert_eq!(report.blocked_specs, 1);
}

// ---------------------------------------------------------------------------
// Checkpoint restore
// ---------------------------------------------------------------------------

/// 11. Save checkpoint with JSON data, restore by ID, verify data round-trips exactly.
#[tokio::test]
async fn test_checkpoint_save_and_restore_by_id() {
    let pool = make_pool().await;
    seed_session(&pool, "sess-cp-id").await;

    let data = r#"{"step":"planning","progress":42,"nested":{"key":"value"}}"#;
    save_checkpoint(
        &pool,
        "cp-id-1",
        "sess-cp-id",
        None,
        None,
        "test-agent",
        data,
        Some("my-label"),
    )
    .await
    .unwrap();

    let restored = get_checkpoint_by_id(&pool, "cp-id-1", "sess-cp-id")
        .await
        .unwrap();
    assert!(restored.is_some(), "checkpoint must be found by ID");
    let cp = restored.unwrap();
    assert_eq!(cp.id, "cp-id-1");
    assert_eq!(
        cp.checkpoint_data, data,
        "checkpoint data must round-trip exactly"
    );
    assert_eq!(cp.label.as_deref(), Some("my-label"));
    assert_eq!(cp.agent, "test-agent");
}

/// 12. Save 2 checkpoints, restore latest (None), verify most recent returned.
#[tokio::test]
async fn test_checkpoint_restore_latest() {
    let pool = make_pool().await;
    seed_session(&pool, "sess-cp-latest").await;

    save_checkpoint(
        &pool,
        "cp-latest-1",
        "sess-cp-latest",
        None,
        None,
        "test-agent",
        r#"{"v":1}"#,
        None,
    )
    .await
    .unwrap();

    // Small sleep to ensure ordering by saved_at.
    tokio::time::sleep(std::time::Duration::from_millis(5)).await;

    save_checkpoint(
        &pool,
        "cp-latest-2",
        "sess-cp-latest",
        None,
        None,
        "test-agent",
        r#"{"v":2}"#,
        Some("second"),
    )
    .await
    .unwrap();

    let latest = get_latest_checkpoint(&pool, "sess-cp-latest")
        .await
        .unwrap();
    assert!(latest.is_some(), "latest checkpoint must be found");
    let cp = latest.unwrap();
    assert_eq!(
        cp.id, "cp-latest-2",
        "most recent checkpoint must be returned"
    );
    assert_eq!(cp.checkpoint_data, r#"{"v":2}"#);
}

/// 13. Restore on session with no checkpoints → returns None (Err semantics: assert None).
#[tokio::test]
async fn test_checkpoint_restore_no_checkpoints_errors() {
    let pool = make_pool().await;
    seed_session(&pool, "sess-cp-empty").await;

    let result = get_latest_checkpoint(&pool, "sess-cp-empty").await.unwrap();
    assert!(
        result.is_none(),
        "get_latest_checkpoint on session with no checkpoints must return None"
    );
}
