#![allow(dead_code)]

#[path = "../src/sdd/mod.rs"]
mod sdd;

use serde_json::{json, Value};
use sqlx::SqlitePool;
use std::io::{BufRead, BufReader, Write};
use std::path::Path;
use std::process::{Child, ChildStdin, ChildStdout, Command, Output, Stdio};
use tempfile::TempDir;

use sdd::{
    event::query_events,
    spec::{create_spec, get_spec, update_spec_ac},
    task::{create_task, get_task},
    workflow::{approve_spec, start_spec},
};

fn spex_bin() -> &'static str {
    env!("CARGO_BIN_EXE_spex")
}

async fn make_project() -> TempDir {
    let root = tempfile::tempdir().expect("temp project must be created");
    std::fs::create_dir_all(root.path().join("docs")).expect("docs dir must be created");
    std::fs::write(
        root.path().join("docs/PRD.md"),
        "# Test PRD\n\nThis fixture is intentionally non-template.\n",
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

#[tokio::test]
async fn cli_spec_done_surfaces_invariant_failure_without_partial_persistence() {
    let root = make_project().await;
    let pool = open_pool(root.path()).await;

    create_spec(&pool, "SPEC-CLI", "CLI invariant", "P0", &[])
        .await
        .unwrap();
    approve_spec(&pool, "SPEC-CLI", "human").await.unwrap();
    start_spec(&pool, "SPEC-CLI", "human").await.unwrap();
    create_task(
        &pool,
        "TASK-CLI-OPEN",
        "SPEC-CLI",
        "Still open",
        "sdd-builder",
        &[],
        None,
    )
    .await
    .unwrap();
    update_spec_ac(&pool, "SPEC-CLI", 1, 1).await.unwrap();
    pool.close().await;

    let output = run_cli(root.path(), &["spec", "done", "SPEC-CLI"]);
    assert!(
        !output.status.success(),
        "spec done must fail when tasks remain open"
    );

    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("task(s) are still open"),
        "stderr was: {stderr}"
    );
    assert!(stderr.contains("TASK-CLI-OPEN"), "stderr was: {stderr}");

    let pool = open_pool(root.path()).await;
    assert_eq!(
        get_spec(&pool, "SPEC-CLI").await.unwrap().unwrap().status,
        "in_progress"
    );
    assert!(query_events(
        &pool,
        Some("SpecCompleted"),
        Some("SPEC-CLI"),
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
async fn mcp_lifecycle_mutations_surface_shared_workflow_events() {
    let root = make_project().await;
    let mut session = McpSession::start(root.path());

    let created = session.call_tool(
        "state_slice_create",
        json!({"id": "SPEC-MCP", "title": "MCP lifecycle", "priority": "P0"}),
    );
    assert!(!created.is_error);
    assert_eq!(created.payload["status"], "draft");

    let approved = session.call_tool(
        "state_slice_update",
        json!({"id": "SPEC-MCP", "status": "approved", "updated_by": "mcp-agent"}),
    );
    assert!(!approved.is_error);
    assert_eq!(approved.payload["status"], "approved");

    let started = session.call_tool(
        "state_slice_update",
        json!({"id": "SPEC-MCP", "status": "in_progress", "updated_by": "mcp-agent"}),
    );
    assert!(!started.is_error);
    assert_eq!(started.payload["status"], "in_progress");

    let task = session.call_tool(
        "state_task_create",
        json!({
            "id": "TASK-MCP",
            "spec": "SPEC-MCP",
            "title": "Exercise MCP workflow",
            "agent": "sdd-builder"
        }),
    );
    assert!(!task.is_error);
    assert_eq!(task.payload["status"], "pending");

    let task_started = session.call_tool(
        "state_task_update",
        json!({"id": "TASK-MCP", "status": "in_progress"}),
    );
    assert!(!task_started.is_error);
    assert_eq!(task_started.payload["status"], "in_progress");

    let task_done = session.call_tool(
        "state_task_update",
        json!({"id": "TASK-MCP", "status": "done"}),
    );
    assert!(!task_done.is_error);
    assert_eq!(task_done.payload["status"], "done");

    let ac_updated = session.call_tool(
        "state_slice_update",
        json!({"id": "SPEC-MCP", "ac_total": 1, "ac_passed": 1}),
    );
    assert!(!ac_updated.is_error);
    assert_eq!(ac_updated.payload["ac_passed"], 1);

    let done = session.call_tool(
        "state_slice_update",
        json!({"id": "SPEC-MCP", "status": "done", "updated_by": "mcp-agent"}),
    );
    assert!(!done.is_error);
    assert_eq!(done.payload["status"], "done");

    session.shutdown();

    let pool = open_pool(root.path()).await;
    assert_eq!(
        query_events(
            &pool,
            Some("SpecApproved"),
            Some("SPEC-MCP"),
            Some("mcp-agent"),
            None,
            None,
            None,
            None,
        )
        .await
        .unwrap()
        .len(),
        1
    );
    assert_eq!(
        query_events(
            &pool,
            Some("SpecStarted"),
            Some("SPEC-MCP"),
            Some("mcp-agent"),
            None,
            None,
            None,
            None,
        )
        .await
        .unwrap()
        .len(),
        1
    );
    assert_eq!(
        query_events(
            &pool,
            Some("TaskStarted"),
            Some("SPEC-MCP"),
            Some("sdd-builder"),
            None,
            None,
            None,
            None,
        )
        .await
        .unwrap()
        .len(),
        1
    );
    assert_eq!(
        query_events(
            &pool,
            Some("TaskCompleted"),
            Some("SPEC-MCP"),
            Some("sdd-builder"),
            None,
            None,
            None,
            None,
        )
        .await
        .unwrap()
        .len(),
        1
    );
    assert_eq!(
        query_events(
            &pool,
            Some("SpecCompleted"),
            Some("SPEC-MCP"),
            Some("mcp-agent"),
            None,
            None,
            None,
            None,
        )
        .await
        .unwrap()
        .len(),
        1
    );
}

#[tokio::test]
async fn mcp_done_failure_is_reported_without_state_or_event_drift() {
    let root = make_project().await;
    let mut session = McpSession::start(root.path());

    assert!(
        !session
            .call_tool(
                "state_slice_create",
                json!({"id": "SPEC-MCP-FAIL", "title": "MCP rollback", "priority": "P0"}),
            )
            .is_error
    );
    assert!(
        !session
            .call_tool(
                "state_slice_update",
                json!({"id": "SPEC-MCP-FAIL", "status": "approved", "updated_by": "mcp-agent"}),
            )
            .is_error
    );
    assert!(
        !session
            .call_tool(
                "state_slice_update",
                json!({"id": "SPEC-MCP-FAIL", "status": "in_progress", "updated_by": "mcp-agent"}),
            )
            .is_error
    );
    assert!(
        !session
            .call_tool(
                "state_task_create",
                json!({
                    "id": "TASK-MCP-OPEN",
                    "spec": "SPEC-MCP-FAIL",
                    "title": "Remain open",
                    "agent": "sdd-builder"
                }),
            )
            .is_error
    );
    assert!(
        !session
            .call_tool(
                "state_slice_update",
                json!({"id": "SPEC-MCP-FAIL", "ac_total": 1, "ac_passed": 1}),
            )
            .is_error
    );

    let failed = session.call_tool(
        "state_slice_update",
        json!({"id": "SPEC-MCP-FAIL", "status": "done", "updated_by": "mcp-agent"}),
    );
    assert!(failed.is_error, "MCP should surface invariant failure");

    let error = failed.payload["error"]
        .as_str()
        .expect("error payload must contain string");
    assert!(
        error.contains("task(s) are still open"),
        "error was: {error}"
    );
    assert!(error.contains("TASK-MCP-OPEN"), "error was: {error}");

    session.shutdown();

    let pool = open_pool(root.path()).await;
    assert_eq!(
        get_spec(&pool, "SPEC-MCP-FAIL")
            .await
            .unwrap()
            .unwrap()
            .status,
        "in_progress"
    );
    assert_eq!(
        get_task(&pool, "TASK-MCP-OPEN")
            .await
            .unwrap()
            .unwrap()
            .status,
        "pending"
    );
    assert!(query_events(
        &pool,
        Some("SpecCompleted"),
        Some("SPEC-MCP-FAIL"),
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
async fn doctor_surfaces_control_plane_invariant_fixture() {
    let root = make_project().await;
    let pool = open_pool(root.path()).await;

    sqlx::query(
        "INSERT INTO specs (id, title, status, priority, depends_on, agents, ac_total, ac_passed, created_at, updated_at, updated_by) VALUES ('SPEC-DRIFT', 'Drifted spec', 'done', 'P0', '[]', '[]', 1, 1, '2026-01-01T00:00:00Z', '2026-01-01T00:00:00Z', 'tester')",
    )
    .execute(&pool)
    .await
    .unwrap();
    sqlx::query(
        "INSERT INTO tasks (id, spec, title, agent, status, inputs, output_artifact, created_at, updated_at) VALUES ('TASK-DRIFT', 'SPEC-DRIFT', 'Pending task', 'sdd-builder', 'pending', '[]', NULL, '2026-01-01T00:00:00Z', '2026-01-01T00:00:00Z')",
    )
    .execute(&pool)
    .await
    .unwrap();
    pool.close().await;

    let output = run_cli(root.path(), &["doctor"]);
    assert!(
        !output.status.success(),
        "doctor must fail on invariant drift"
    );

    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(
        stdout.contains("Control-plane invariants"),
        "stdout was: {stdout}"
    );
    assert!(
        stdout.contains("done specs with unfinished tasks"),
        "stdout was: {stdout}"
    );
    assert!(
        stdout.contains("SPEC-DRIFT=[TASK-DRIFT(pending)]"),
        "stdout was: {stdout}"
    );
    assert!(
        stdout.contains("specs missing SpecStarted events"),
        "stdout was: {stdout}"
    );
}
