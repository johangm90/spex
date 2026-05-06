//! Integration tests for SPEC-007: Guided Sprint Workflows & Review Readiness
//!
//! Covers: CLI readiness surface, CLI session checkpoint surface, and MCP tool surface.

#![allow(dead_code)]

#[path = "../src/config.rs"]
mod config;
#[path = "../src/sdd/mod.rs"]
mod sdd;
#[path = "../src/webhooks.rs"]
mod webhooks;

#[path = "../src/cli/readiness.rs"]
mod cli_readiness;
#[path = "../src/cli/session.rs"]
mod cli_session;
#[path = "../src/cli/util.rs"]
mod cli_util;

mod mcp_tools {
    use anyhow::Result;
    use serde_json::{json, Value};
    use sqlx::SqlitePool;

    use crate::sdd::readiness::{
        get_checkpoint_by_id, get_latest_checkpoint, save_checkpoint, spec_readiness,
        transition_phase, WorkflowPhaseKind,
    };

    pub async fn dispatch_tool(pool: &SqlitePool, name: &str, args: Value) -> Result<Value> {
        match name {
            "state_readiness_spec" => {
                let spec_id = args["spec_id"]
                    .as_str()
                    .ok_or_else(|| anyhow::anyhow!("missing spec_id"))?;
                let report = spec_readiness(pool, spec_id).await?;
                Ok(serde_json::to_value(report)?)
            }
            "state_readiness_phase_transition" => {
                let spec_id = args["spec_id"]
                    .as_str()
                    .ok_or_else(|| anyhow::anyhow!("missing spec_id"))?;
                let phase_str = args["phase"]
                    .as_str()
                    .ok_or_else(|| anyhow::anyhow!("missing phase"))?;
                let phase = WorkflowPhaseKind::from_str(phase_str)
                    .ok_or_else(|| anyhow::anyhow!("unknown phase: {}", phase_str))?;
                let result = transition_phase(pool, spec_id, phase, None, None).await?;
                Ok(json!({ "id": result.id, "spec_id": result.spec_id, "phase": result.phase }))
            }
            "state_session_checkpoint_save" => {
                use chrono::Utc;
                let session_id = args["session_id"]
                    .as_str()
                    .ok_or_else(|| anyhow::anyhow!("missing session_id"))?;
                let agent = args["agent"]
                    .as_str()
                    .ok_or_else(|| anyhow::anyhow!("missing agent"))?;
                let label = args["label"].as_str();
                let checkpoint_data = args
                    .get("checkpoint_data")
                    .ok_or_else(|| anyhow::anyhow!("missing checkpoint_data"))?;
                let data_json = serde_json::to_string(checkpoint_data)?;
                let id = format!("cp-{}", Utc::now().timestamp_nanos_opt().unwrap_or(0));
                let cp =
                    save_checkpoint(pool, &id, session_id, None, None, agent, &data_json, label)
                        .await?;
                Ok(
                    json!({ "id": cp.id, "session_id": cp.session_id, "agent": cp.agent, "label": cp.label }),
                )
            }
            "state_session_checkpoint_restore" => {
                let session_id = args["session_id"]
                    .as_str()
                    .ok_or_else(|| anyhow::anyhow!("missing session_id"))?;
                let checkpoint_id = args["checkpoint_id"].as_str();
                let cp = if let Some(cp_id) = checkpoint_id {
                    get_checkpoint_by_id(pool, cp_id, session_id).await?
                } else {
                    get_latest_checkpoint(pool, session_id).await?
                };
                match cp {
                    None => Ok(json!({ "checkpoint": null })),
                    Some(c) => {
                        let checkpoint_data: Value = serde_json::from_str(&c.checkpoint_data)
                            .unwrap_or(Value::String(c.checkpoint_data.clone()));
                        Ok(json!({
                            "id": c.id,
                            "session_id": c.session_id,
                            "agent": c.agent,
                            "label": c.label,
                            "saved_at": c.saved_at,
                            "checkpoint_data": checkpoint_data,
                        }))
                    }
                }
            }
            _ => Err(anyhow::anyhow!("Unknown tool: {}", name)),
        }
    }
}

use chrono::Utc;
use serde_json::json;
use sqlx::SqlitePool;

use sdd::readiness::{list_review_requirements, review_complete, satisfy_review_requirement};
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
// CLI readiness surface
// ---------------------------------------------------------------------------

/// 1. Create a spec, call cmd_readiness_spec, verify it returns Ok.
#[tokio::test]
async fn test_cli_readiness_spec_report() {
    let pool = make_pool().await;
    seed_spec(&pool, "spec-cli-rd-1").await;

    let result = cli_readiness::cmd_readiness_spec(&pool, "spec-cli-rd-1").await;
    assert!(
        result.is_ok(),
        "cmd_readiness_spec should return Ok: {:?}",
        result
    );
}

/// 2. Create 2 specs, call cmd_readiness_operator, verify Ok.
#[tokio::test]
async fn test_cli_readiness_operator_report() {
    let pool = make_pool().await;
    seed_spec(&pool, "spec-cli-op-a").await;
    seed_spec(&pool, "spec-cli-op-b").await;

    let result = cli_readiness::cmd_readiness_operator(&pool).await;
    assert!(
        result.is_ok(),
        "cmd_readiness_operator should return Ok: {:?}",
        result
    );
}

/// 3. Create spec, call cmd_readiness_phase with "review", verify Ok.
#[tokio::test]
async fn test_cli_readiness_phase_transition() {
    let pool = make_pool().await;
    seed_spec(&pool, "spec-cli-ph-1").await;

    let result =
        cli_readiness::cmd_readiness_phase(&pool, "spec-cli-ph-1", "review", None, None).await;
    assert!(
        result.is_ok(),
        "cmd_readiness_phase should return Ok: {:?}",
        result
    );
}

/// 4. Create spec, call cmd_readiness_enter_review, then verify 3 requirements seeded.
#[tokio::test]
async fn test_cli_readiness_enter_review_seeds_requirements() {
    let pool = make_pool().await;
    seed_spec(&pool, "spec-cli-er-1").await;

    let result = cli_readiness::cmd_readiness_enter_review(&pool, "spec-cli-er-1", None).await;
    assert!(
        result.is_ok(),
        "cmd_readiness_enter_review should return Ok: {:?}",
        result
    );

    let reqs = list_review_requirements(&pool, "spec-cli-er-1")
        .await
        .unwrap();
    assert_eq!(
        reqs.len(),
        3,
        "enter_review should seed exactly 3 default requirements"
    );
}

/// 5. Create spec, enter review, satisfy all requirements manually,
///    call cmd_readiness_approve, verify Ok and review_complete is true.
#[tokio::test]
async fn test_cli_readiness_approve_transitions_to_done() {
    let pool = make_pool().await;
    seed_spec(&pool, "spec-cli-ap-1").await;

    // Enter review (seeds 3 requirements).
    cli_readiness::cmd_readiness_enter_review(&pool, "spec-cli-ap-1", None)
        .await
        .unwrap();

    // Satisfy test_pass and lint_pass manually; approve_review will handle review_approved.
    let reqs = list_review_requirements(&pool, "spec-cli-ap-1")
        .await
        .unwrap();
    for req in &reqs {
        if req.kind != "review_approved" {
            satisfy_review_requirement(&pool, &req.id, Some("ci-agent"))
                .await
                .unwrap();
        }
    }

    let result = cli_readiness::cmd_readiness_approve(&pool, "spec-cli-ap-1", "human").await;
    assert!(
        result.is_ok(),
        "cmd_readiness_approve should return Ok: {:?}",
        result
    );

    let complete = review_complete(&pool, "spec-cli-ap-1").await.unwrap();
    assert!(
        complete,
        "review_complete should be true after all requirements satisfied and approved"
    );
}

// ---------------------------------------------------------------------------
// CLI session checkpoint surface
// ---------------------------------------------------------------------------

/// 6. Start session, save checkpoint, list checkpoints — verify Ok.
#[tokio::test]
async fn test_cli_session_checkpoint_save_and_list() {
    let pool = make_pool().await;
    let session_id = seed_session(&pool, "sess-cli-cp-1").await;

    let save_result = cli_session::cmd_session_checkpoint(
        &pool,
        &session_id,
        "agent",
        None,
        None,
        Some("label"),
        r#"{"key":"val"}"#,
    )
    .await;
    assert!(
        save_result.is_ok(),
        "cmd_session_checkpoint should return Ok: {:?}",
        save_result
    );

    let list_result = cli_session::cmd_session_checkpoints(&pool, &session_id).await;
    assert!(
        list_result.is_ok(),
        "cmd_session_checkpoints should return Ok: {:?}",
        list_result
    );
}

/// 7. Start session, save 2 checkpoints, call cmd_session_restore with None — verify Ok.
#[tokio::test]
async fn test_cli_session_restore_latest() {
    let pool = make_pool().await;
    let session_id = seed_session(&pool, "sess-cli-rs-1").await;

    cli_session::cmd_session_checkpoint(
        &pool,
        &session_id,
        "agent",
        None,
        None,
        Some("first"),
        r#"{"v":1}"#,
    )
    .await
    .unwrap();

    tokio::time::sleep(std::time::Duration::from_millis(5)).await;

    cli_session::cmd_session_checkpoint(
        &pool,
        &session_id,
        "agent",
        None,
        None,
        Some("second"),
        r#"{"v":2}"#,
    )
    .await
    .unwrap();

    let restore_result = cli_session::cmd_session_restore(&pool, &session_id, None).await;
    assert!(
        restore_result.is_ok(),
        "cmd_session_restore should return Ok: {:?}",
        restore_result
    );
}

// ---------------------------------------------------------------------------
// MCP tool surface
// ---------------------------------------------------------------------------

/// 8. Create spec, call state_readiness_spec, verify result has `ready` field.
#[tokio::test]
async fn test_mcp_state_readiness_spec() {
    let pool = make_pool().await;
    seed_spec(&pool, "spec-mcp-rd-1").await;

    let result = mcp_tools::dispatch_tool(
        &pool,
        "state_readiness_spec",
        json!({"spec_id": "spec-mcp-rd-1"}),
    )
    .await;
    assert!(
        result.is_ok(),
        "dispatch_tool state_readiness_spec should return Ok: {:?}",
        result
    );

    let val = result.unwrap();
    assert!(
        val.get("ready").is_some(),
        "result should have 'ready' field, got: {}",
        val
    );
}

/// 9. Create spec, call state_readiness_phase_transition, verify Ok.
#[tokio::test]
async fn test_mcp_state_readiness_phase_transition() {
    let pool = make_pool().await;
    seed_spec(&pool, "spec-mcp-ph-1").await;

    let result = mcp_tools::dispatch_tool(
        &pool,
        "state_readiness_phase_transition",
        json!({"spec_id": "spec-mcp-ph-1", "phase": "review"}),
    )
    .await;
    assert!(
        result.is_ok(),
        "dispatch_tool state_readiness_phase_transition should return Ok: {:?}",
        result
    );
}

/// 10. Start session, save checkpoint via MCP, restore via MCP, verify checkpoint_data round-trips.
#[tokio::test]
async fn test_mcp_state_session_checkpoint_save_restore() {
    let pool = make_pool().await;
    let session_id = seed_session(&pool, "sess-mcp-cp-1").await;

    let checkpoint_data = json!({"step": "planning", "progress": 42});

    let save_result = mcp_tools::dispatch_tool(
        &pool,
        "state_session_checkpoint_save",
        json!({
            "session_id": session_id,
            "agent": "test-agent",
            "label": "mcp-test",
            "checkpoint_data": checkpoint_data,
        }),
    )
    .await;
    assert!(
        save_result.is_ok(),
        "dispatch_tool state_session_checkpoint_save should return Ok: {:?}",
        save_result
    );

    let saved = save_result.unwrap();
    let checkpoint_id = saved["id"].as_str().expect("saved checkpoint must have id");

    let restore_result = mcp_tools::dispatch_tool(
        &pool,
        "state_session_checkpoint_restore",
        json!({
            "session_id": session_id,
            "checkpoint_id": checkpoint_id,
        }),
    )
    .await;
    assert!(
        restore_result.is_ok(),
        "dispatch_tool state_session_checkpoint_restore should return Ok: {:?}",
        restore_result
    );

    let restored = restore_result.unwrap();
    assert_eq!(
        restored["checkpoint_data"], checkpoint_data,
        "checkpoint_data must round-trip exactly"
    );
}
