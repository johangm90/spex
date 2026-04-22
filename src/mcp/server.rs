use anyhow::Result;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use sqlx::SqlitePool;
use std::sync::Arc;
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};

use crate::mcp::tools;

#[derive(Debug, Deserialize)]
#[allow(dead_code)]
struct JsonRpcRequest {
    jsonrpc: String,
    id: Option<Value>,
    method: String,
    params: Option<Value>,
}

#[derive(Debug, Serialize)]
struct JsonRpcResponse {
    jsonrpc: String,
    id: Option<Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    result: Option<Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    error: Option<JsonRpcError>,
}

#[derive(Debug, Serialize)]
struct JsonRpcError {
    code: i32,
    message: String,
}

impl JsonRpcResponse {
    fn success(id: Option<Value>, result: Value) -> Self {
        Self {
            jsonrpc: "2.0".to_string(),
            id,
            result: Some(result),
            error: None,
        }
    }

    fn error(id: Option<Value>, code: i32, message: String) -> Self {
        Self {
            jsonrpc: "2.0".to_string(),
            id,
            result: None,
            error: Some(JsonRpcError { code, message }),
        }
    }
}

fn tool_content(value: Value) -> Value {
    json!({
        "content": [{"type": "text", "text": value.to_string()}]
    })
}

fn tool_error_content(msg: &str) -> Value {
    json!({
        "content": [{"type": "text", "text": format!("{{\"error\": \"{}\"}}", msg)}],
        "isError": true
    })
}

#[cfg(test)]
async fn dispatch_tool(pool: &SqlitePool, name: &str, args: Value) -> Result<Value> {
    tools::dispatch_tool(pool, name, args).await
}

#[cfg(test)]
fn build_tools_list() -> Value {
    tools::build_tools_list()
}

pub async fn run_mcp_server(pool: Arc<SqlitePool>) -> Result<()> {
    let stdin = tokio::io::stdin();
    let stdout = tokio::io::stdout();
    let mut reader = BufReader::new(stdin);
    let mut writer = tokio::io::BufWriter::new(stdout);
    let mut line = String::new();

    loop {
        line.clear();
        let bytes_read = reader.read_line(&mut line).await?;
        if bytes_read == 0 {
            break; // EOF
        }

        let trimmed = line.trim();
        if trimmed.is_empty() {
            continue;
        }

        let response = match serde_json::from_str::<JsonRpcRequest>(trimmed) {
            Err(e) => Some(JsonRpcResponse::error(
                None,
                -32700,
                format!("Parse error: {}", e),
            )),
            Ok(req) => {
                let id = req.id.clone();
                handle_request(&pool, req)
                    .await
                    .unwrap_or_else(|e| Some(JsonRpcResponse::error(id, -32603, e.to_string())))
            }
        };

        if let Some(response) = response {
            let response_str = serde_json::to_string(&response)?;
            writer.write_all(response_str.as_bytes()).await?;
            writer.write_all(b"\n").await?;
            writer.flush().await?;
        }
    }

    Ok(())
}

async fn handle_request(pool: &SqlitePool, req: JsonRpcRequest) -> Result<Option<JsonRpcResponse>> {
    let id = req.id.clone();

    match req.method.as_str() {
        "initialize" => {
            let result = json!({
                "protocolVersion": "2024-11-05",
                "capabilities": {
                    "tools": {}
                },
                "serverInfo": {
                    "name": "spex-state",
                    "version": "0.1.0"
                }
            });
            Ok(Some(JsonRpcResponse::success(id, result)))
        }

        "notifications/initialized" => {
            // JSON-RPC 2.0: notifications (no id) must NOT receive a response.
            // Return None to signal the caller to skip writing to stdout.
            #[allow(clippy::needless_return)]
            return Ok(None);
        }

        "tools/list" => {
            let tools = tools::build_tools_list();
            Ok(Some(JsonRpcResponse::success(
                id,
                json!({ "tools": tools }),
            )))
        }

        "tools/call" => {
            let params = req.params.unwrap_or(json!({}));
            let tool_name = params
                .get("name")
                .and_then(|v| v.as_str())
                .ok_or_else(|| anyhow::anyhow!("Missing tool name"))?
                .to_string();

            let arguments = params.get("arguments").cloned().unwrap_or(json!({}));

            let result = tools::dispatch_tool(pool, &tool_name, arguments).await;
            match result {
                Ok(value) => Ok(Some(JsonRpcResponse::success(id, tool_content(value)))),
                Err(e) => Ok(Some(JsonRpcResponse::success(
                    id,
                    tool_error_content(&e.to_string()),
                ))),
            }
        }

        _ => Ok(Some(JsonRpcResponse::error(
            id,
            -32601,
            format!("Method not found: {}", req.method),
        ))),
    }
}

pub(crate) fn canonical_tool_names() -> Vec<String> {
    tools::canonical_tool_names()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::sdd::event::query_events;
    use crate::sdd::memory::{memory_get_full, memory_set};
    use crate::sdd::test_helpers::make_pool;

    #[tokio::test]
    async fn state_snapshot_returns_required_keys() {
        let pool = make_pool().await;
        let result = dispatch_tool(&pool, "state_snapshot", json!({}))
            .await
            .unwrap();
        assert!(result.get("specs").is_some(), "missing 'specs' key");
        assert!(result.get("tasks").is_some(), "missing 'tasks' key");
        assert!(
            result.get("recent_events").is_some(),
            "missing 'recent_events' key"
        );
        assert!(result.get("artifacts").is_some(), "missing 'artifacts' key");
        assert!(result.get("agents").is_some(), "missing 'agents' key");
        assert!(result.get("active_project").is_some());
        assert!(result.get("project_profile").is_some());
        assert!(result.get("subprojects_summary").is_some());
        assert!(result.get("repo_map").is_some());
        assert!(result.get("validation_commands").is_some());
        assert!(
            result.get("memory_stats").is_none(),
            "memory_stats must be absent without agent param"
        );
    }

    #[tokio::test]
    async fn state_project_context_persists_bootstrap_memory() {
        let pool = make_pool().await;

        let result = dispatch_tool(&pool, "state_project_context", json!({}))
            .await
            .unwrap();

        assert!(result.get("project_profile").is_some());

        let profile = memory_get_full(&pool, "spex-architect", "project_profile", None)
            .await
            .unwrap()
            .expect("project_profile must be stored");
        let repo_map = memory_get_full(&pool, "spex-architect", "repo_map", None)
            .await
            .unwrap()
            .expect("repo_map must be stored");

        assert!(!profile.value.is_empty());
        assert!(!repo_map.value.is_empty());
    }

    #[tokio::test]
    async fn state_project_context_supports_subpath() {
        let pool = make_pool().await;
        let root = tempfile::tempdir().unwrap();
        std::fs::write(
            root.path().join("package.json"),
            r#"{"private": true, "workspaces": ["apps/*"]}"#,
        )
        .unwrap();
        std::fs::create_dir_all(root.path().join("apps/web/src")).unwrap();
        std::fs::write(
            root.path().join("apps/web/package.json"),
            r#"{"dependencies": {"next": "14.0.0"}, "scripts": {"test": "vitest"}}"#,
        )
        .unwrap();

        let previous = std::env::current_dir().unwrap();
        std::env::set_current_dir(root.path()).unwrap();

        let result = dispatch_tool(
            &pool,
            "state_project_context",
            json!({"subpath": "apps/web"}),
        )
        .await
        .unwrap();

        std::env::set_current_dir(previous).unwrap();

        assert_eq!(
            result.get("subpath").and_then(|v| v.as_str()),
            Some("apps/web")
        );
        assert_eq!(result["active_project"]["name"], "web");
        assert!(result["project_profile"]["frameworks"]
            .as_array()
            .unwrap()
            .iter()
            .any(|v| v == "Next.js"));
        assert_eq!(result["validation_commands"]["primary"], "npm run test");
    }

    #[tokio::test]
    async fn state_snapshot_includes_subprojects_summary() {
        let pool = make_pool().await;
        let root = tempfile::tempdir().unwrap();
        std::fs::write(
            root.path().join("package.json"),
            r#"{"private": true, "workspaces": ["apps/*", "packages/*"]}"#,
        )
        .unwrap();
        std::fs::create_dir_all(root.path().join("apps/web")).unwrap();
        std::fs::create_dir_all(root.path().join("packages/core/src")).unwrap();
        std::fs::write(
            root.path().join("apps/web/package.json"),
            r#"{"dependencies": {"next": "14.0.0"}, "scripts": {"test": "vitest"}}"#,
        )
        .unwrap();
        std::fs::write(
            root.path().join("packages/core/Cargo.toml"),
            "[package]\nname='core'\nversion='0.1.0'\n",
        )
        .unwrap();
        std::fs::write(
            root.path().join("packages/core/src/lib.rs"),
            "pub fn x() {}\n",
        )
        .unwrap();

        let previous = std::env::current_dir().unwrap();
        std::env::set_current_dir(root.path()).unwrap();

        let result = dispatch_tool(&pool, "state_snapshot", json!({}))
            .await
            .unwrap();

        std::env::set_current_dir(previous).unwrap();

        assert_eq!(result["subprojects_summary"]["count"], 2);
        assert!(result["subprojects_summary"]["items"]
            .as_array()
            .unwrap()
            .iter()
            .any(|v| v["path"] == "apps/web" && v["primary_validation"] == "npm run test"));
        assert!(result["subprojects_summary"]["items"]
            .as_array()
            .unwrap()
            .iter()
            .any(|v| v["path"] == "packages/core" && v["primary_validation"] == "cargo test"));
    }

    #[tokio::test]
    async fn state_snapshot_includes_memory_stats_when_agent_given() {
        let pool = make_pool().await;
        memory_set(
            &pool,
            "alice",
            "k1",
            "v1",
            None,
            Some("decision"),
            None,
            None,
        )
        .await
        .unwrap();
        let result = dispatch_tool(&pool, "state_snapshot", json!({"agent": "alice"}))
            .await
            .unwrap();
        let stats = result.get("memory_stats").expect("missing memory_stats");
        assert_eq!(stats["total"], 1);
    }

    #[tokio::test]
    async fn state_slice_create_then_get() {
        let pool = make_pool().await;

        let created = dispatch_tool(
            &pool,
            "state_slice_create",
            json!({"id": "SPEC-001", "title": "Auth feature", "priority": "P0"}),
        )
        .await
        .unwrap();

        assert_eq!(created["id"].as_str().unwrap(), "SPEC-001");
        assert_eq!(created["title"].as_str().unwrap(), "Auth feature");

        let fetched = dispatch_tool(&pool, "state_slice_get", json!({"id": "SPEC-001"}))
            .await
            .unwrap();

        assert_eq!(fetched["id"].as_str().unwrap(), "SPEC-001");
        assert_eq!(fetched["title"].as_str().unwrap(), "Auth feature");
        assert_eq!(fetched["priority"].as_str().unwrap(), "P0");
    }

    #[tokio::test]
    async fn state_task_create_then_get() {
        let pool = make_pool().await;

        dispatch_tool(
            &pool,
            "state_slice_create",
            json!({"id": "SPEC-T01", "title": "Task parent spec"}),
        )
        .await
        .unwrap();

        let created = dispatch_tool(
            &pool,
            "state_task_create",
            json!({
                "id": "TASK-001",
                "spec": "SPEC-T01",
                "title": "Implement login",
                "agent": "sdd-builder"
            }),
        )
        .await
        .unwrap();

        assert_eq!(created["id"].as_str().unwrap(), "TASK-001");
        assert_eq!(created["spec"].as_str().unwrap(), "SPEC-T01");
        assert_eq!(created["title"].as_str().unwrap(), "Implement login");
        assert_eq!(created["agent"].as_str().unwrap(), "sdd-builder");

        let fetched = dispatch_tool(&pool, "state_task_get", json!({"id": "TASK-001"}))
            .await
            .unwrap();

        assert_eq!(fetched["id"].as_str().unwrap(), "TASK-001");
        assert_eq!(fetched["title"].as_str().unwrap(), "Implement login");
    }

    #[tokio::test]
    async fn state_slice_update_uses_transactional_workflow_events() {
        let pool = make_pool().await;

        dispatch_tool(
            &pool,
            "state_slice_create",
            json!({"id": "SPEC-MCP-1", "title": "MCP spec"}),
        )
        .await
        .unwrap();

        let updated = dispatch_tool(
            &pool,
            "state_slice_update",
            json!({"id": "SPEC-MCP-1", "status": "approved", "updated_by": "mcp-agent"}),
        )
        .await
        .unwrap();

        assert_eq!(updated["status"], "approved");

        let events = query_events(
            &pool,
            Some("SpecApproved"),
            Some("SPEC-MCP-1"),
            Some("mcp-agent"),
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
    async fn state_task_update_uses_transactional_workflow_events() {
        let pool = make_pool().await;

        dispatch_tool(
            &pool,
            "state_slice_create",
            json!({"id": "SPEC-MCP-2", "title": "MCP task spec"}),
        )
        .await
        .unwrap();
        dispatch_tool(
            &pool,
            "state_task_create",
            json!({
                "id": "TASK-MCP-1",
                "spec": "SPEC-MCP-2",
                "title": "Migrate handler",
                "agent": "sdd-builder"
            }),
        )
        .await
        .unwrap();

        let updated = dispatch_tool(
            &pool,
            "state_task_update",
            json!({"id": "TASK-MCP-1", "status": "in_progress"}),
        )
        .await
        .unwrap();

        assert_eq!(updated["status"], "in_progress");

        let events = query_events(
            &pool,
            Some("TaskStarted"),
            Some("SPEC-MCP-2"),
            Some("sdd-builder"),
            None,
            None,
            None,
            None,
        )
        .await
        .unwrap();
        assert_eq!(events.len(), 1);
        assert_eq!(events[0].payload, r#"{"task":"TASK-MCP-1"}"#);
    }

    #[tokio::test]
    async fn state_task_update_failed_to_pending_emits_replanned_event() {
        let pool = make_pool().await;

        dispatch_tool(
            &pool,
            "state_slice_create",
            json!({"id": "SPEC-MCP-3", "title": "MCP replan spec"}),
        )
        .await
        .unwrap();
        dispatch_tool(
            &pool,
            "state_task_create",
            json!({
                "id": "TASK-MCP-REPLAN",
                "spec": "SPEC-MCP-3",
                "title": "Retry handler",
                "agent": "sdd-builder"
            }),
        )
        .await
        .unwrap();
        dispatch_tool(
            &pool,
            "state_task_update",
            json!({"id": "TASK-MCP-REPLAN", "status": "in_progress"}),
        )
        .await
        .unwrap();
        dispatch_tool(
            &pool,
            "state_task_update",
            json!({"id": "TASK-MCP-REPLAN", "status": "failed"}),
        )
        .await
        .unwrap();

        let updated = dispatch_tool(
            &pool,
            "state_task_update",
            json!({"id": "TASK-MCP-REPLAN", "status": "pending"}),
        )
        .await
        .unwrap();

        assert_eq!(updated["status"], "pending");

        let events = query_events(
            &pool,
            Some("TaskReplanned"),
            Some("SPEC-MCP-3"),
            Some("sdd-builder"),
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
    async fn memory_set_then_get() {
        let pool = make_pool().await;

        dispatch_tool(
            &pool,
            "memory_set",
            json!({"agent": "test-agent", "key": "mykey", "value": "hello-world"}),
        )
        .await
        .unwrap();

        let fetched = dispatch_tool(
            &pool,
            "memory_get",
            json!({"agent": "test-agent", "key": "mykey"}),
        )
        .await
        .unwrap();

        assert_eq!(fetched["value"].as_str().unwrap(), "hello-world");
    }

    #[tokio::test]
    async fn state_event_emit_then_query() {
        let pool = make_pool().await;

        let emit_result = dispatch_tool(
            &pool,
            "state_event_emit",
            json!({"type": "test.event", "agent": "test-agent", "payload": {"msg": "hi"}}),
        )
        .await
        .unwrap();

        assert!(emit_result["ok"].as_bool().unwrap());

        let events = dispatch_tool(
            &pool,
            "state_event_query",
            json!({"type": "test.event", "limit": 5}),
        )
        .await
        .unwrap();

        let arr = events.as_array().unwrap();
        assert!(!arr.is_empty(), "expected at least one event");
        assert_eq!(arr[0]["type"].as_str().unwrap(), "test.event");
    }

    #[tokio::test]
    async fn unknown_tool_returns_err() {
        let pool = make_pool().await;
        let result = dispatch_tool(&pool, "nonexistent_tool", json!({})).await;
        assert!(result.is_err(), "expected Err for unknown tool");
        let msg = result.unwrap_err().to_string();
        assert!(
            msg.contains("Unknown tool"),
            "unexpected error message: {msg}"
        );
    }

    #[tokio::test]
    async fn handle_request_initialize() {
        let pool = make_pool().await;
        let req = JsonRpcRequest {
            jsonrpc: "2.0".to_string(),
            id: Some(json!(1)),
            method: "initialize".to_string(),
            params: None,
        };
        let resp = handle_request(&pool, req).await.unwrap();
        let result = resp.unwrap().result.expect("expected result");
        assert!(result.get("protocolVersion").is_some());
        assert!(result.get("capabilities").is_some());
        assert!(result.get("serverInfo").is_some());
    }

    #[tokio::test]
    async fn handle_request_tools_list_has_expected_count() {
        let pool = make_pool().await;
        let req = JsonRpcRequest {
            jsonrpc: "2.0".to_string(),
            id: Some(json!(2)),
            method: "tools/list".to_string(),
            params: None,
        };
        let resp = handle_request(&pool, req).await.unwrap();
        let result = resp.unwrap().result.expect("expected result");
        let tools = result["tools"].as_array().expect("tools must be array");
        assert_eq!(tools.len(), 23, "expected 23 tools, got {}", tools.len());
    }

    #[tokio::test]
    async fn handle_request_tools_call_valid_tool() {
        let pool = make_pool().await;
        let req = JsonRpcRequest {
            jsonrpc: "2.0".to_string(),
            id: Some(json!(3)),
            method: "tools/call".to_string(),
            params: Some(json!({
                "name": "state_snapshot",
                "arguments": {}
            })),
        };
        let resp = handle_request(&pool, req).await.unwrap();
        let resp = resp.unwrap();
        assert!(resp.error.is_none(), "expected no error");
        let result = resp.result.expect("expected result");
        let content = result["content"].as_array().expect("content must be array");
        assert!(!content.is_empty());
        assert_eq!(content[0]["type"].as_str().unwrap(), "text");
    }

    #[tokio::test]
    async fn handle_request_unknown_method_returns_error_code() {
        let pool = make_pool().await;
        let req = JsonRpcRequest {
            jsonrpc: "2.0".to_string(),
            id: Some(json!(99)),
            method: "bogus/method".to_string(),
            params: None,
        };
        let resp = handle_request(&pool, req).await.unwrap();
        let resp = resp.unwrap();
        assert!(resp.result.is_none(), "expected no result");
        let err = resp.error.expect("expected error");
        assert_eq!(err.code, -32601);
        assert!(err.message.contains("bogus/method"));
    }

    #[tokio::test]
    async fn state_event_query_schema_includes_until() {
        let tools = build_tools_list();
        let arr = tools.as_array().expect("tools list must be array");
        let tool = arr
            .iter()
            .find(|t| t["name"] == "state_event_query")
            .expect("state_event_query tool must exist");
        let props = &tool["inputSchema"]["properties"];
        assert!(
            props.get("until").is_some(),
            "state_event_query schema must declare 'until' property"
        );
    }

    #[tokio::test]
    async fn state_event_query_until_filters_events() {
        let pool = make_pool().await;

        dispatch_tool(
            &pool,
            "state_event_emit",
            json!({"type": "ts.test", "agent": "a1", "payload": {}}),
        )
        .await
        .unwrap();

        let events = dispatch_tool(
            &pool,
            "state_event_query",
            json!({"type": "ts.test", "until": "2000-01-01T00:00:00Z"}),
        )
        .await
        .unwrap();
        let arr = events.as_array().unwrap();
        assert!(
            arr.is_empty(),
            "until in the past should exclude all events"
        );
    }

    #[tokio::test]
    async fn memory_set_rejects_non_array_related_to() {
        let pool = make_pool().await;
        let result = dispatch_tool(
            &pool,
            "memory_set",
            json!({"agent": "a", "key": "k", "value": "v", "related_to": "not-an-array"}),
        )
        .await;
        assert!(result.is_err());
        let msg = result.unwrap_err().to_string();
        assert!(msg.contains("must be a JSON array"), "got: {msg}");
    }

    #[tokio::test]
    async fn memory_set_rejects_invalid_related_to_format() {
        let pool = make_pool().await;
        let result = dispatch_tool(
            &pool,
            "memory_set",
            json!({"agent": "a", "key": "k", "value": "v", "related_to": ["no-slash"]}),
        )
        .await;
        assert!(result.is_err());
        let msg = result.unwrap_err().to_string();
        assert!(msg.contains("agent/key"), "got: {msg}");
    }

    #[tokio::test]
    async fn memory_set_accepts_valid_related_to() {
        let pool = make_pool().await;
        let result = dispatch_tool(
            &pool,
            "memory_set",
            json!({"agent": "a", "key": "k", "value": "v", "related_to": ["bob/decision-1", "carol/pattern-2"]}),
        )
        .await;
        assert!(result.is_ok(), "valid related_to must be accepted");
    }
}
