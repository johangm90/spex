use anyhow::Result;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use sqlx::SqlitePool;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};

use crate::sdd::{
    artifact::{query_artifacts, register_artifact},
    context_gap::{create_context_gap, get_context_gap, list_context_gaps, update_context_gap},
    event::{emit_event, query_events},
    handoff_snapshot::{create_handoff_snapshot, get_handoff_snapshot, list_handoff_snapshots},
    incident::{create_incident, get_incident, list_incidents, update_incident},
    interrupt::{create_interrupt, get_interrupt, list_interrupts, update_interrupt},
    memory::{
        memory_context, memory_delete, memory_get_all, memory_get_full, memory_search, memory_set,
        memory_stats,
    },
    plan_version::{
        create_plan_version, get_plan_version, list_plan_versions, supersede_plan_versions,
    },
    replan_request::{
        create_replan_request, get_replan_request, list_replan_requests, update_replan_request,
    },
    scheduler::scheduler_next,
    spec::{
        create_spec, get_spec, list_specs, update_spec_ac, update_spec_agents, update_spec_status,
    },
    task::{
        create_task, get_task, list_tasks, update_task_metadata, update_task_output_artifact,
        update_task_status, TaskLockRequirement,
    },
    task_lease::{
        claim_task_lease, expire_stale_task_leases, get_task_lease, heartbeat_task_lease,
        list_task_leases, release_task_lease,
    },
    task_lock::{acquire_task_locks, query_task_locks, release_task_locks},
    verification_run::{create_verification_run, get_verification_run, list_verification_runs},
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

pub async fn run_mcp_server(pool: Arc<SqlitePool>, project_dir: String) -> Result<()> {
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
                handle_request(&pool, &project_dir, req)
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

async fn handle_request(pool: &SqlitePool, project_dir: &str, req: JsonRpcRequest) -> Result<JsonRpcResponse> {
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

            let result = dispatch_tool(pool, project_dir, &tool_name, arguments).await;
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

async fn dispatch_tool(pool: &SqlitePool, project_dir: &str, name: &str, args: Value) -> Result<Value> {
    match name {
        "state_snapshot" => {
            let specs = list_specs(pool, project_dir).await?;
            let tasks = list_tasks(pool, project_dir, None).await?;
            let events = query_events(pool, project_dir, None, None, None, Some(10), None, None).await?;
            let incidents = list_incidents(pool, project_dir, None, None).await?;
            let context_gaps = list_context_gaps(pool, project_dir, None, None).await?;
            let interrupts = list_interrupts(pool, project_dir, None, None).await?;
            let verification_runs = list_verification_runs(pool, project_dir, None, None, None).await?;
            let active_plan_versions = list_plan_versions(pool, project_dir, None).await?;
            let leases = list_task_leases(pool, project_dir, None).await?;
            let active_locks = query_task_locks(pool, project_dir, None, None, true).await?;
            let open_replans = list_replan_requests(pool, project_dir, None, Some("open")).await?;
            let open_incidents: Vec<_> = incidents
                .iter()
                .filter(|i| {
                    i.status != "resolved"
                        && i.status != "duplicate"
                        && i.status != "not_reproducible"
                })
                .cloned()
                .collect();
            let blocking_incidents: Vec<_> = open_incidents
                .iter()
                .filter(|i| i.blocking)
                .cloned()
                .collect();
            let open_context_gaps: Vec<_> = context_gaps
                .iter()
                .filter(|g| g.status != "resolved" && g.status != "wont_fix")
                .cloned()
                .collect();
            let blocking_context_gaps: Vec<_> = open_context_gaps
                .iter()
                .filter(|g| g.blocking)
                .cloned()
                .collect();
            let active_interrupts: Vec<_> = interrupts
                .iter()
                .filter(|it| it.status == "open" || it.status == "active")
                .cloned()
                .collect();
            let verification_failures: Vec<_> = verification_runs
                .iter()
                .filter(|v| v.status == "fail" || v.status == "blocked" || v.status == "flaky")
                .cloned()
                .collect();
            let config_source = detect_config_source(project_dir);

            let mut payload = json!({
                "specs": specs,
                "tasks": tasks,
                "recent_events": events,
                "operational_summary": {
                    "open_incidents": open_incidents.len(),
                    "blocking_incidents": blocking_incidents.len(),
                    "open_context_gaps": open_context_gaps.len(),
                    "blocking_context_gaps": blocking_context_gaps.len(),
                    "active_interrupts": active_interrupts.len(),
                    "verification_failures": verification_failures.len(),
                    "active_plan_versions": active_plan_versions.iter().filter(|p| p.status == "active").count(),
                    "active_leases": leases.iter().filter(|l| l.status == "claimed" || l.status == "running").count(),
                    "active_locks": active_locks.len(),
                    "open_replans": open_replans.len()
                },
                "blocking_records": {
                    "incidents": blocking_incidents.into_iter().take(10).collect::<Vec<_>>(),
                    "context_gaps": blocking_context_gaps.into_iter().take(10).collect::<Vec<_>>()
                },
                "active_interrupts": active_interrupts.into_iter().take(10).collect::<Vec<_>>(),
                "recent_verification_failures": verification_failures.into_iter().take(10).collect::<Vec<_>>(),
                "active_plan_versions": active_plan_versions.into_iter().filter(|p| p.status == "active").take(10).collect::<Vec<_>>(),
                "active_leases": leases.into_iter().filter(|l| l.status == "claimed" || l.status == "running").take(20).collect::<Vec<_>>(),
                "active_locks": active_locks.into_iter().take(20).collect::<Vec<_>>(),
                "open_replans": open_replans.into_iter().take(20).collect::<Vec<_>>(),
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

        "spec_get" | "state_spec_get" | "slice_get" | "state_slice_get" => {
            let id = args.get("id").and_then(|v| v.as_str());
            if let Some(id) = id {
                let spec = get_spec(pool, project_dir, id).await?;
                Ok(json!(spec))
            } else {
                let specs = list_specs(pool, project_dir).await?;
                Ok(json!(specs))
            }
        }

        "spec_create" | "state_spec_create" | "slice_create" | "state_slice_create" => {
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

            let spec = create_spec(pool, project_dir, id, title, priority, &depends_on).await?;

            // Optionally update agents
            if let Some(agents_arr) = args.get("agents").and_then(|v| v.as_array()) {
                let agents: Vec<String> = agents_arr
                    .iter()
                    .filter_map(|v| v.as_str().map(String::from))
                    .collect();
                update_spec_agents(pool, project_dir, id, &agents).await?;
            }

            Ok(json!(spec))
        }

        "spec_update" | "state_spec_update" | "slice_update" | "state_slice_update" => {
            let id = args
                .get("id")
                .and_then(|v| v.as_str())
                .ok_or_else(|| anyhow::anyhow!("Missing field: id"))?;

            let updated_by = args
                .get("updated_by")
                .and_then(|v| v.as_str())
                .unwrap_or("agent");

            if let Some(status) = args.get("status").and_then(|v| v.as_str()) {
                update_spec_status(pool, project_dir, id, status, updated_by).await?;
            }

            if let Some(ac_total) = args.get("ac_total").and_then(|v| v.as_i64()) {
                let ac_passed = args.get("ac_passed").and_then(|v| v.as_i64()).unwrap_or(0);
                update_spec_ac(pool, project_dir, id, ac_total, ac_passed).await?;
            } else if let Some(ac_passed) = args.get("ac_passed").and_then(|v| v.as_i64()) {
                let spec = get_spec(pool, project_dir, id)
                    .await?
                    .ok_or_else(|| anyhow::anyhow!("Spec not found: {}", id))?;
                update_spec_ac(pool, project_dir, id, spec.ac_total, ac_passed).await?;
            }

            if let Some(agents_arr) = args.get("agents").and_then(|v| v.as_array()) {
                let agents: Vec<String> = agents_arr
                    .iter()
                    .filter_map(|v| v.as_str().map(String::from))
                    .collect();
                update_spec_agents(pool, project_dir, id, &agents).await?;
            }

            let spec = get_spec(pool, project_dir, id)
                .await?
                .ok_or_else(|| anyhow::anyhow!("Spec not found: {}", id))?;
            Ok(json!(spec))
        }

        "task_get" | "state_task_get" => {
            let id = args.get("id").and_then(|v| v.as_str());
            let spec = args.get("spec").and_then(|v| v.as_str());

            if let Some(id) = id {
                let task = get_task(pool, project_dir, id).await?;
                Ok(json!(task))
            } else {
                let tasks = list_tasks(pool, project_dir, spec).await?;
                Ok(json!(tasks))
            }
        }

        "task_create" | "state_task_create" => {
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
            let depends_on: Vec<String> = args
                .get("depends_on")
                .and_then(|v| v.as_array())
                .map(|a| {
                    a.iter()
                        .filter_map(|v| v.as_str().map(String::from))
                        .collect()
                })
                .unwrap_or_default();
            let conflicts_with: Vec<String> = args
                .get("conflicts_with")
                .and_then(|v| v.as_array())
                .map(|a| {
                    a.iter()
                        .filter_map(|v| v.as_str().map(String::from))
                        .collect()
                })
                .unwrap_or_default();
            let lock_set: Vec<String> = args
                .get("lock_set")
                .and_then(|v| v.as_array())
                .map(|a| {
                    a.iter()
                        .filter_map(|v| v.as_str().map(String::from))
                        .collect()
                })
                .unwrap_or_default();
            let lock_requirements: Vec<TaskLockRequirement> = args
                .get("lock_requirements")
                .and_then(|v| v.as_array())
                .map(|a| {
                    a.iter()
                        .filter_map(|item| {
                            Some(TaskLockRequirement {
                                lock_type: item.get("lock_type")?.as_str()?.to_string(),
                                resource: item.get("resource")?.as_str()?.to_string(),
                            })
                        })
                        .collect()
                })
                .unwrap_or_default();
            let priority = args.get("priority").and_then(|v| v.as_i64()).unwrap_or(100);
            let risk_level = args
                .get("risk_level")
                .and_then(|v| v.as_str())
                .unwrap_or("medium");
            let execution_bucket = args
                .get("execution_bucket")
                .and_then(|v| v.as_str())
                .unwrap_or("coordinated_parallel");
            let estimate_points = args
                .get("estimate_points")
                .and_then(|v| v.as_i64())
                .unwrap_or(3);
            let unblock_value = args
                .get("unblock_value")
                .and_then(|v| v.as_i64())
                .unwrap_or(0);
            let plan_version = args.get("plan_version").and_then(|v| v.as_str());
            let output_artifact = args.get("output_artifact").and_then(|v| v.as_str());

            let task = create_task(
                pool,
                project_dir,
                id,
                spec,
                title,
                agent,
                &inputs,
                &depends_on,
                &conflicts_with,
                &lock_set,
                &lock_requirements,
                priority,
                risk_level,
                execution_bucket,
                estimate_points,
                unblock_value,
                plan_version,
                output_artifact,
            )
            .await?;
            Ok(json!(task))
        }

        "task_update" | "state_task_update" => {
            let id = args
                .get("id")
                .and_then(|v| v.as_str())
                .ok_or_else(|| anyhow::anyhow!("Missing field: id"))?;

            if let Some(status) = args.get("status").and_then(|v| v.as_str()) {
                update_task_status(pool, project_dir, id, status).await?;
            }

            if let Some(artifact) = args.get("output_artifact").and_then(|v| v.as_str()) {
                update_task_output_artifact(pool, project_dir, id, artifact).await?;
            }

            if args.get("depends_on").is_some()
                || args.get("conflicts_with").is_some()
                || args.get("lock_set").is_some()
                || args.get("lock_requirements").is_some()
                || args.get("priority").is_some()
                || args.get("risk_level").is_some()
                || args.get("execution_bucket").is_some()
                || args.get("plan_version").is_some()
            {
                let depends_on: Option<Vec<String>> =
                    args.get("depends_on").and_then(|v| v.as_array()).map(|a| {
                        a.iter()
                            .filter_map(|v| v.as_str().map(String::from))
                            .collect()
                    });
                let conflicts_with: Option<Vec<String>> = args
                    .get("conflicts_with")
                    .and_then(|v| v.as_array())
                    .map(|a| {
                        a.iter()
                            .filter_map(|v| v.as_str().map(String::from))
                            .collect()
                    });
                let lock_set: Option<Vec<String>> =
                    args.get("lock_set").and_then(|v| v.as_array()).map(|a| {
                        a.iter()
                            .filter_map(|v| v.as_str().map(String::from))
                            .collect()
                    });
                let lock_requirements: Option<Vec<TaskLockRequirement>> = args
                    .get("lock_requirements")
                    .and_then(|v| v.as_array())
                    .map(|a| {
                        a.iter()
                            .filter_map(|item| {
                                Some(TaskLockRequirement {
                                    lock_type: item.get("lock_type")?.as_str()?.to_string(),
                                    resource: item.get("resource")?.as_str()?.to_string(),
                                })
                            })
                            .collect()
                    });
                let priority = args.get("priority").and_then(|v| v.as_i64());
                let risk_level = args.get("risk_level").and_then(|v| v.as_str());
                let execution_bucket = args.get("execution_bucket").and_then(|v| v.as_str());
                let estimate_points = args.get("estimate_points").and_then(|v| v.as_i64());
                let unblock_value = args.get("unblock_value").and_then(|v| v.as_i64());
                let plan_version = if args.get("plan_version").is_some() {
                    Some(args.get("plan_version").and_then(|v| v.as_str()))
                } else {
                    None
                };
                update_task_metadata(
                    pool,
                    project_dir,
                    id,
                    depends_on.as_deref(),
                    conflicts_with.as_deref(),
                    lock_set.as_deref(),
                    lock_requirements.as_deref(),
                    priority,
                    risk_level,
                    execution_bucket,
                    estimate_points,
                    unblock_value,
                    plan_version,
                )
                .await?;
            }

            let task = get_task(pool, project_dir, id)
                .await?
                .ok_or_else(|| anyhow::anyhow!("Task not found: {}", id))?;
            Ok(json!(task))
        }

        "plan_version_get" | "state_plan_version_get" => {
            let id = args.get("id").and_then(|v| v.as_str());
            let spec = args.get("spec").and_then(|v| v.as_str());
            if let Some(id) = id {
                Ok(json!(get_plan_version(pool, project_dir, id).await?))
            } else {
                Ok(json!(list_plan_versions(pool, project_dir, spec).await?))
            }
        }

        "plan_version_create" | "state_plan_version_create" => {
            let id = args
                .get("id")
                .and_then(|v| v.as_str())
                .ok_or_else(|| anyhow::anyhow!("Missing field: id"))?;
            let spec = args
                .get("spec")
                .and_then(|v| v.as_str())
                .ok_or_else(|| anyhow::anyhow!("Missing field: spec"))?;
            let version = args
                .get("version")
                .and_then(|v| v.as_i64())
                .ok_or_else(|| anyhow::anyhow!("Missing field: version"))?;
            let reason = args.get("reason").and_then(|v| v.as_str());
            let plan_json = args
                .get("plan_json")
                .map(|v| v.to_string())
                .ok_or_else(|| anyhow::anyhow!("Missing field: plan_json"))?;
            let supersede = args
                .get("supersede_existing")
                .and_then(|v| v.as_bool())
                .unwrap_or(false);
            if supersede {
                supersede_plan_versions(pool, project_dir, spec).await?;
            }
            Ok(json!(
                create_plan_version(pool, project_dir, id, spec, version, reason, &plan_json).await?
            ))
        }

        "task_lease_get" | "state_task_lease_get" => {
            let task = args.get("task").and_then(|v| v.as_str());
            let status = args.get("status").and_then(|v| v.as_str());
            if let Some(task) = task {
                Ok(json!(get_task_lease(pool, project_dir, task).await?))
            } else {
                Ok(json!(list_task_leases(pool, project_dir, status).await?))
            }
        }

        "task_lease_claim" | "state_task_lease_claim" => {
            let task = args
                .get("task")
                .and_then(|v| v.as_str())
                .ok_or_else(|| anyhow::anyhow!("Missing field: task"))?;
            let agent = args
                .get("agent")
                .and_then(|v| v.as_str())
                .ok_or_else(|| anyhow::anyhow!("Missing field: agent"))?;
            let ttl = args
                .get("lease_ttl_seconds")
                .and_then(|v| v.as_i64())
                .unwrap_or(1800);
            Ok(json!(claim_task_lease(pool, project_dir, task, agent, ttl).await?))
        }

        "task_lease_heartbeat" | "state_task_lease_heartbeat" => {
            let task = args
                .get("task")
                .and_then(|v| v.as_str())
                .ok_or_else(|| anyhow::anyhow!("Missing field: task"))?;
            let ttl = args
                .get("lease_ttl_seconds")
                .and_then(|v| v.as_i64())
                .unwrap_or(1800);
            let progress_status = args.get("progress_status").and_then(|v| v.as_str());
            Ok(json!(
                heartbeat_task_lease(pool, project_dir, task, ttl, progress_status).await?
            ))
        }

        "task_lease_release" | "state_task_lease_release" => {
            let task = args
                .get("task")
                .and_then(|v| v.as_str())
                .ok_or_else(|| anyhow::anyhow!("Missing field: task"))?;
            let final_status = args.get("final_status").and_then(|v| v.as_str());
            let released = release_task_lease(pool, project_dir, task, final_status).await?;
            let _ = release_task_locks(pool, project_dir, task).await?;
            Ok(json!(released))
        }

        "task_lease_expire" | "state_task_lease_expire" => {
            Ok(json!(expire_stale_task_leases(pool, project_dir).await?))
        }

        "task_lock_query" | "state_task_lock_query" => {
            let spec = args.get("spec").and_then(|v| v.as_str());
            let task = args.get("task").and_then(|v| v.as_str());
            let active_only = args
                .get("active_only")
                .and_then(|v| v.as_bool())
                .unwrap_or(true);
            Ok(json!(
                query_task_locks(pool, project_dir, spec, task, active_only).await?
            ))
        }

        "task_lock_acquire" | "state_task_lock_acquire" => {
            let task = args
                .get("task")
                .and_then(|v| v.as_str())
                .ok_or_else(|| anyhow::anyhow!("Missing field: task"))?;
            let spec = args
                .get("spec")
                .and_then(|v| v.as_str())
                .ok_or_else(|| anyhow::anyhow!("Missing field: spec"))?;
            let locks_value = args
                .get("locks")
                .and_then(|v| v.as_array())
                .ok_or_else(|| anyhow::anyhow!("Missing field: locks"))?;
            let locks: Vec<(String, String)> = locks_value
                .iter()
                .filter_map(|item| {
                    Some((
                        item.get("lock_type")?.as_str()?.to_string(),
                        item.get("resource")?.as_str()?.to_string(),
                    ))
                })
                .collect();
            Ok(json!(acquire_task_locks(pool, project_dir, task, spec, &locks).await?))
        }

        "task_lock_release" | "state_task_lock_release" => {
            let task = args
                .get("task")
                .and_then(|v| v.as_str())
                .ok_or_else(|| anyhow::anyhow!("Missing field: task"))?;
            Ok(json!(release_task_locks(pool, project_dir, task).await?))
        }

        "replan_request_get" | "state_replan_request_get" => {
            let id = args.get("id").and_then(|v| v.as_str());
            let spec = args.get("spec").and_then(|v| v.as_str());
            let status = args.get("status").and_then(|v| v.as_str());
            if let Some(id) = id {
                Ok(json!(get_replan_request(pool, project_dir, id).await?))
            } else {
                Ok(json!(list_replan_requests(pool, project_dir, spec, status).await?))
            }
        }

        "replan_request_create" | "state_replan_request_create" => {
            let id = args
                .get("id")
                .and_then(|v| v.as_str())
                .ok_or_else(|| anyhow::anyhow!("Missing field: id"))?;
            let spec = args
                .get("spec")
                .and_then(|v| v.as_str())
                .ok_or_else(|| anyhow::anyhow!("Missing field: spec"))?;
            let agent = args
                .get("agent")
                .and_then(|v| v.as_str())
                .ok_or_else(|| anyhow::anyhow!("Missing field: agent"))?;
            let reason = args
                .get("reason")
                .and_then(|v| v.as_str())
                .ok_or_else(|| anyhow::anyhow!("Missing field: reason"))?;
            let task = args.get("task").and_then(|v| v.as_str());
            let impact: Vec<String> = args
                .get("impact")
                .and_then(|v| v.as_array())
                .map(|a| {
                    a.iter()
                        .filter_map(|v| v.as_str().map(String::from))
                        .collect()
                })
                .unwrap_or_default();
            let proposed_action = args.get("proposed_action").and_then(|v| v.as_str());
            Ok(json!(
                create_replan_request(
                    pool,
                    project_dir,
                    id,
                    spec,
                    task,
                    agent,
                    reason,
                    &impact,
                    proposed_action
                )
                .await?
            ))
        }

        "replan_request_update" | "state_replan_request_update" => {
            let id = args
                .get("id")
                .and_then(|v| v.as_str())
                .ok_or_else(|| anyhow::anyhow!("Missing field: id"))?;
            let status = args
                .get("status")
                .and_then(|v| v.as_str())
                .ok_or_else(|| anyhow::anyhow!("Missing field: status"))?;
            Ok(json!(update_replan_request(pool, project_dir, id, status).await?))
        }

        "scheduler_next" | "state_scheduler_next" => {
            let spec = args
                .get("spec")
                .and_then(|v| v.as_str())
                .ok_or_else(|| anyhow::anyhow!("Missing field: spec"))?;
            let agent = args
                .get("agent")
                .and_then(|v| v.as_str())
                .ok_or_else(|| anyhow::anyhow!("Missing field: agent"))?;
            Ok(json!(scheduler_next(pool, project_dir, spec, agent).await?))
        }

        "incident_get" | "state_incident_get" => {
            let id = args.get("id").and_then(|v| v.as_str());
            let spec = args.get("spec").and_then(|v| v.as_str());
            let status = args.get("status").and_then(|v| v.as_str());
            if let Some(id) = id {
                Ok(json!(get_incident(pool, project_dir, id).await?))
            } else {
                Ok(json!(list_incidents(pool, project_dir, spec, status).await?))
            }
        }

        "incident_create" | "state_incident_create" => {
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
            let severity = args
                .get("severity")
                .and_then(|v| v.as_str())
                .ok_or_else(|| anyhow::anyhow!("Missing field: severity"))?;
            let source = args
                .get("source")
                .and_then(|v| v.as_str())
                .ok_or_else(|| anyhow::anyhow!("Missing field: source"))?;
            let task = args.get("task").and_then(|v| v.as_str());
            let blocking = args
                .get("blocking")
                .and_then(|v| v.as_bool())
                .unwrap_or(false);
            let repro_steps = args.get("repro_steps").and_then(|v| v.as_str());
            Ok(json!(
                create_incident(
                    pool,
                    project_dir,
                    id,
                    spec,
                    task,
                    title,
                    severity,
                    source,
                    blocking,
                    repro_steps
                )
                .await?
            ))
        }

        "incident_update" | "state_incident_update" => {
            let id = args
                .get("id")
                .and_then(|v| v.as_str())
                .ok_or_else(|| anyhow::anyhow!("Missing field: id"))?;
            let status = args.get("status").and_then(|v| v.as_str());
            let blocking = args.get("blocking").and_then(|v| v.as_bool());
            let root_cause = args.get("root_cause").and_then(|v| v.as_str());
            let fix_strategy = args.get("fix_strategy").and_then(|v| v.as_str());
            Ok(json!(
                update_incident(pool, project_dir, id, status, blocking, root_cause, fix_strategy).await?
            ))
        }

        "context_gap_get" | "state_context_gap_get" => {
            let id = args.get("id").and_then(|v| v.as_str());
            let spec = args.get("spec").and_then(|v| v.as_str());
            let status = args.get("status").and_then(|v| v.as_str());
            if let Some(id) = id {
                Ok(json!(get_context_gap(pool, project_dir, id).await?))
            } else {
                Ok(json!(list_context_gaps(pool, project_dir, spec, status).await?))
            }
        }

        "context_gap_create" | "state_context_gap_create" => {
            let id = args
                .get("id")
                .and_then(|v| v.as_str())
                .ok_or_else(|| anyhow::anyhow!("Missing field: id"))?;
            let spec = args
                .get("spec")
                .and_then(|v| v.as_str())
                .ok_or_else(|| anyhow::anyhow!("Missing field: spec"))?;
            let kind = args
                .get("kind")
                .and_then(|v| v.as_str())
                .ok_or_else(|| anyhow::anyhow!("Missing field: kind"))?;
            let criticality = args
                .get("criticality")
                .and_then(|v| v.as_str())
                .ok_or_else(|| anyhow::anyhow!("Missing field: criticality"))?;
            let question = args
                .get("question")
                .and_then(|v| v.as_str())
                .ok_or_else(|| anyhow::anyhow!("Missing field: question"))?;
            let task = args.get("task").and_then(|v| v.as_str());
            let blocking = args
                .get("blocking")
                .and_then(|v| v.as_bool())
                .unwrap_or(false);
            let assumption = args.get("assumption").and_then(|v| v.as_str());
            Ok(json!(
                create_context_gap(
                    pool,
                    project_dir,
                    id,
                    spec,
                    task,
                    kind,
                    criticality,
                    blocking,
                    question,
                    assumption
                )
                .await?
            ))
        }

        "context_gap_update" | "state_context_gap_update" => {
            let id = args
                .get("id")
                .and_then(|v| v.as_str())
                .ok_or_else(|| anyhow::anyhow!("Missing field: id"))?;
            let status = args.get("status").and_then(|v| v.as_str());
            let blocking = args.get("blocking").and_then(|v| v.as_bool());
            let assumption = args.get("assumption").and_then(|v| v.as_str());
            let resolution = args.get("resolution").and_then(|v| v.as_str());
            Ok(json!(
                update_context_gap(pool, project_dir, id, status, blocking, assumption, resolution).await?
            ))
        }

        "verification_run_get" | "state_verification_run_get" => {
            let id = args.get("id").and_then(|v| v.as_str());
            let spec = args.get("spec").and_then(|v| v.as_str());
            let task = args.get("task").and_then(|v| v.as_str());
            let status = args.get("status").and_then(|v| v.as_str());
            if let Some(id) = id {
                Ok(json!(get_verification_run(pool, project_dir, id).await?))
            } else {
                Ok(json!(
                    list_verification_runs(pool, project_dir, spec, task, status).await?
                ))
            }
        }

        "verification_run_create" | "state_verification_run_create" => {
            let id = args
                .get("id")
                .and_then(|v| v.as_str())
                .ok_or_else(|| anyhow::anyhow!("Missing field: id"))?;
            let spec = args
                .get("spec")
                .and_then(|v| v.as_str())
                .ok_or_else(|| anyhow::anyhow!("Missing field: spec"))?;
            let kind = args
                .get("kind")
                .and_then(|v| v.as_str())
                .ok_or_else(|| anyhow::anyhow!("Missing field: kind"))?;
            let status = args
                .get("status")
                .and_then(|v| v.as_str())
                .ok_or_else(|| anyhow::anyhow!("Missing field: status"))?;
            let summary = args
                .get("summary")
                .and_then(|v| v.as_str())
                .ok_or_else(|| anyhow::anyhow!("Missing field: summary"))?;
            let task = args.get("task").and_then(|v| v.as_str());
            let slice = args.get("slice").and_then(|v| v.as_str());
            let command = args.get("command").and_then(|v| v.as_str());
            let evidence = args.get("evidence").and_then(|v| v.as_str());
            Ok(json!(
                create_verification_run(
                    pool, project_dir, id, spec, task, slice, kind, status, command, summary, evidence
                )
                .await?
            ))
        }

        "interrupt_get" | "state_interrupt_get" => {
            let id = args.get("id").and_then(|v| v.as_str());
            let spec = args.get("spec").and_then(|v| v.as_str());
            let status = args.get("status").and_then(|v| v.as_str());
            if let Some(id) = id {
                Ok(json!(get_interrupt(pool, project_dir, id).await?))
            } else {
                Ok(json!(list_interrupts(pool, project_dir, spec, status).await?))
            }
        }

        "interrupt_create" | "state_interrupt_create" => {
            let id = args
                .get("id")
                .and_then(|v| v.as_str())
                .ok_or_else(|| anyhow::anyhow!("Missing field: id"))?;
            let spec = args
                .get("spec")
                .and_then(|v| v.as_str())
                .ok_or_else(|| anyhow::anyhow!("Missing field: spec"))?;
            let reason_type = args
                .get("reason_type")
                .and_then(|v| v.as_str())
                .ok_or_else(|| anyhow::anyhow!("Missing field: reason_type"))?;
            let preempted_tasks: Vec<String> = args
                .get("preempted_tasks")
                .and_then(|v| v.as_array())
                .map(|a| {
                    a.iter()
                        .filter_map(|v| v.as_str().map(String::from))
                        .collect()
                })
                .unwrap_or_default();
            let resume_hint = args.get("resume_hint").and_then(|v| v.as_str());
            Ok(json!(
                create_interrupt(pool, project_dir, id, spec, reason_type, &preempted_tasks, resume_hint)
                    .await?
            ))
        }

        "interrupt_update" | "state_interrupt_update" => {
            let id = args
                .get("id")
                .and_then(|v| v.as_str())
                .ok_or_else(|| anyhow::anyhow!("Missing field: id"))?;
            let status = args.get("status").and_then(|v| v.as_str());
            let resume_hint = args.get("resume_hint").and_then(|v| v.as_str());
            Ok(json!(
                update_interrupt(pool, project_dir, id, status, resume_hint).await?
            ))
        }

        "handoff_snapshot_get" | "state_handoff_snapshot_get" => {
            let id = args.get("id").and_then(|v| v.as_str());
            let spec = args.get("spec").and_then(|v| v.as_str());
            if let Some(id) = id {
                Ok(json!(get_handoff_snapshot(pool, project_dir, id).await?))
            } else {
                Ok(json!(list_handoff_snapshots(pool, project_dir, spec).await?))
            }
        }

        "handoff_snapshot_create" | "state_handoff_snapshot_create" => {
            let id = args
                .get("id")
                .and_then(|v| v.as_str())
                .ok_or_else(|| anyhow::anyhow!("Missing field: id"))?;
            let spec = args
                .get("spec")
                .and_then(|v| v.as_str())
                .ok_or_else(|| anyhow::anyhow!("Missing field: spec"))?;
            let interrupt = args.get("interrupt").and_then(|v| v.as_str());
            let last_wave = args.get("last_wave").and_then(|v| v.as_i64());
            let last_task = args.get("last_task").and_then(|v| v.as_str());
            let files_touched: Vec<String> = args
                .get("files_touched")
                .and_then(|v| v.as_array())
                .map(|a| {
                    a.iter()
                        .filter_map(|v| v.as_str().map(String::from))
                        .collect()
                })
                .unwrap_or_default();
            let decisions: Vec<String> = args
                .get("decisions")
                .and_then(|v| v.as_array())
                .map(|a| {
                    a.iter()
                        .filter_map(|v| v.as_str().map(String::from))
                        .collect()
                })
                .unwrap_or_default();
            let open_risks: Vec<String> = args
                .get("open_risks")
                .and_then(|v| v.as_array())
                .map(|a| {
                    a.iter()
                        .filter_map(|v| v.as_str().map(String::from))
                        .collect()
                })
                .unwrap_or_default();
            let next_steps: Vec<String> = args
                .get("next_steps")
                .and_then(|v| v.as_array())
                .map(|a| {
                    a.iter()
                        .filter_map(|v| v.as_str().map(String::from))
                        .collect()
                })
                .unwrap_or_default();
            Ok(json!(
                create_handoff_snapshot(
                    pool,
                    project_dir,
                    id,
                    spec,
                    interrupt,
                    last_wave,
                    last_task,
                    &files_touched,
                    &decisions,
                    &open_risks,
                    &next_steps
                )
                .await?
            ))
        }

        "event_emit" | "state_event_emit" => {
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

            emit_event(pool, project_dir, event_type, spec, agent, &payload).await?;
            Ok(json!({"ok": true}))
        }

        "event_query" | "state_event_query" => {
            let type_filter = args.get("type").and_then(|v| v.as_str());
            let spec_filter = args.get("spec").and_then(|v| v.as_str());
            let agent_filter = args.get("agent").and_then(|v| v.as_str());
            let limit = args.get("limit").and_then(|v| v.as_i64());
            let since = args.get("since").and_then(|v| v.as_str());
            let until = args.get("until").and_then(|v| v.as_str());

            let events = query_events(
                pool,
                project_dir,
                type_filter,
                spec_filter,
                agent_filter,
                limit,
                since,
                until,
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

            memory_set(pool, project_dir, agent, key, &value, spec, mem_type, ttl_seconds).await?;
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
                let memory = memory_get_full(pool, project_dir, agent, key, spec).await?;
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
                let entries = memory_get_all(pool, project_dir, agent, spec).await?;
                let entries_obj: Vec<Value> = entries
                    .into_iter()
                    .map(|(k, v)| json!({"key": k, "value": parse_memory_value(&v)}))
                    .collect();
                Ok(json!({"entries": entries_obj}))
            }
        }

        "artifact_register" => {
            let id = args
                .get("id")
                .and_then(|v| v.as_str())
                .ok_or_else(|| anyhow::anyhow!("Missing field: id"))?;
            let spec = args
                .get("spec")
                .and_then(|v| v.as_str())
                .ok_or_else(|| anyhow::anyhow!("Missing field: spec"))?;
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
                project_dir,
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

        "artifact_query" => {
            let spec = args.get("spec").and_then(|v| v.as_str());
            let task = args.get("task").and_then(|v| v.as_str());
            let agent = args.get("agent").and_then(|v| v.as_str());
            let artifact_type = args.get("type").and_then(|v| v.as_str());

            let artifacts = query_artifacts(pool, project_dir, spec, task, agent, artifact_type).await?;
            Ok(json!(artifacts))
        }

        "constitution_get" | "state_constitution_get" | "prd_get" | "state_prd_get" => {
            // Read docs/PRD.md (source of truth is the file, not the DB)
            let prd_path = std::path::Path::new(project_dir)
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

            let results = memory_search(pool, project_dir, agent, query_str, spec, mem_type, limit).await?;
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

            let deleted = memory_delete(pool, project_dir, agent, key, spec).await?;
            Ok(json!({"deleted": deleted}))
        }

        "memory_context" => {
            let agent = args
                .get("agent")
                .and_then(|v| v.as_str())
                .ok_or_else(|| anyhow::anyhow!("Missing field: agent"))?;
            let spec = args.get("spec").and_then(|v| v.as_str());
            let limit = args.get("limit").and_then(|v| v.as_i64());

            let entries = memory_context(pool, project_dir, agent, spec, limit).await?;
            Ok(json!(entries))
        }

        "memory_stats" => {
            let agent = args
                .get("agent")
                .and_then(|v| v.as_str())
                .ok_or_else(|| anyhow::anyhow!("Missing field: agent"))?;
            let spec = args.get("spec").and_then(|v| v.as_str());

            let stats = memory_stats(pool, project_dir, agent, spec).await?;
            Ok(stats)
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
            "description": "Alias for spec_get. Get a specific slice/spec by ID, or list all.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "id": {"type": "string", "description": "Slice/Spec ID (optional; omit to list all)"}
                }
            }
        },
        {
            "name": "spec_get",
            "description": "Legacy alias for state_slice_get.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "id": {"type": "string", "description": "Spec ID (optional; omit to list all)"}
                }
            }
        },
        {
            "name": "state_slice_create",
            "description": "Alias for spec_create. Create a new slice/spec.",
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
            "name": "spec_create",
            "description": "Legacy alias for state_slice_create.",
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
            "description": "Alias for spec_update. Update slice/spec status, AC counts, or agents.",
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
            "name": "spec_update",
            "description": "Legacy alias for state_slice_update.",
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
            "description": "Alias for task_get. Get a task by ID, or list tasks (optionally filtered by spec).",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "id": {"type": "string"},
                    "spec": {"type": "string"}
                }
            }
        },
        {
            "name": "task_get",
            "description": "Legacy alias for state_task_get.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "id": {"type": "string"},
                    "spec": {"type": "string"}
                }
            }
        },
        {
            "name": "state_task_create",
            "description": "Alias for task_create. Create a new task within a spec.",
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
            "name": "task_create",
            "description": "Legacy alias for state_task_create.",
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
            "description": "Alias for task_update. Update task status or output artifact.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "id": {"type": "string"},
                    "status": {"type": "string"},
                    "output_artifact": {"type": "string"},
                    "depends_on": {"type": "array", "items": {"type": "string"}},
                    "conflicts_with": {"type": "array", "items": {"type": "string"}},
                    "lock_set": {"type": "array", "items": {"type": "string"}},
                    "lock_requirements": {"type": "array", "items": {"type": "object", "properties": {"lock_type": {"type": "string"}, "resource": {"type": "string"}}, "required": ["lock_type", "resource"]}},
                    "priority": {"type": "number"},
                    "risk_level": {"type": "string", "enum": ["low", "medium", "high", "critical"]},
                    "execution_bucket": {"type": "string", "enum": ["safe_parallel", "coordinated_parallel", "serialized_only"]},
                    "estimate_points": {"type": "number"},
                    "unblock_value": {"type": "number"},
                    "plan_version": {"type": ["string", "null"]}
                },
                "required": ["id"]
            }
        },
        {
            "name": "task_update",
            "description": "Legacy alias for state_task_update.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "id": {"type": "string"},
                    "status": {"type": "string"},
                    "output_artifact": {"type": "string"},
                    "depends_on": {"type": "array", "items": {"type": "string"}},
                    "conflicts_with": {"type": "array", "items": {"type": "string"}},
                    "lock_set": {"type": "array", "items": {"type": "string"}},
                    "lock_requirements": {"type": "array", "items": {"type": "object", "properties": {"lock_type": {"type": "string"}, "resource": {"type": "string"}}, "required": ["lock_type", "resource"]}},
                    "priority": {"type": "number"},
                    "risk_level": {"type": "string", "enum": ["low", "medium", "high", "critical"]},
                    "execution_bucket": {"type": "string", "enum": ["safe_parallel", "coordinated_parallel", "serialized_only"]},
                    "estimate_points": {"type": "number"},
                    "unblock_value": {"type": "number"},
                    "plan_version": {"type": ["string", "null"]}
                },
                "required": ["id"]
            }
        },
        {
            "name": "state_event_emit",
            "description": "Alias for event_emit. Emit a domain event to the event log.",
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
            "name": "event_emit",
            "description": "Legacy alias for state_event_emit.",
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
            "description": "Alias for event_query. Query the event log with optional filters.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "type": {"type": "string"},
                    "spec": {"type": "string"},
                    "agent": {"type": "string"},
                    "limit": {"type": "number"},
                    "since": {"type": "string"}
                }
            }
        },
        {
            "name": "event_query",
            "description": "Legacy alias for state_event_query.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "type": {"type": "string"},
                    "spec": {"type": "string"},
                    "agent": {"type": "string"},
                    "limit": {"type": "number"},
                    "since": {"type": "string"}
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
            "name": "artifact_register",
            "description": "Register an output artifact produced by an agent.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "id": {"type": "string"},
                    "spec": {"type": "string"},
                    "agent": {"type": "string"},
                    "type": {"type": "string"},
                    "task": {"type": "string"},
                    "path": {"type": "string"},
                    "description": {"type": "string"}
                },
                "required": ["id", "spec", "agent", "type"]
            }
        },
        {
            "name": "artifact_query",
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
            "name": "state_plan_version_get",
            "description": "Get a plan version by ID, or list plan versions for a spec.",
            "inputSchema": {"type": "object", "properties": {"id": {"type": "string"}, "spec": {"type": "string"}}}
        },
        {
            "name": "state_plan_version_create",
            "description": "Create a new plan version for a slice/spec.",
            "inputSchema": {"type": "object", "properties": {"id": {"type": "string"}, "spec": {"type": "string"}, "version": {"type": "number"}, "reason": {"type": "string"}, "plan_json": {}, "supersede_existing": {"type": "boolean"}}, "required": ["id", "spec", "version", "plan_json"]}
        },
        {
            "name": "state_task_lease_get",
            "description": "Get a task lease by task ID, or list leases by status.",
            "inputSchema": {"type": "object", "properties": {"task": {"type": "string"}, "status": {"type": "string"}}}
        },
        {
            "name": "state_task_lease_claim",
            "description": "Claim a scheduler lease for a ready task.",
            "inputSchema": {"type": "object", "properties": {"task": {"type": "string"}, "agent": {"type": "string"}, "lease_ttl_seconds": {"type": "number"}}, "required": ["task", "agent"]}
        },
        {
            "name": "state_task_lease_heartbeat",
            "description": "Refresh an active task lease heartbeat.",
            "inputSchema": {"type": "object", "properties": {"task": {"type": "string"}, "lease_ttl_seconds": {"type": "number"}, "progress_status": {"type": "string"}}, "required": ["task"]}
        },
        {
            "name": "state_task_lease_release",
            "description": "Release a task lease and optionally move the task to a final status.",
            "inputSchema": {"type": "object", "properties": {"task": {"type": "string"}, "final_status": {"type": "string"}}, "required": ["task"]}
        },
        {
            "name": "state_task_lease_expire",
            "description": "Expire stale active task leases and return them to ready state.",
            "inputSchema": {"type": "object", "properties": {}}
        },
        {
            "name": "state_task_lock_query",
            "description": "Query module/semantic/file locks for tasks.",
            "inputSchema": {"type": "object", "properties": {"spec": {"type": "string"}, "task": {"type": "string"}, "active_only": {"type": "boolean"}}}
        },
        {
            "name": "state_task_lock_acquire",
            "description": "Acquire module/semantic/file locks for a task.",
            "inputSchema": {"type": "object", "properties": {"task": {"type": "string"}, "spec": {"type": "string"}, "locks": {"type": "array", "items": {"type": "object", "properties": {"lock_type": {"type": "string", "enum": ["module", "semantic", "file"]}, "resource": {"type": "string"}}, "required": ["lock_type", "resource"]}}}, "required": ["task", "spec", "locks"]}
        },
        {
            "name": "state_task_lock_release",
            "description": "Release all active locks for a task.",
            "inputSchema": {"type": "object", "properties": {"task": {"type": "string"}}, "required": ["task"]}
        },
        {
            "name": "state_replan_request_get",
            "description": "Get a replan request by ID, or list replan requests filtered by spec/status.",
            "inputSchema": {"type": "object", "properties": {"id": {"type": "string"}, "spec": {"type": "string"}, "status": {"type": "string"}}}
        },
        {
            "name": "state_replan_request_create",
            "description": "Create a replan request when execution drifts from the approved plan.",
            "inputSchema": {"type": "object", "properties": {"id": {"type": "string"}, "spec": {"type": "string"}, "task": {"type": "string"}, "agent": {"type": "string"}, "reason": {"type": "string"}, "impact": {"type": "array", "items": {"type": "string"}}, "proposed_action": {"type": "string"}}, "required": ["id", "spec", "agent", "reason"]}
        },
        {
            "name": "state_replan_request_update",
            "description": "Update replan request status.",
            "inputSchema": {"type": "object", "properties": {"id": {"type": "string"}, "status": {"type": "string"}}, "required": ["id", "status"]}
        },
        {
            "name": "state_scheduler_next",
            "description": "Return the next schedulable ready task for an agent within a spec.",
            "inputSchema": {"type": "object", "properties": {"spec": {"type": "string"}, "agent": {"type": "string"}}, "required": ["spec", "agent"]}
        },
        {
            "name": "state_incident_get",
            "description": "Get an incident by ID, or list incidents filtered by spec/status.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "id": {"type": "string"},
                    "spec": {"type": "string"},
                    "status": {"type": "string"}
                }
            }
        },
        {
            "name": "state_incident_create",
            "description": "Create a new incident linked to a spec/task.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "id": {"type": "string"},
                    "spec": {"type": "string"},
                    "task": {"type": "string"},
                    "title": {"type": "string"},
                    "severity": {"type": "string", "enum": ["low", "medium", "high", "critical"]},
                    "source": {"type": "string", "enum": ["spec_defect", "implementation_defect", "verification_gap", "documentation_gap", "environment", "unknown"]},
                    "blocking": {"type": "boolean"},
                    "repro_steps": {"type": "string"}
                },
                "required": ["id", "spec", "title", "severity", "source"]
            }
        },
        {
            "name": "state_incident_update",
            "description": "Update incident status, blocking flag, or root-cause notes.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "id": {"type": "string"},
                    "status": {"type": "string"},
                    "blocking": {"type": "boolean"},
                    "root_cause": {"type": "string"},
                    "fix_strategy": {"type": "string"}
                },
                "required": ["id"]
            }
        },
        {
            "name": "state_context_gap_get",
            "description": "Get a context gap by ID, or list gaps filtered by spec/status.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "id": {"type": "string"},
                    "spec": {"type": "string"},
                    "status": {"type": "string"}
                }
            }
        },
        {
            "name": "state_context_gap_create",
            "description": "Create a new context gap for missing or contradictory information.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "id": {"type": "string"},
                    "spec": {"type": "string"},
                    "task": {"type": "string"},
                    "kind": {"type": "string", "enum": ["missing_doc", "outdated_doc", "contradictory_doc", "undocumented_behavior"]},
                    "criticality": {"type": "string", "enum": ["low", "medium", "high"]},
                    "blocking": {"type": "boolean"},
                    "question": {"type": "string"},
                    "assumption": {"type": "string"}
                },
                "required": ["id", "spec", "kind", "criticality", "question"]
            }
        },
        {
            "name": "state_context_gap_update",
            "description": "Update context-gap status, blocking flag, assumption, or resolution.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "id": {"type": "string"},
                    "status": {"type": "string"},
                    "blocking": {"type": "boolean"},
                    "assumption": {"type": "string"},
                    "resolution": {"type": "string"}
                },
                "required": ["id"]
            }
        },
        {
            "name": "state_verification_run_get",
            "description": "Get a verification run by ID, or list runs filtered by spec/task/status.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "id": {"type": "string"},
                    "spec": {"type": "string"},
                    "task": {"type": "string"},
                    "status": {"type": "string"}
                }
            }
        },
        {
            "name": "state_verification_run_create",
            "description": "Record a structured verification run and its evidence.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "id": {"type": "string"},
                    "spec": {"type": "string"},
                    "task": {"type": "string"},
                    "slice": {"type": "string"},
                    "kind": {"type": "string", "enum": ["static", "unit", "integration", "contract", "e2e", "smoke", "migration", "docs", "observability"]},
                    "status": {"type": "string", "enum": ["pass", "pass_with_risk", "fail", "flaky", "blocked"]},
                    "command": {"type": "string"},
                    "summary": {"type": "string"},
                    "evidence": {"type": "string"}
                },
                "required": ["id", "spec", "kind", "status", "summary"]
            }
        },
        {
            "name": "state_interrupt_get",
            "description": "Get an interrupt by ID, or list interrupts filtered by spec/status.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "id": {"type": "string"},
                    "spec": {"type": "string"},
                    "status": {"type": "string"}
                }
            }
        },
        {
            "name": "state_interrupt_create",
            "description": "Create an interrupt record for reprioritized or urgent work.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "id": {"type": "string"},
                    "spec": {"type": "string"},
                    "reason_type": {"type": "string", "enum": ["emergency", "customer_critical", "revenue", "incident", "strategy", "dependency"]},
                    "preempted_tasks": {"type": "array", "items": {"type": "string"}},
                    "resume_hint": {"type": "string"}
                },
                "required": ["id", "spec", "reason_type"]
            }
        },
        {
            "name": "state_interrupt_update",
            "description": "Update interrupt status or resume hint.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "id": {"type": "string"},
                    "status": {"type": "string"},
                    "resume_hint": {"type": "string"}
                },
                "required": ["id"]
            }
        },
        {
            "name": "state_handoff_snapshot_get",
            "description": "Get a handoff snapshot by ID, or list snapshots filtered by spec.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "id": {"type": "string"},
                    "spec": {"type": "string"}
                }
            }
        },
        {
            "name": "state_handoff_snapshot_create",
            "description": "Create a resumable handoff snapshot for interrupted work.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "id": {"type": "string"},
                    "spec": {"type": "string"},
                    "interrupt": {"type": "string"},
                    "last_wave": {"type": "number"},
                    "last_task": {"type": "string"},
                    "files_touched": {"type": "array", "items": {"type": "string"}},
                    "decisions": {"type": "array", "items": {"type": "string"}},
                    "open_risks": {"type": "array", "items": {"type": "string"}},
                    "next_steps": {"type": "array", "items": {"type": "string"}}
                },
                "required": ["id", "spec"]
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
            "name": "state_constitution_get",
            "description": "Legacy alias for state_prd_get.",
            "inputSchema": {
                "type": "object",
                "properties": {}
            }
        },
        {
            "name": "constitution_get",
            "description": "Legacy alias for state_prd_get.",
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
        }
    ])
}

#[allow(dead_code)]
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
