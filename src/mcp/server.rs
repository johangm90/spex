use anyhow::Result;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use sqlx::SqlitePool;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};

use crate::sdd::{
    artifact::{query_artifacts, register_artifact},
    event::{emit_event, query_events},
    memory::{
        memory_context, memory_delete, memory_get_full, memory_list, memory_search, memory_set,
        memory_stats,
    },
    spec::{
        create_spec, get_spec, list_specs, update_spec_ac, update_spec_agents, update_spec_status,
        SpecStatus,
    },
    task::{create_task, get_task, list_tasks, update_task_output_artifact, update_task_status, TaskStatus},
};

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

fn parse_memory_value(raw: &str) -> Value {
    serde_json::from_str(raw).unwrap_or_else(|_| Value::String(raw.to_string()))
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
            Err(e) => JsonRpcResponse::error(None, -32700, format!("Parse error: {}", e)),
            Ok(req) => {
                let id = req.id.clone();
                handle_request(&pool, req)
                    .await
                    .unwrap_or_else(|e| JsonRpcResponse::error(id, -32603, e.to_string()))
            }
        };

        let response_str = serde_json::to_string(&response)?;
        writer.write_all(response_str.as_bytes()).await?;
        writer.write_all(b"\n").await?;
        writer.flush().await?;
    }

    Ok(())
}

async fn handle_request(pool: &SqlitePool, req: JsonRpcRequest) -> Result<JsonRpcResponse> {
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
            Ok(JsonRpcResponse::success(id, result))
        }

        "notifications/initialized" => {
            // No response needed for notifications, but we still return something
            // that won't be sent (we can just ignore it in the loop)
            // Actually for JSON-RPC, notifications have no id, so we shouldn't respond
            Ok(JsonRpcResponse::success(None, json!(null)))
        }

        "tools/list" => {
            let tools = build_tools_list();
            Ok(JsonRpcResponse::success(id, json!({ "tools": tools })))
        }

        "tools/call" => {
            let params = req.params.unwrap_or(json!({}));
            let tool_name = params
                .get("name")
                .and_then(|v| v.as_str())
                .ok_or_else(|| anyhow::anyhow!("Missing tool name"))?
                .to_string();

            let arguments = params.get("arguments").cloned().unwrap_or(json!({}));

            let result = dispatch_tool(pool, &tool_name, arguments).await;
            match result {
                Ok(value) => Ok(JsonRpcResponse::success(id, tool_content(value))),
                Err(e) => Ok(JsonRpcResponse::success(
                    id,
                    tool_error_content(&e.to_string()),
                )),
            }
        }

        _ => Ok(JsonRpcResponse::error(
            id,
            -32601,
            format!("Method not found: {}", req.method),
        )),
    }
}

async fn dispatch_tool(pool: &SqlitePool, name: &str, args: Value) -> Result<Value> {
    match name {
        "state_snapshot" => {
            let specs = list_specs(pool, None, None).await?;
            let tasks = list_tasks(pool, None, None, None).await?;
            let events = query_events(pool, None, None, None, Some(10), None, None, None).await?;
            let project_dir = detect_project_dir();
            let config_source = detect_config_source(&project_dir);

            let mut payload = json!({
                "specs": specs,
                "tasks": tasks,
                "recent_events": events,
                "project_dir": project_dir,
                "config_source": config_source
            });

            if payload
                .get("config_source")
                .and_then(|v| v.as_str())
                .unwrap_or("unknown")
                == "global-opencode.json"
            {
                payload["isolation_warning"] = json!(
                    "MCP is configured globally; run `spex mcp setup` in this project for isolation."
                );
            }

            Ok(payload)
        }

        "state_slice_get" => {
            let id = args.get("id").and_then(|v| v.as_str());
            if let Some(id) = id {
                let spec = get_spec(pool, id).await?;
                Ok(json!(spec))
            } else {
                let limit = args.get("limit").and_then(|v| v.as_i64());
                let offset = args.get("offset").and_then(|v| v.as_i64());
                let specs = list_specs(pool, limit, offset).await?;
                Ok(json!(specs))
            }
        }

        "state_slice_create" => {
            let id = args
                .get("id")
                .and_then(|v| v.as_str())
                .ok_or_else(|| anyhow::anyhow!("Missing field: id"))?;
            let title = args
                .get("title")
                .and_then(|v| v.as_str())
                .ok_or_else(|| anyhow::anyhow!("Missing field: title"))?;
            let priority = args
                .get("priority")
                .and_then(|v| v.as_str())
                .unwrap_or("P1");
            let depends_on: Vec<String> = args
                .get("depends_on")
                .and_then(|v| v.as_array())
                .map(|a| {
                    a.iter()
                        .filter_map(|v| v.as_str().map(String::from))
                        .collect()
                })
                .unwrap_or_default();

            let spec = create_spec(pool, id, title, priority, &depends_on).await?;

            // Optionally update agents
            if let Some(agents_arr) = args.get("agents").and_then(|v| v.as_array()) {
                let agents: Vec<String> = agents_arr
                    .iter()
                    .filter_map(|v| v.as_str().map(String::from))
                    .collect();
                update_spec_agents(pool, id, &agents).await?;
            }

            Ok(json!(spec))
        }

        "state_slice_update" => {
            let id = args
                .get("id")
                .and_then(|v| v.as_str())
                .ok_or_else(|| anyhow::anyhow!("Missing field: id"))?;

            let updated_by = args
                .get("updated_by")
                .and_then(|v| v.as_str())
                .unwrap_or("agent");

            if let Some(status) = args.get("status").and_then(|v| v.as_str()) {
                SpecStatus::from_str(status).ok_or_else(|| {
                    anyhow::anyhow!(
                        "Invalid spec status '{}'. Valid: draft, approved, in_progress, done, paused",
                        status
                    )
                })?;
                update_spec_status(pool, id, status, updated_by).await?;
            }

            if let Some(ac_total) = args.get("ac_total").and_then(|v| v.as_i64()) {
                let ac_passed = args.get("ac_passed").and_then(|v| v.as_i64()).unwrap_or(0);
                update_spec_ac(pool, id, ac_total, ac_passed).await?;
            } else if let Some(ac_passed) = args.get("ac_passed").and_then(|v| v.as_i64()) {
                let spec = get_spec(pool, id)
                    .await?
                    .ok_or_else(|| anyhow::anyhow!("Spec not found: {}", id))?;
                update_spec_ac(pool, id, spec.ac_total, ac_passed).await?;
            }

            if let Some(agents_arr) = args.get("agents").and_then(|v| v.as_array()) {
                let agents: Vec<String> = agents_arr
                    .iter()
                    .filter_map(|v| v.as_str().map(String::from))
                    .collect();
                update_spec_agents(pool, id, &agents).await?;
            }

            let spec = get_spec(pool, id)
                .await?
                .ok_or_else(|| anyhow::anyhow!("Spec not found: {}", id))?;
            Ok(json!(spec))
        }

        "state_task_get" => {
            let id = args.get("id").and_then(|v| v.as_str());
            let spec = args.get("spec").and_then(|v| v.as_str());

            if let Some(id) = id {
                let task = get_task(pool, id).await?;
                Ok(json!(task))
            } else {
                let limit = args.get("limit").and_then(|v| v.as_i64());
                let offset = args.get("offset").and_then(|v| v.as_i64());
                let tasks = list_tasks(pool, spec, limit, offset).await?;
                Ok(json!(tasks))
            }
        }

        "state_task_create" => {
            let id = args
                .get("id")
                .and_then(|v| v.as_str())
                .ok_or_else(|| anyhow::anyhow!("Missing field: id"))?;
            let spec = args
                .get("spec")
                .and_then(|v| v.as_str())
                .ok_or_else(|| anyhow::anyhow!("Missing field: spec"))?;
            let title = args
                .get("title")
                .and_then(|v| v.as_str())
                .ok_or_else(|| anyhow::anyhow!("Missing field: title"))?;
            let agent = args
                .get("agent")
                .and_then(|v| v.as_str())
                .ok_or_else(|| anyhow::anyhow!("Missing field: agent"))?;
            let inputs: Vec<String> = args
                .get("inputs")
                .and_then(|v| v.as_array())
                .map(|a| {
                    a.iter()
                        .filter_map(|v| v.as_str().map(String::from))
                        .collect()
                })
                .unwrap_or_default();
            let output_artifact = args.get("output_artifact").and_then(|v| v.as_str());

            let task = create_task(pool, id, spec, title, agent, &inputs, output_artifact).await?;
            Ok(json!(task))
        }

        "state_task_update" => {
            let id = args
                .get("id")
                .and_then(|v| v.as_str())
                .ok_or_else(|| anyhow::anyhow!("Missing field: id"))?;

            if let Some(status) = args.get("status").and_then(|v| v.as_str()) {
                TaskStatus::from_str(status).ok_or_else(|| {
                    anyhow::anyhow!(
                        "Invalid task status '{}'. Valid: pending, in_progress, done, failed",
                        status
                    )
                })?;
                update_task_status(pool, id, status).await?;
            }

            if let Some(artifact) = args.get("output_artifact").and_then(|v| v.as_str()) {
                update_task_output_artifact(pool, id, artifact).await?;
            }

            let task = get_task(pool, id)
                .await?
                .ok_or_else(|| anyhow::anyhow!("Task not found: {}", id))?;
            Ok(json!(task))
        }

        "state_event_emit" => {
            let event_type = args
                .get("type")
                .and_then(|v| v.as_str())
                .ok_or_else(|| anyhow::anyhow!("Missing field: type"))?;
            let spec = args.get("spec").and_then(|v| v.as_str());
            let agent = args.get("agent").and_then(|v| v.as_str());
            let payload = args
                .get("payload")
                .map(|v| v.to_string())
                .unwrap_or_else(|| "{}".to_string());

            emit_event(pool, event_type, spec, agent, &payload).await?;
            Ok(json!({"ok": true}))
        }

        "state_event_query" => {
            let type_filter = args.get("type").and_then(|v| v.as_str());
            let spec_filter = args.get("spec").and_then(|v| v.as_str());
            let agent_filter = args.get("agent").and_then(|v| v.as_str());
            let limit = args.get("limit").and_then(|v| v.as_i64());
            let since = args.get("since").and_then(|v| v.as_str());
            let until = args.get("until").and_then(|v| v.as_str());
            let offset = args.get("offset").and_then(|v| v.as_i64());

            let events = query_events(
                pool,
                type_filter,
                spec_filter,
                agent_filter,
                limit,
                since,
                until,
                offset,
            )
            .await?;
            Ok(json!(events))
        }

        "memory_set" => {
            let agent = args
                .get("agent")
                .and_then(|v| v.as_str())
                .ok_or_else(|| anyhow::anyhow!("Missing field: agent"))?;
            let key = args
                .get("key")
                .and_then(|v| v.as_str())
                .ok_or_else(|| anyhow::anyhow!("Missing field: key"))?;
            let value = args
                .get("value")
                .ok_or_else(|| anyhow::anyhow!("Missing field: value"))?
                .to_string();
            let spec = args.get("spec").and_then(|v| v.as_str());
            let mem_type = args.get("type").and_then(|v| v.as_str());
            let ttl_seconds = args.get("ttl_seconds").and_then(|v| v.as_i64());

            memory_set(pool, agent, key, &value, spec, mem_type, ttl_seconds).await?;
            Ok(json!({"ok": true}))
        }

        "memory_get" => {
            let agent = args
                .get("agent")
                .and_then(|v| v.as_str())
                .ok_or_else(|| anyhow::anyhow!("Missing field: agent"))?;
            let key = args.get("key").and_then(|v| v.as_str());
            let spec = args.get("spec").and_then(|v| v.as_str());

            if let Some(key) = key {
                let memory = memory_get_full(pool, agent, key, spec).await?;
                if let Some(m) = memory {
                    let value = parse_memory_value(&m.value);
                    Ok(json!({
                        "value": value,
                        "type": m.type_,
                        "access_count": m.access_count,
                        "last_accessed_at": m.last_accessed_at,
                        "revision_count": m.revision_count,
                        "expires_at": m.expires_at,
                        "updated_at": m.updated_at,
                    }))
                } else {
                    Ok(json!({"value": null}))
                }
            } else {
                let entries = memory_list(pool, agent, spec, None, None, None).await?;
                let entries_obj: Vec<Value> = entries
                    .into_iter()
                    .map(|m| json!({"key": m.key, "value": parse_memory_value(&m.value)}))
                    .collect();
                Ok(json!({"entries": entries_obj}))
            }
        }

        "state_artifact_register" => {
            let id = args
                .get("id")
                .and_then(|v| v.as_str())
                .ok_or_else(|| anyhow::anyhow!("Missing field: id"))?;
            let spec = args.get("spec").and_then(|v| v.as_str());
            let agent = args
                .get("agent")
                .and_then(|v| v.as_str())
                .ok_or_else(|| anyhow::anyhow!("Missing field: agent"))?;
            let artifact_type = args
                .get("type")
                .and_then(|v| v.as_str())
                .ok_or_else(|| anyhow::anyhow!("Missing field: type"))?;
            let task = args.get("task").and_then(|v| v.as_str());
            let path = args.get("path").and_then(|v| v.as_str());
            let description = args.get("description").and_then(|v| v.as_str());

            let artifact = register_artifact(
                pool,
                id,
                spec,
                task,
                agent,
                artifact_type,
                path,
                description,
            )
            .await?;
            Ok(json!(artifact))
        }

        "state_artifact_query" => {
            let spec = args.get("spec").and_then(|v| v.as_str());
            let task = args.get("task").and_then(|v| v.as_str());
            let agent = args.get("agent").and_then(|v| v.as_str());
            let artifact_type = args.get("type").and_then(|v| v.as_str());

            let artifacts = query_artifacts(pool, spec, task, agent, artifact_type).await?;
            Ok(json!(artifacts))
        }

        "state_prd_get" => {
            // Read docs/PRD.md (source of truth is the file, not the DB)
            let project_dir = detect_project_dir();
            let prd_path = std::path::Path::new(&project_dir)
                .join("docs")
                .join("PRD.md");
            let (content, exists) = if prd_path.exists() {
                (std::fs::read_to_string(&prd_path).unwrap_or_default(), true)
            } else {
                (String::new(), false)
            };
            // Detect if PRD is still the default template (unfilled)
            let is_template = content.contains("<!-- What is this project?")
                || content.contains("<!-- Top 3 measurable goals")
                || content.contains("<!-- What is explicitly out of scope?")
                || (!exists);
            Ok(json!({
                "content": content,
                "path": prd_path.display().to_string(),
                "exists": exists,
                "is_template": is_template
            }))
        }

        "memory_search" => {
            let agent = args
                .get("agent")
                .and_then(|v| v.as_str())
                .ok_or_else(|| anyhow::anyhow!("Missing field: agent"))?;
            let query_str = args
                .get("query")
                .and_then(|v| v.as_str())
                .ok_or_else(|| anyhow::anyhow!("Missing field: query"))?;
            let spec = args.get("spec").and_then(|v| v.as_str());
            let mem_type = args.get("type").and_then(|v| v.as_str());
            let limit = args.get("limit").and_then(|v| v.as_i64());

            let results = memory_search(pool, agent, query_str, spec, mem_type, limit).await?;
            Ok(json!(results))
        }

        "memory_delete" => {
            let agent = args
                .get("agent")
                .and_then(|v| v.as_str())
                .ok_or_else(|| anyhow::anyhow!("Missing field: agent"))?;
            let key = args
                .get("key")
                .and_then(|v| v.as_str())
                .ok_or_else(|| anyhow::anyhow!("Missing field: key"))?;
            let spec = args.get("spec").and_then(|v| v.as_str());

            let deleted = memory_delete(pool, agent, key, spec).await?;
            Ok(json!({"deleted": deleted}))
        }

        "memory_context" => {
            let agent = args
                .get("agent")
                .and_then(|v| v.as_str())
                .ok_or_else(|| anyhow::anyhow!("Missing field: agent"))?;
            let spec = args.get("spec").and_then(|v| v.as_str());
            let limit = args.get("limit").and_then(|v| v.as_i64());

            let entries = memory_context(pool, agent, spec, limit).await?;
            Ok(json!(entries))
        }

        "memory_stats" => {
            let agent = args
                .get("agent")
                .and_then(|v| v.as_str())
                .ok_or_else(|| anyhow::anyhow!("Missing field: agent"))?;
            let spec = args.get("spec").and_then(|v| v.as_str());

            let stats = memory_stats(pool, agent, spec).await?;
            Ok(stats)
        }

        "memory_list" => {
            let agent = args
                .get("agent")
                .and_then(|v| v.as_str())
                .ok_or_else(|| anyhow::anyhow!("Missing field: agent"))?;
            let spec = args.get("spec").and_then(|v| v.as_str());
            let mem_type = args.get("type").and_then(|v| v.as_str());
            let limit = args.get("limit").and_then(|v| v.as_i64());
            let offset = args.get("offset").and_then(|v| v.as_i64());

            let entries = memory_list(pool, agent, spec, mem_type, limit, offset).await?;
            Ok(json!(entries))
        }

        _ => Err(anyhow::anyhow!("Unknown tool: {}", name)),
    }
}

fn build_tools_list() -> Value {
    json!([
        {
            "name": "state_snapshot",
            "description": "Returns a full project overview: constitution, specs, tasks, and recent events.",
            "inputSchema": {
                "type": "object",
                "properties": {}
            }
        },
        {
            "name": "state_slice_get",
            "description": "Get a specific slice/spec by ID, or list all.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "id": {"type": "string", "description": "Slice/Spec ID (optional; omit to list all)"},
                    "limit": {"type": "integer", "description": "Max results when listing all (omit for no limit)"},
                    "offset": {"type": "integer", "description": "Skip N results when listing all (requires limit)"}
                }
            }
        },
        {
            "name": "state_slice_create",
            "description": "Create a new slice/spec.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "id": {"type": "string"},
                    "title": {"type": "string"},
                    "priority": {"type": "string", "enum": ["P0", "P1", "P2", "P3"]},
                    "depends_on": {"type": "array", "items": {"type": "string"}},
                    "agents": {"type": "array", "items": {"type": "string"}}
                },
                "required": ["id", "title"]
            }
        },
        {
            "name": "state_slice_update",
            "description": "Update slice/spec status, AC counts, or agents.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "id": {"type": "string"},
                    "status": {"type": "string"},
                    "ac_total": {"type": "number"},
                    "ac_passed": {"type": "number"},
                    "agents": {"type": "array", "items": {"type": "string"}},
                    "updated_by": {"type": "string"}
                },
                "required": ["id"]
            }
        },
        {
            "name": "state_task_get",
            "description": "Get a task by ID, or list tasks (optionally filtered by spec).",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "id": {"type": "string"},
                    "spec": {"type": "string"},
                    "limit": {"type": "integer", "description": "Max results when listing (omit for no limit)"},
                    "offset": {"type": "integer", "description": "Skip N results when listing (requires limit)"}
                }
            }
        },
        {
            "name": "state_task_create",
            "description": "Create a new task within a spec.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "id": {"type": "string"},
                    "spec": {"type": "string"},
                    "title": {"type": "string"},
                    "agent": {"type": "string"},
                    "inputs": {"type": "array", "items": {"type": "string"}},
                    "output_artifact": {"type": "string"}
                },
                "required": ["id", "spec", "title", "agent"]
            }
        },
        {
            "name": "state_task_update",
            "description": "Update task status or output artifact.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "id": {"type": "string"},
                    "status": {"type": "string"},
                    "output_artifact": {"type": "string"}
                },
                "required": ["id"]
            }
        },
        {
            "name": "state_event_emit",
            "description": "Emit a domain event to the event log.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "type": {"type": "string"},
                    "spec": {"type": "string"},
                    "agent": {"type": "string"},
                    "payload": {"type": "object"}
                },
                "required": ["type"]
            }
        },
        {
            "name": "state_event_query",
            "description": "Query the event log with optional filters.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "type": {"type": "string"},
                    "spec": {"type": "string"},
                    "agent": {"type": "string"},
                    "limit": {"type": "number"},
                    "since": {"type": "string"},
                    "offset": {"type": "number"}
                }
            }
        },
        {
            "name": "memory_set",
            "description": "Set a key-value entry in an agent's scratchpad.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "agent": {"type": "string"},
                    "key": {"type": "string"},
                    "value": {},
                    "spec": {"type": "string"},
                    "type": {"type": "string", "enum": ["decision","architecture","bugfix","pattern","config","discovery","learning"]},
                    "ttl_seconds": {"type": "integer", "description": "Optional time-to-live in seconds from now"}
                },
                "required": ["agent", "key", "value"]
            }
        },
        {
            "name": "memory_get",
            "description": "Get a value from agent memory, or get all entries for an agent.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "agent": {"type": "string"},
                    "key": {"type": "string"},
                    "spec": {"type": "string", "description": "Optional scope for the memory key"}
                },
                "required": ["agent"]
            }
        },
        {
            "name": "state_artifact_register",
            "description": "Register an output artifact produced by an agent.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "id": {"type": "string"},
                    "spec": {"type": "string", "description": "Parent spec ID (optional for global/cross-spec artifacts such as registered agents)"},
                    "agent": {"type": "string"},
                    "type": {"type": "string"},
                    "task": {"type": "string"},
                    "path": {"type": "string"},
                    "description": {"type": "string"}
                },
                "required": ["id", "agent", "type"]
            }
        },
        {
            "name": "state_artifact_query",
            "description": "Query registered artifacts with optional filters.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "spec": {"type": "string"},
                    "task": {"type": "string"},
                    "agent": {"type": "string"},
                    "type": {"type": "string"}
                }
            }
        },
        {
            "name": "state_prd_get",
            "description": "Read PRD.md from the project root (at docs/PRD.md). Returns content, path, exists flag, and is_template flag (true if the file is still the default unfilled template).",
            "inputSchema": {
                "type": "object",
                "properties": {}
            }
        },
        {
            "name": "memory_search",
            "description": "Full-text search across agent memory entries using FTS5. Returns ranked results.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "agent": {"type": "string"},
                    "query": {"type": "string", "description": "FTS5 search query"},
                    "spec": {"type": "string"},
                    "type": {"type": "string", "enum": ["decision","architecture","bugfix","pattern","config","discovery","learning"]},
                    "limit": {"type": "integer", "default": 10}
                },
                "required": ["agent", "query"]
            }
        },
        {
            "name": "memory_delete",
            "description": "Soft-delete a memory entry. Deleted entries are hidden from all other tools.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "agent": {"type": "string"},
                    "key": {"type": "string"},
                    "spec": {"type": "string"}
                },
                "required": ["agent", "key"]
            }
        },
        {
            "name": "memory_context",
            "description": "Retrieve the most recently accessed memory entries for session recovery after context compaction.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "agent": {"type": "string"},
                    "spec": {"type": "string"},
                    "limit": {"type": "integer", "default": 10}
                },
                "required": ["agent"]
            }
        },
        {
            "name": "memory_stats",
            "description": "Get memory statistics for an agent: total entries, breakdown by type, most accessed key, last write time.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "agent": {"type": "string"},
                    "spec": {"type": "string"}
                },
                "required": ["agent"]
            }
        },
        {
            "name": "memory_list",
            "description": "List memory entries for an agent with optional filters. Returns full Memory objects including metadata.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "agent": {"type": "string"},
                    "spec": {"type": "string"},
                    "type": {"type": "string", "enum": ["decision","architecture","bugfix","pattern","config","discovery","learning"]},
                    "limit": {"type": "integer", "default": 100},
                    "offset": {"type": "integer", "default": 0}
                },
                "required": ["agent"]
            }
        }
    ])
}

fn detect_project_dir() -> String {
    std::env::current_dir()
        .ok()
        .and_then(|p| p.canonicalize().ok())
        .unwrap_or_else(|| PathBuf::from("."))
        .display()
        .to_string()
}

fn detect_config_source(project_dir: &str) -> &'static str {
    if std::env::var_os("OPENCODE_CONFIG").is_some() {
        return "env";
    }

    let local_path = Path::new(project_dir).join("opencode.json");
    if local_path.exists() {
        return "local-opencode.json";
    }

    if let Some(opencode_dir) = crate::cli::util::opencode_config_dir() {
        let global_path = opencode_dir.join("config.json");
        if global_path.exists() {
            return "global-opencode.json";
        }
    }

    "unknown"
}

#[cfg(test)]
mod tests {
    use super::*;
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

        assert_eq!(emit_result["ok"].as_bool().unwrap(), true);

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
        let result = resp.result.expect("expected result");
        assert!(result.get("protocolVersion").is_some());
        assert!(result.get("capabilities").is_some());
        assert!(result.get("serverInfo").is_some());
    }

    #[tokio::test]
    async fn handle_request_tools_list_has_18_items() {
        let pool = make_pool().await;
        let req = JsonRpcRequest {
            jsonrpc: "2.0".to_string(),
            id: Some(json!(2)),
            method: "tools/list".to_string(),
            params: None,
        };
        let resp = handle_request(&pool, req).await.unwrap();
        let result = resp.result.expect("expected result");
        let tools = result["tools"].as_array().expect("tools must be array");
        assert_eq!(tools.len(), 19, "expected 19 tools, got {}", tools.len());
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
        assert!(resp.result.is_none(), "expected no result");
        let err = resp.error.expect("expected error");
        assert_eq!(err.code, -32601);
        assert!(err.message.contains("bogus/method"));
    }
}
