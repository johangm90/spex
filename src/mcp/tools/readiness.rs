use anyhow::Result;
use chrono::Utc;
use serde_json::{json, Value};
use sqlx::SqlitePool;

use crate::sdd::readiness::{
    approve_review, enter_review, get_checkpoint_by_id, get_latest_checkpoint,
    insert_review_requirement, list_review_requirements, operator_readiness,
    satisfy_review_requirement, save_checkpoint, spec_readiness, transition_phase,
    ReviewRequirementKind, WorkflowPhaseKind,
};

use super::args::{optional_str, required_str};

pub(super) fn tool_descriptors() -> Vec<Value> {
    vec![
        json!({
            "name": "state_readiness_spec",
            "description": "Get readiness report for a spec.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "spec_id": {"type": "string", "description": "Spec ID"}
                },
                "required": ["spec_id"]
            }
        }),
        json!({
            "name": "state_readiness_operator",
            "description": "Get readiness report across all specs.",
            "inputSchema": {
                "type": "object",
                "properties": {}
            }
        }),
        json!({
            "name": "state_readiness_phase_transition",
            "description": "Transition a spec to a new workflow phase.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "spec_id": {"type": "string", "description": "Spec ID"},
                    "phase": {"type": "string", "description": "Target phase: planning | in_progress | review | done"},
                    "entered_by": {"type": "string", "description": "Agent or user performing the transition"},
                    "notes": {"type": "string", "description": "Optional notes"}
                },
                "required": ["spec_id", "phase"]
            }
        }),
        json!({
            "name": "state_readiness_enter_review",
            "description": "Enter review phase and seed default requirements.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "spec_id": {"type": "string", "description": "Spec ID"},
                    "agent": {"type": "string", "description": "Agent entering review"}
                },
                "required": ["spec_id"]
            }
        }),
        json!({
            "name": "state_readiness_approve",
            "description": "Approve review for a spec.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "spec_id": {"type": "string", "description": "Spec ID"},
                    "approved_by": {"type": "string", "description": "Agent or user approving"}
                },
                "required": ["spec_id", "approved_by"]
            }
        }),
        json!({
            "name": "state_readiness_add_requirement",
            "description": "Add a review requirement to a spec.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "spec_id": {"type": "string", "description": "Spec ID"},
                    "kind": {"type": "string", "description": "Requirement kind: test_pass | lint_pass | review_approved | custom"},
                    "description": {"type": "string", "description": "Human-readable description of the requirement"}
                },
                "required": ["spec_id", "kind", "description"]
            }
        }),
        json!({
            "name": "state_readiness_satisfy_requirement",
            "description": "Satisfy a review requirement.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "req_id": {"type": "string", "description": "Requirement ID"},
                    "satisfied_by": {"type": "string", "description": "Agent or user satisfying the requirement"}
                },
                "required": ["req_id", "satisfied_by"]
            }
        }),
        json!({
            "name": "state_readiness_list_requirements",
            "description": "List review requirements for a spec.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "spec_id": {"type": "string", "description": "Spec ID"}
                },
                "required": ["spec_id"]
            }
        }),
        json!({
            "name": "state_session_checkpoint_save",
            "description": "Save a session checkpoint.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "session_id": {"type": "string", "description": "Session ID"},
                    "agent": {"type": "string", "description": "Agent saving the checkpoint"},
                    "spec_id": {"type": "string", "description": "Optional spec ID"},
                    "task_id": {"type": "string", "description": "Optional task ID"},
                    "label": {"type": "string", "description": "Optional human-readable label"},
                    "checkpoint_data": {"type": "object", "description": "Checkpoint payload (arbitrary JSON object)"}
                },
                "required": ["session_id", "agent", "checkpoint_data"]
            }
        }),
        json!({
            "name": "state_session_checkpoint_restore",
            "description": "Restore a session checkpoint. Returns the latest checkpoint if checkpoint_id is omitted.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "session_id": {"type": "string", "description": "Session ID"},
                    "checkpoint_id": {"type": "string", "description": "Optional checkpoint ID; omit to get the latest"}
                },
                "required": ["session_id"]
            }
        }),
    ]
}

pub(super) async fn handle(
    pool: &SqlitePool,
    tool_name: &str,
    args: &Value,
) -> Option<Result<Value>> {
    match tool_name {
        "state_readiness_spec" => Some(handle_readiness_spec(pool, args).await),
        "state_readiness_operator" => Some(handle_readiness_operator(pool).await),
        "state_readiness_phase_transition" => Some(handle_phase_transition(pool, args).await),
        "state_readiness_enter_review" => Some(handle_enter_review(pool, args).await),
        "state_readiness_approve" => Some(handle_approve(pool, args).await),
        "state_readiness_add_requirement" => Some(handle_add_requirement(pool, args).await),
        "state_readiness_satisfy_requirement" => Some(handle_satisfy_requirement(pool, args).await),
        "state_readiness_list_requirements" => Some(handle_list_requirements(pool, args).await),
        "state_session_checkpoint_save" => Some(handle_checkpoint_save(pool, args).await),
        "state_session_checkpoint_restore" => Some(handle_checkpoint_restore(pool, args).await),
        _ => None,
    }
}

async fn handle_readiness_spec(pool: &SqlitePool, args: &Value) -> Result<Value> {
    let spec_id = required_str(args, "spec_id")?;
    let report = spec_readiness(pool, spec_id).await?;
    Ok(serde_json::to_value(report)?)
}

async fn handle_readiness_operator(pool: &SqlitePool) -> Result<Value> {
    let report = operator_readiness(pool).await?;
    Ok(serde_json::to_value(report)?)
}

async fn handle_phase_transition(pool: &SqlitePool, args: &Value) -> Result<Value> {
    let spec_id = required_str(args, "spec_id")?;
    let phase_str = required_str(args, "phase")?;
    let entered_by = optional_str(args, "entered_by");
    let notes = optional_str(args, "notes");

    let phase = WorkflowPhaseKind::from_str(phase_str)
        .ok_or_else(|| anyhow::anyhow!("Unknown phase: {}", phase_str))?;

    let result = transition_phase(pool, spec_id, phase, entered_by, notes).await?;

    Ok(json!({
        "id": result.id,
        "spec_id": result.spec_id,
        "phase": result.phase,
        "entered_at": result.entered_at,
        "entered_by": result.entered_by,
        "notes": result.notes,
    }))
}

async fn handle_enter_review(pool: &SqlitePool, args: &Value) -> Result<Value> {
    let spec_id = required_str(args, "spec_id")?;
    let agent = optional_str(args, "agent");

    let result = enter_review(pool, spec_id, agent).await?;

    Ok(json!({
        "id": result.id,
        "spec_id": result.spec_id,
        "phase": result.phase,
        "entered_at": result.entered_at,
        "entered_by": result.entered_by,
    }))
}

async fn handle_approve(pool: &SqlitePool, args: &Value) -> Result<Value> {
    let spec_id = required_str(args, "spec_id")?;
    let approved_by = required_str(args, "approved_by")?;

    let transitioned_to_done = approve_review(pool, spec_id, approved_by).await?;

    Ok(json!({ "transitioned_to_done": transitioned_to_done }))
}

async fn handle_add_requirement(pool: &SqlitePool, args: &Value) -> Result<Value> {
    let spec_id = required_str(args, "spec_id")?;
    let kind_str = required_str(args, "kind")?;
    let description = required_str(args, "description")?;

    let kind = ReviewRequirementKind::from_str(kind_str)
        .ok_or_else(|| anyhow::anyhow!("Unknown requirement kind: {}", kind_str))?;

    let id = format!("req-{}", Utc::now().timestamp_nanos_opt().unwrap_or(0));

    let req = insert_review_requirement(pool, &id, spec_id, kind, description).await?;

    Ok(json!({
        "id": req.id,
        "spec_id": req.spec_id,
        "kind": req.kind,
        "description": req.description,
        "satisfied": req.satisfied,
        "created_at": req.created_at,
    }))
}

async fn handle_satisfy_requirement(pool: &SqlitePool, args: &Value) -> Result<Value> {
    let req_id = required_str(args, "req_id")?;
    let satisfied_by = required_str(args, "satisfied_by")?;

    satisfy_review_requirement(pool, req_id, Some(satisfied_by)).await?;

    Ok(json!({ "req_id": req_id, "satisfied": true, "satisfied_by": satisfied_by }))
}

async fn handle_list_requirements(pool: &SqlitePool, args: &Value) -> Result<Value> {
    let spec_id = required_str(args, "spec_id")?;
    let reqs = list_review_requirements(pool, spec_id).await?;

    let items: Vec<Value> = reqs
        .iter()
        .map(|r| {
            json!({
                "id": r.id,
                "spec_id": r.spec_id,
                "kind": r.kind,
                "description": r.description,
                "satisfied": r.satisfied,
                "satisfied_at": r.satisfied_at,
                "satisfied_by": r.satisfied_by,
                "created_at": r.created_at,
            })
        })
        .collect();

    Ok(json!({ "requirements": items, "count": items.len() }))
}

async fn handle_checkpoint_save(pool: &SqlitePool, args: &Value) -> Result<Value> {
    let session_id = required_str(args, "session_id")?;
    let agent = required_str(args, "agent")?;
    let spec_id = optional_str(args, "spec_id");
    let task_id = optional_str(args, "task_id");
    let label = optional_str(args, "label");

    let checkpoint_data = args
        .get("checkpoint_data")
        .ok_or_else(|| anyhow::anyhow!("Missing required argument: checkpoint_data"))?;
    let data_json = serde_json::to_string(checkpoint_data)?;

    let id = format!("cp-{}", Utc::now().timestamp_nanos_opt().unwrap_or(0));

    let cp = save_checkpoint(
        pool, &id, session_id, spec_id, task_id, agent, &data_json, label,
    )
    .await?;

    Ok(json!({
        "id": cp.id,
        "session_id": cp.session_id,
        "spec_id": cp.spec_id,
        "task_id": cp.task_id,
        "agent": cp.agent,
        "label": cp.label,
        "saved_at": cp.saved_at,
    }))
}

async fn handle_checkpoint_restore(pool: &SqlitePool, args: &Value) -> Result<Value> {
    let session_id = required_str(args, "session_id")?;
    let checkpoint_id = optional_str(args, "checkpoint_id");

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
                "spec_id": c.spec_id,
                "task_id": c.task_id,
                "agent": c.agent,
                "label": c.label,
                "saved_at": c.saved_at,
                "checkpoint_data": checkpoint_data,
            }))
        }
    }
}
