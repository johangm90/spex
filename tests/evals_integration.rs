#![allow(dead_code)]

#[path = "../src/config.rs"]
mod config;
#[path = "../src/sdd/mod.rs"]
mod sdd;
#[path = "../src/webhooks.rs"]
mod webhooks;

use serde_json::{json, Value};
use sqlx::SqlitePool;
use std::io::{BufRead, BufReader, Write};
use std::path::Path;
use std::process::{Child, ChildStdin, ChildStdout, Command, Output, Stdio};
use tempfile::TempDir;

use sdd::{
    artifact::register_artifact,
    evals::{
        compare_eval_run_to_latest_baseline, compare_eval_runs, get_eval_run_details,
        list_eval_run_details, record_eval_run, EvalRunFilters, NewEvalRun, NewEvalRunLink,
        NewEvalScorecardDimension, RecordEvalRun,
    },
    spec::create_spec,
    task::create_task,
    workflow::{approve_spec, start_spec, start_task},
};

fn spex_bin() -> &'static str {
    env!("CARGO_BIN_EXE_spex")
}

async fn make_project() -> TempDir {
    let root = tempfile::tempdir().expect("temp project must be created");
    std::fs::create_dir_all(root.path().join("docs")).expect("docs dir must be created");
    std::fs::write(
        root.path().join("docs/PRD.md"),
        "# Eval test fixture\n\nNon-template PRD.\n",
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

struct McpSession {
    child: Child,
    stdin: ChildStdin,
    stdout: BufReader<ChildStdout>,
    next_id: u64,
}

impl McpSession {
    fn start(root: &Path) -> Self {
        let mut child = Command::new(spex_bin())
            .current_dir(root)
            .args(["mcp", "serve"])
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .spawn()
            .expect("MCP server must start");

        let stdin = child.stdin.take().expect("MCP stdin must be piped");
        let stdout = BufReader::new(child.stdout.take().expect("MCP stdout must be piped"));
        let mut session = Self {
            child,
            stdin,
            stdout,
            next_id: 1,
        };

        let init = session.request(json!({
            "jsonrpc": "2.0",
            "id": 0,
            "method": "initialize",
            "params": {}
        }));
        assert!(init.get("result").is_some(), "initialize must succeed");

        session
    }

    fn call_tool(&mut self, name: &str, arguments: Value) -> ToolReply {
        let response = self.request(json!({
            "jsonrpc": "2.0",
            "id": self.next_id,
            "method": "tools/call",
            "params": {
                "name": name,
                "arguments": arguments,
            }
        }));
        self.next_id += 1;
        ToolReply::from_response(response)
    }

    fn request(&mut self, payload: Value) -> Value {
        writeln!(self.stdin, "{}", payload).expect("request must be written");
        self.stdin.flush().expect("request must flush");

        let mut line = String::new();
        self.stdout
            .read_line(&mut line)
            .expect("response must be readable");
        serde_json::from_str(&line).expect("response must be valid JSON")
    }

    fn shutdown(&mut self) {
        self.child.kill().expect("MCP server must stop cleanly");
        self.child.wait().expect("MCP server must reap cleanly");
    }
}

struct ToolReply {
    is_error: bool,
    payload: Value,
}

impl ToolReply {
    fn from_response(response: Value) -> Self {
        let result = response
            .get("result")
            .expect("tool response must have result");
        let is_error = result
            .get("isError")
            .and_then(Value::as_bool)
            .unwrap_or(false);
        let text = result["content"][0]["text"]
            .as_str()
            .expect("tool response text must exist");

        Self {
            is_error,
            payload: serde_json::from_str(text).expect("tool payload must be JSON"),
        }
    }
}

async fn setup_eval_scope(root: &Path) -> SqlitePool {
    let pool = open_pool(root).await;
    create_spec(&pool, "SPEC-EVAL-INT", "Eval integration spec", "P1", &[])
        .await
        .unwrap();
    approve_spec(&pool, "SPEC-EVAL-INT", "human", None)
        .await
        .unwrap();
    start_spec(&pool, "SPEC-EVAL-INT", "human").await.unwrap();
    create_task(
        &pool,
        "TASK-EVAL-INT",
        "SPEC-EVAL-INT",
        "Eval integration task",
        "builder",
        &[],
        None,
    )
    .await
    .unwrap();
    start_task(&pool, "TASK-EVAL-INT", "builder").await.unwrap();
    register_artifact(
        &pool,
        "ART-EVAL-INT",
        Some("SPEC-EVAL-INT"),
        Some("TASK-EVAL-INT"),
        "builder",
        "source",
        Some("src/lib.rs"),
        Some("eval integration artifact"),
        None,
    )
    .await
    .unwrap();
    pool
}

#[tokio::test]
async fn eval_domain_persistence_and_comparison_cover_append_only_and_filters() {
    let root = make_project().await;
    let pool = setup_eval_scope(root.path()).await;

    let baseline = record_eval_run(
        &pool,
        RecordEvalRun {
            run: NewEvalRun {
                id: "eval-domain-baseline",
                evaluator: "reviewer",
                target_kind: "task",
                target_ref: "TASK-EVAL-INT",
                spec: None,
                task: None,
                artifact_id: None,
                summary: Some("baseline"),
                rationale: Some("older run"),
                outcome: "warn",
                overall_score: Some(0.45),
                source: "recorded",
                metadata_json: json!({"phase": "baseline"}),
            },
            dimensions: vec![
                NewEvalScorecardDimension {
                    eval_run_id: "eval-domain-baseline",
                    dimension_name: "validation",
                    normalized_status: "warn",
                    normalized_score: Some(0.45),
                    normalized_value: None,
                    rationale: Some("warnings remain"),
                    details_json: json!({"warnings": 3}),
                },
                NewEvalScorecardDimension {
                    eval_run_id: "eval-domain-baseline",
                    dimension_name: "risk",
                    normalized_status: "warn",
                    normalized_score: Some(0.50),
                    normalized_value: Some("medium"),
                    rationale: None,
                    details_json: json!({}),
                },
            ],
            links: vec![NewEvalRunLink {
                eval_run_id: "eval-domain-baseline",
                link_kind: "artifact",
                link_ref: "ART-EVAL-INT",
                relation: "context",
            }],
        },
    )
    .await
    .unwrap();

    let current = record_eval_run(
        &pool,
        RecordEvalRun {
            run: NewEvalRun {
                id: "eval-domain-current",
                evaluator: "reviewer",
                target_kind: "task",
                target_ref: "TASK-EVAL-INT",
                spec: None,
                task: None,
                artifact_id: None,
                summary: Some("current"),
                rationale: Some("newer run"),
                outcome: "pass",
                overall_score: Some(0.93),
                source: "mcp",
                metadata_json: json!({"phase": "current"}),
            },
            dimensions: vec![
                NewEvalScorecardDimension {
                    eval_run_id: "eval-domain-current",
                    dimension_name: "validation_coverage",
                    normalized_status: "pass",
                    normalized_score: Some(0.93),
                    normalized_value: None,
                    rationale: Some("tests passed"),
                    details_json: json!({"warnings": 0}),
                },
                NewEvalScorecardDimension {
                    eval_run_id: "eval-domain-current",
                    dimension_name: "risk",
                    normalized_status: "pass",
                    normalized_score: Some(0.90),
                    normalized_value: Some("low"),
                    rationale: None,
                    details_json: json!({}),
                },
            ],
            links: vec![
                NewEvalRunLink {
                    eval_run_id: "eval-domain-current",
                    link_kind: "artifact",
                    link_ref: "ART-EVAL-INT",
                    relation: "context",
                },
                NewEvalRunLink {
                    eval_run_id: "eval-domain-current",
                    link_kind: "eval_run",
                    link_ref: "eval-domain-baseline",
                    relation: "baseline",
                },
            ],
        },
    )
    .await
    .unwrap();

    assert_eq!(baseline.run.spec.as_deref(), Some("SPEC-EVAL-INT"));
    assert_eq!(baseline.run.task.as_deref(), Some("TASK-EVAL-INT"));
    assert_eq!(current.links.len(), 2);

    let fetched = get_eval_run_details(&pool, "eval-domain-current")
        .await
        .unwrap()
        .unwrap();
    assert_eq!(fetched.run.outcome, "pass");
    assert_eq!(fetched.dimensions.len(), 2);
    assert_eq!(fetched.links.len(), 2);

    let filtered = list_eval_run_details(
        &pool,
        EvalRunFilters {
            task: Some("TASK-EVAL-INT"),
            source: Some("mcp"),
            limit: Some(10),
            ..Default::default()
        },
    )
    .await
    .unwrap();
    assert_eq!(filtered.len(), 1);
    assert_eq!(filtered[0].run.id, "eval-domain-current");

    let comparison = compare_eval_runs(&pool, "eval-domain-baseline", "eval-domain-current")
        .await
        .unwrap();
    assert_eq!(comparison.overall_classification, "improved");
    assert_eq!(comparison.baseline_eval_id, "eval-domain-baseline");
    assert_eq!(comparison.current_eval_id, "eval-domain-current");

    let latest = compare_eval_run_to_latest_baseline(&pool, "eval-domain-current")
        .await
        .unwrap()
        .unwrap();
    assert_eq!(latest.baseline_eval_id, "eval-domain-baseline");

    let duplicate_err = record_eval_run(
        &pool,
        RecordEvalRun {
            run: NewEvalRun {
                id: "eval-domain-current",
                evaluator: "reviewer",
                target_kind: "task",
                target_ref: "TASK-EVAL-INT",
                spec: None,
                task: None,
                artifact_id: None,
                summary: None,
                rationale: None,
                outcome: "pass",
                overall_score: Some(1.0),
                source: "recorded",
                metadata_json: json!({}),
            },
            dimensions: vec![],
            links: vec![],
        },
    )
    .await
    .unwrap_err();
    assert!(
        duplicate_err.to_string().contains("UNIQUE")
            || duplicate_err.to_string().contains("unique")
    );

    let all_runs = list_eval_run_details(&pool, EvalRunFilters::default())
        .await
        .unwrap();
    assert_eq!(
        all_runs.len(),
        2,
        "failed writes must not partially persist"
    );
}

#[tokio::test]
async fn eval_cli_round_trip_and_no_eval_backward_compatibility() {
    let root = make_project().await;
    let pool = setup_eval_scope(root.path()).await;
    pool.close().await;

    let empty_list = run_cli(root.path(), &["eval", "list"]);
    assert!(
        empty_list.status.success(),
        "stderr: {}",
        stderr_text(&empty_list)
    );
    assert!(stdout_text(&empty_list).contains("No evals found."));

    let create = run_cli(
        root.path(),
        &[
            "eval",
            "create",
            "--id",
            "eval-cli-baseline",
            "--evaluator",
            "reviewer",
            "--target-kind",
            "task",
            "--target-ref",
            "TASK-EVAL-INT",
            "--outcome",
            "warn",
            "--overall-score",
            "0.44",
            "--dimensions-json",
            r#"[{"name":"validation","status":"warn","score":0.44}]"#,
            "--json",
        ],
    );
    assert!(create.status.success(), "stderr: {}", stderr_text(&create));
    let created: Value = serde_json::from_str(&stdout_text(&create)).unwrap();
    assert_eq!(created["run"]["id"], "eval-cli-baseline");

    let create_current = run_cli(
        root.path(),
        &[
            "eval",
            "create",
            "--id",
            "eval-cli-current",
            "--evaluator",
            "reviewer",
            "--target-kind",
            "task",
            "--target-ref",
            "TASK-EVAL-INT",
            "--outcome",
            "pass",
            "--overall-score",
            "0.91",
            "--dimensions-json",
            r#"[{"name":"validation_coverage","status":"pass","score":0.91}]"#,
            "--links-json",
            r#"[{"kind":"eval_run","ref":"eval-cli-baseline","relation":"baseline"}]"#,
            "--json",
        ],
    );
    assert!(
        create_current.status.success(),
        "stderr: {}",
        stderr_text(&create_current)
    );

    let list = run_cli(
        root.path(),
        &["eval", "list", "--task", "TASK-EVAL-INT", "--json"],
    );
    assert!(list.status.success(), "stderr: {}", stderr_text(&list));
    let listed: Value = serde_json::from_str(&stdout_text(&list)).unwrap();
    assert_eq!(listed.as_array().unwrap().len(), 2);

    let show = run_cli(root.path(), &["eval", "show", "eval-cli-current", "--json"]);
    assert!(show.status.success(), "stderr: {}", stderr_text(&show));
    let shown: Value = serde_json::from_str(&stdout_text(&show)).unwrap();
    assert_eq!(shown["run"]["id"], "eval-cli-current");

    let compare = run_cli(
        root.path(),
        &[
            "eval",
            "compare",
            "--baseline-id",
            "eval-cli-baseline",
            "--current-id",
            "eval-cli-current",
            "--json",
        ],
    );
    assert!(
        compare.status.success(),
        "stderr: {}",
        stderr_text(&compare)
    );
    let compared: Value = serde_json::from_str(&stdout_text(&compare)).unwrap();
    assert_eq!(compared["overall_classification"], "improved");

    let latest = run_cli(
        root.path(),
        &[
            "eval",
            "compare",
            "--current-id",
            "eval-cli-current",
            "--latest-baseline",
            "--json",
        ],
    );
    assert!(latest.status.success(), "stderr: {}", stderr_text(&latest));
    let latest_json: Value = serde_json::from_str(&stdout_text(&latest)).unwrap();
    assert_eq!(latest_json["baseline_eval_id"], "eval-cli-baseline");

    let bad_compare = run_cli(
        root.path(),
        &[
            "eval",
            "compare",
            "--baseline-id",
            "eval-cli-baseline",
            "--current-id",
            "eval-cli-current",
            "--latest-baseline",
        ],
    );
    assert!(!bad_compare.status.success());
    assert!(stderr_text(&bad_compare).contains("pass either --baseline-id or --latest-baseline"));
}

#[tokio::test]
async fn eval_mcp_round_trip_and_no_eval_backward_compatibility() {
    let root = make_project().await;
    let _pool = setup_eval_scope(root.path()).await;
    let mut session = McpSession::start(root.path());

    let empty = session.call_tool("state_eval_list", json!({"task": "TASK-EVAL-INT"}));
    assert!(!empty.is_error);
    assert_eq!(empty.payload.as_array().unwrap().len(), 0);

    let baseline = session.call_tool(
        "state_eval_create",
        json!({
            "id": "eval-mcp-baseline-int",
            "evaluator": "reviewer",
            "target_kind": "task",
            "target_ref": "TASK-EVAL-INT",
            "outcome": "warn",
            "overall_score": 0.51,
            "dimensions": [
                {"name": "validation", "status": "warn", "score": 0.51}
            ]
        }),
    );
    assert!(!baseline.is_error);
    assert_eq!(baseline.payload["run"]["id"], "eval-mcp-baseline-int");

    let current = session.call_tool(
        "state_eval_create",
        json!({
            "id": "eval-mcp-current-int",
            "evaluator": "reviewer",
            "target_kind": "task",
            "target_ref": "TASK-EVAL-INT",
            "outcome": "pass",
            "overall_score": 0.95,
            "dimensions": [
                {"name": "validation_coverage", "status": "pass", "score": 0.95}
            ],
            "links": [
                {"kind": "eval_run", "ref": "eval-mcp-baseline-int", "relation": "baseline"}
            ]
        }),
    );
    assert!(!current.is_error);

    let listed = session.call_tool("state_eval_list", json!({"task": "TASK-EVAL-INT"}));
    assert!(!listed.is_error);
    assert_eq!(listed.payload.as_array().unwrap().len(), 2);

    let fetched = session.call_tool("state_eval_get", json!({"id": "eval-mcp-current-int"}));
    assert!(!fetched.is_error);
    assert_eq!(fetched.payload["run"]["id"], "eval-mcp-current-int");

    let compared = session.call_tool(
        "state_eval_compare",
        json!({
            "baseline_eval_id": "eval-mcp-baseline-int",
            "current_eval_id": "eval-mcp-current-int"
        }),
    );
    assert!(!compared.is_error);
    assert_eq!(compared.payload["overall_classification"], "improved");

    let latest = session.call_tool(
        "state_eval_latest_baseline",
        json!({"current_eval_id": "eval-mcp-current-int"}),
    );
    assert!(!latest.is_error);
    assert_eq!(latest.payload["baseline_eval_id"], "eval-mcp-baseline-int");

    let missing = session.call_tool("state_eval_get", json!({"id": "missing-eval"}));
    assert!(missing.is_error);
    assert!(missing.payload["error"]
        .as_str()
        .unwrap()
        .contains("not found"));

    session.shutdown();
}
