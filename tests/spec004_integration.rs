#![allow(dead_code)]

#[path = "../src/config.rs"]
mod config;
#[path = "../src/sdd/mod.rs"]
mod sdd;
#[path = "../src/webhooks.rs"]
mod webhooks;

use sdd::{
    evidence::{
        attach_validation_run_to_evidence_bundle, create_evidence_bundle, record_validation_run,
        EvidenceBundleStatus, EvidenceRef, NewEvidenceBundle, RecordedValidationRun,
        ValidationCommandAlias, ValidationRequirementLevel, ValidationRunSource,
    },
    spec::create_spec,
    task::{create_task, get_task},
    workflow::{approve_spec, complete_spec, start_spec, start_task},
};
use sqlx::SqlitePool;
use std::path::Path;
use std::process::{Command, Output};
use tempfile::TempDir;

fn spex_bin() -> &'static str {
    env!("CARGO_BIN_EXE_spex")
}

async fn make_project() -> TempDir {
    let root = tempfile::tempdir().expect("temp project must be created");
    std::fs::create_dir_all(root.path().join("docs")).expect("docs dir must be created");
    std::fs::write(
        root.path().join("docs/PRD.md"),
        "# SPEC-004 test fixture\n\nNon-template PRD.\n",
    )
    .expect("PRD fixture must be written");

    let db_path = root.path().join(".spex").join("state.db");
    let pool = sdd::db::open_db(&db_path)
        .await
        .expect("fixture DB must open");
    pool.close().await;

    root
}

async fn open_pool(root: &Path) -> SqlitePool {
    sdd::db::open_db(&root.join(".spex").join("state.db"))
        .await
        .expect("project DB must open")
}

fn run_cli(root: &Path, args: &[&str]) -> Output {
    Command::new(spex_bin())
        .current_dir(root)
        .args(args)
        .output()
        .expect("CLI command must run")
}

fn stdout_text(output: &Output) -> String {
    String::from_utf8_lossy(&output.stdout).to_string()
}

fn stderr_text(output: &Output) -> String {
    String::from_utf8_lossy(&output.stderr).to_string()
}

fn parse_session_id(stdout: &str) -> String {
    stdout
        .lines()
        .find_map(|line| {
            line.split_once("ID:    ")
                .map(|(_, id)| id.trim().to_string())
        })
        .expect("session ID must be printed")
}

async fn record_task_primary_evidence(pool: &SqlitePool, spec_id: &str, task_id: &str) {
    create_evidence_bundle(
        pool,
        NewEvidenceBundle {
            id: "bundle-spec004-task",
            reference: EvidenceRef::for_task(spec_id, task_id),
            status: EvidenceBundleStatus::Submitted,
            summary: Some("task evidence"),
            behavior_change: false,
            metadata_json: serde_json::json!({}),
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
            id: "vrun-spec004-task",
            evidence_bundle_id: None,
            reference: EvidenceRef::for_task(spec_id, task_id),
            command_alias: ValidationCommandAlias::Primary,
            command: "cargo test --all-targets",
            source: ValidationRunSource::Recorded,
            exit_code: Some(0),
            success: true,
            ran_at: &ran_at,
            recorded_by: Some("builder"),
            output_summary: Some("tests passed"),
            metadata_json: serde_json::json!({}),
        },
    )
    .await
    .unwrap();

    attach_validation_run_to_evidence_bundle(
        pool,
        "bundle-spec004-task",
        "vrun-spec004-task",
        ValidationRequirementLevel::Primary,
    )
    .await
    .unwrap();
}

#[tokio::test]
async fn session_cli_round_trip_and_trace_full_support_task_scope() {
    let root = make_project().await;
    let pool = open_pool(root.path()).await;

    create_spec(&pool, "SPEC-TRACE", "Trace fixture", "P1", &[])
        .await
        .unwrap();
    approve_spec(&pool, "SPEC-TRACE", "human", None)
        .await
        .unwrap();
    start_spec(&pool, "SPEC-TRACE", "human").await.unwrap();
    create_task(
        &pool,
        "TASK-TRACE",
        "SPEC-TRACE",
        "Trace task",
        "builder",
        &[],
        None,
    )
    .await
    .unwrap();
    start_task(&pool, "TASK-TRACE", "builder").await.unwrap();
    pool.close().await;

    let start = run_cli(
        root.path(),
        &[
            "session",
            "start",
            "--agent",
            "builder",
            "--spec",
            "SPEC-TRACE",
            "--task",
            "TASK-TRACE",
            "--host",
            "test-host",
        ],
    );
    assert!(start.status.success(), "stderr: {}", stderr_text(&start));
    let session_id = parse_session_id(&stdout_text(&start));

    let list = run_cli(
        root.path(),
        &["session", "list", "--spec", "SPEC-TRACE", "--active"],
    );
    assert!(list.status.success(), "stderr: {}", stderr_text(&list));
    let list_stdout = stdout_text(&list);
    assert!(list_stdout.contains("builder"));
    assert!(list_stdout.contains("TASK-TRACE"));

    let end = run_cli(root.path(), &["session", "end", &session_id]);
    assert!(end.status.success(), "stderr: {}", stderr_text(&end));

    let trace = run_cli(
        root.path(),
        &[
            "trace",
            "--spec",
            "SPEC-TRACE",
            "--task",
            "TASK-TRACE",
            "--full",
        ],
    );
    assert!(trace.status.success(), "stderr: {}", stderr_text(&trace));
    let trace_stdout = stdout_text(&trace);
    assert!(trace_stdout.contains("SESSION"), "stdout: {trace_stdout}");
    assert!(trace_stdout.contains("EVENT"), "stdout: {trace_stdout}");
    assert!(
        trace_stdout.contains("TASK-TRACE"),
        "stdout: {trace_stdout}"
    );
}

#[tokio::test]
async fn spec_workflow_rejects_blank_updated_by() {
    let root = make_project().await;
    let pool = open_pool(root.path()).await;

    create_spec(&pool, "SPEC-UPD", "Updated by guard", "P1", &[])
        .await
        .unwrap();

    let approve_err = approve_spec(&pool, "SPEC-UPD", "   ", None)
        .await
        .unwrap_err();
    assert!(approve_err.to_string().contains("updated_by is required"));

    approve_spec(&pool, "SPEC-UPD", "human", None)
        .await
        .unwrap();
    start_spec(&pool, "SPEC-UPD", "human").await.unwrap();

    let done_err = complete_spec(&pool, "SPEC-UPD", "", None)
        .await
        .unwrap_err();
    assert!(done_err.to_string().contains("updated_by is required"));
}

#[tokio::test]
async fn failing_webhook_does_not_block_cli_task_completion() {
    let root = make_project().await;
    std::fs::create_dir_all(root.path().join(".spex")).unwrap();
    std::fs::write(
        root.path().join(".spex/config.toml"),
        r#"[webhooks]
url = "http://127.0.0.1:9/webhook"
events = ["TaskDone"]
timeout_secs = 1
"#,
    )
    .unwrap();

    let pool = open_pool(root.path()).await;
    create_spec(&pool, "SPEC-WH", "Webhook fixture", "P1", &[])
        .await
        .unwrap();
    approve_spec(&pool, "SPEC-WH", "human", None).await.unwrap();
    start_spec(&pool, "SPEC-WH", "human").await.unwrap();
    create_task(
        &pool,
        "TASK-WH",
        "SPEC-WH",
        "Webhook task",
        "builder",
        &[],
        None,
    )
    .await
    .unwrap();
    start_task(&pool, "TASK-WH", "builder").await.unwrap();
    record_task_primary_evidence(&pool, "SPEC-WH", "TASK-WH").await;
    pool.close().await;

    let output = run_cli(
        root.path(),
        &["task", "done", "TASK-WH", "--updated-by", "builder"],
    );
    assert!(output.status.success(), "stderr: {}", stderr_text(&output));

    let pool = open_pool(root.path()).await;
    let task = get_task(&pool, "TASK-WH").await.unwrap().unwrap();
    assert_eq!(task.status, "done");
}
