mod args;
mod artifacts;
mod evals;
mod lifecycle;
mod memory;
mod policy;
mod project;
mod readiness;
mod sessions;

use anyhow::Result;
use serde_json::{json, Value};
use sqlx::SqlitePool;

pub async fn dispatch_tool(pool: &SqlitePool, name: &str, args: Value) -> Result<Value> {
    match name {
        "state_snapshot" => project::handle_snapshot(pool, args).await,
        "state_project_context" => project::handle_project_context(pool, args).await,
        "state_slice_get" => lifecycle::handle_slice_get(pool, args).await,
        "state_slice_create" => lifecycle::handle_slice_create(pool, args).await,
        "state_slice_update" => lifecycle::handle_slice_update(pool, args).await,
        "state_task_get" => lifecycle::handle_task_get(pool, args).await,
        "state_task_create" => lifecycle::handle_task_create(pool, args).await,
        "state_task_update" => lifecycle::handle_task_update(pool, args).await,
        "state_event_emit" => lifecycle::handle_event_emit(pool, args).await,
        "state_event_query" => lifecycle::handle_event_query(pool, args).await,
        "memory_set" => memory::handle_set(pool, args).await,
        "memory_get" => memory::handle_get(pool, args).await,
        "state_artifact_register" => artifacts::handle_register(pool, args).await,
        "state_artifact_query" => artifacts::handle_query(pool, args).await,
        "state_eval_create"
        | "state_eval_list"
        | "state_eval_get"
        | "state_eval_compare"
        | "state_eval_latest_baseline" => evals::handle(pool, name, &args).await.unwrap(),
        "state_prd_get" => project::handle_prd_get(pool, args).await,
        "memory_search" => memory::handle_search(pool, args).await,
        "memory_delete" => memory::handle_delete(pool, args).await,
        "memory_context" => memory::handle_context(pool, args).await,
        "memory_stats" => memory::handle_stats(pool, args).await,
        "memory_list" => memory::handle_list(pool, args).await,
        "memory_find_related" => memory::handle_find_related(pool, args).await,
        "memory_gc" => memory::handle_gc(pool, args).await,
        "state_prd_set" => project::handle_prd_set(pool, args).await,
        "policy_config_set" => policy::handle_config_set(pool, args).await,
        "policy_config_get" => policy::handle_config_get(pool, args).await,
        "policy_evidence_add" => policy::handle_evidence_add(pool, args).await,
        "policy_evidence_list" => policy::handle_evidence_list(pool, args).await,
        "policy_approval_request" => policy::handle_approval_request(pool, args).await,
        "policy_approval_decide" => policy::handle_approval_decide(pool, args).await,
        "policy_approval_list" => policy::handle_approval_list(pool, args).await,
        "state_session_start" | "state_session_end" | "state_sessions_list" => {
            sessions::handle(pool, name, &args).await.unwrap()
        }
        "state_readiness_spec"
        | "state_readiness_operator"
        | "state_readiness_phase_transition"
        | "state_readiness_enter_review"
        | "state_readiness_approve"
        | "state_readiness_add_requirement"
        | "state_readiness_satisfy_requirement"
        | "state_readiness_list_requirements"
        | "state_session_checkpoint_save"
        | "state_session_checkpoint_restore" => readiness::handle(pool, name, &args).await.unwrap(),
        _ => Err(anyhow::anyhow!("Unknown tool: {}", name)),
    }
}

fn build_static_tools_list() -> Value {
    json!([
        {
            "name": "state_snapshot",
            "description": "Returns a full project overview: constitution, specs, tasks, and recent events.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "agent": {"type": "string", "description": "Include memory_stats for this agent"},
                    "spec": {"type": "string", "description": "Scope memory_stats to this spec (requires agent)"}
                }
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
            "name": "state_project_context",
            "description": "Inspect the current repository, derive active project metadata, repo map, and validation commands, and persist them into architect memory.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "subpath": {
                        "type": "string",
                        "description": "Optional subdirectory inside the project root to inspect as a specific subproject."
                    }
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
                    "until": {"type": "string"},
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
                    "ttl_seconds": {"type": "integer", "description": "Optional time-to-live in seconds from now"},
                    "related_to": {"type": "array", "items": {"type": "string"}, "description": "Optional list of related memory keys"}
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
                    "description": {"type": "string"},
                    "content_hash": {"type": "string", "description": "Optional content hash (e.g. SHA-256) for integrity verification"}
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
        },
        {
            "name": "memory_find_related",
            "description": "Find memory entries whose related_to field references a given target. Target format: 'agent/key'.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "target": {"type": "string", "description": "Target reference in 'agent/key' format"},
                    "spec": {"type": "string", "description": "Optional scope for the search"}
                },
                "required": ["target"]
            }
        },
        {
            "name": "memory_gc",
            "description": "Garbage-collect soft-deleted and TTL-expired memory entries. Use dry_run=true to preview what would be removed without deleting anything.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "dry_run": {"type": "boolean", "description": "If true, report counts without deleting (default: false)", "default": false}
                }
            }
        },
        {
            "name": "state_prd_set",
            "description": "Write or overwrite docs/PRD.md in the project root. Use this to create or update the product requirements document.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "content": {"type": "string", "description": "Full markdown content to write to docs/PRD.md"}
                },
                "required": ["content"]
            }
        }
    ])
}

pub(crate) fn build_tools_list() -> Value {
    let mut base = build_static_tools_list();
    let arr = base.as_array_mut().expect("tools list must be array");
    arr.extend(evals::tool_descriptors());
    arr.extend(policy::tool_descriptors());
    arr.extend(sessions::tool_descriptors());
    arr.extend(readiness::tool_descriptors());
    base
}

pub(crate) fn canonical_tool_names() -> Vec<String> {
    build_tools_list()
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|tool| tool.get("name").and_then(|name| name.as_str()))
        .map(str::to_string)
        .collect()
}
