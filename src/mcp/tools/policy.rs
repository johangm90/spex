use anyhow::{anyhow, Result};
use chrono::Utc;
use serde_json::{json, Value};
use sqlx::SqlitePool;

use crate::sdd::{
    evidence::{
        create_evidence_bundle, get_evidence_bundle_for_entity, list_evidence_bundles,
        EvidenceBundleStatus, EvidenceRef, NewEvidenceBundle,
    },
    policy::{
        create_approval, create_policy_config, decide_approval, get_policy_config_by_scope_key,
        list_approvals, resolve_effective_policy, update_policy_config, ApprovalDecision,
        ApprovalEntityKind, ApprovalStatus, CreateApproval, CreatePolicyConfig, EnforcementMode,
        PolicyScopeKey, PolicyScopeKind, UpdatePolicyConfig,
    },
};

use super::args::{optional_bool, optional_str, required_str, string_array};

// ─── policy_config_set ────────────────────────────────────────────────────────

pub(super) async fn handle_config_set(pool: &SqlitePool, args: Value) -> Result<Value> {
    let spec = optional_str(&args, "spec");
    let task = optional_str(&args, "task");
    let agent = optional_str(&args, "agent");
    let require_evidence = optional_bool(&args, "require_evidence").unwrap_or(false);
    let require_approval = optional_bool(&args, "require_approval").unwrap_or(false);
    let risky_ops = string_array(&args, "risky_operations")?;

    let (scope_kind, scope_ref) = resolve_scope(spec, task)?;

    let rules = build_rules_json(require_evidence, require_approval, &risky_ops);

    let key = PolicyScopeKey::new(scope_kind, scope_ref, agent)?;
    let config = match get_policy_config_by_scope_key(pool, &key).await? {
        Some(existing) => {
            update_policy_config(
                pool,
                &existing.id,
                UpdatePolicyConfig {
                    enabled: true,
                    enforcement_mode: EnforcementMode::Enforced,
                    rules_json: &rules,
                    rationale: None,
                    updated_by: agent,
                },
            )
            .await?
        }
        None => {
            let id = format!("pcfg-{}", Utc::now().timestamp_nanos_opt().unwrap_or(0));
            create_policy_config(
                pool,
                CreatePolicyConfig {
                    id: &id,
                    scope_kind,
                    scope_ref: &key.scope_ref,
                    agent,
                    enabled: true,
                    enforcement_mode: EnforcementMode::Enforced,
                    rules_json: &rules,
                    rationale: None,
                    created_by: agent,
                },
            )
            .await?
        }
    };

    Ok(json!(config))
}

// ─── policy_config_get ────────────────────────────────────────────────────────

pub(super) async fn handle_config_get(pool: &SqlitePool, args: Value) -> Result<Value> {
    let spec = optional_str(&args, "spec");
    let task = optional_str(&args, "task");
    let agent = optional_str(&args, "agent");

    // Determine spec_status for resolution — default to "in_progress" so policy is enforced
    let spec_status = if let Some(spec_id) = spec {
        crate::sdd::spec::get_spec(pool, spec_id)
            .await?
            .map(|s| s.status)
            .unwrap_or_else(|| "in_progress".to_string())
    } else {
        "in_progress".to_string()
    };

    let effective = resolve_effective_policy(pool, spec, task, agent, &spec_status).await?;

    Ok(json!({
        "spec_ref": effective.spec_ref,
        "task_ref": effective.task_ref,
        "agent": effective.agent,
        "enforcement_mode": effective.enforcement_mode.as_str(),
        "fail_closed": effective.fail_closed,
        "allowed_write_scopes": effective.allowed_write_scopes,
        "task_completion": {
            "require_evidence_bundle": effective.task_completion.require_evidence_bundle,
            "require_rationale": effective.task_completion.require_rationale,
            "require_validation": effective.task_completion.require_validation.map(|v| v.as_str()),
            "require_approval": effective.task_completion.require_approval,
        },
        "spec_completion": {
            "require_evidence_bundle": effective.spec_completion.require_evidence_bundle,
            "require_rationale": effective.spec_completion.require_rationale,
            "require_validation": effective.spec_completion.require_validation.map(|v| v.as_str()),
            "require_approval": effective.spec_completion.require_approval,
        },
        "sources": effective.sources.iter().map(|s| json!({
            "id": s.id,
            "scope_kind": s.scope_kind.as_str(),
            "scope_ref": s.scope_ref,
            "agent": s.agent,
            "enforcement_mode": s.enforcement_mode.as_str(),
        })).collect::<Vec<_>>(),
    }))
}

// ─── policy_evidence_add ──────────────────────────────────────────────────────

pub(super) async fn handle_evidence_add(pool: &SqlitePool, args: Value) -> Result<Value> {
    let task = optional_str(&args, "task");
    let spec = optional_str(&args, "spec");
    let summary = required_str(&args, "summary")?;
    let passed = optional_bool(&args, "passed").unwrap_or(true);

    let evidence_ref = build_evidence_ref(spec, task)?;

    // Upsert: if a bundle already exists for this entity, update it; otherwise create
    let bundle = match get_evidence_bundle_for_entity(pool, &evidence_ref).await? {
        Some(existing) => {
            crate::sdd::evidence::update_evidence_bundle(
                pool,
                &existing.id,
                crate::sdd::evidence::EvidenceBundlePatch {
                    status: if passed {
                        EvidenceBundleStatus::Submitted
                    } else {
                        EvidenceBundleStatus::Draft
                    },
                    summary: Some(summary),
                    behavior_change: existing.behavior_change,
                    metadata_json: existing.metadata_json,
                    updated_by: None,
                },
            )
            .await?
        }
        None => {
            let id = format!("evb-{}", Utc::now().timestamp_nanos_opt().unwrap_or(0));
            create_evidence_bundle(
                pool,
                NewEvidenceBundle {
                    id: &id,
                    reference: evidence_ref,
                    status: if passed {
                        EvidenceBundleStatus::Submitted
                    } else {
                        EvidenceBundleStatus::Draft
                    },
                    summary: Some(summary),
                    behavior_change: false,
                    metadata_json: json!({}),
                    created_by: None,
                    updated_by: None,
                },
            )
            .await?
        }
    };

    Ok(json!(bundle))
}

// ─── policy_evidence_list ─────────────────────────────────────────────────────

pub(super) async fn handle_evidence_list(pool: &SqlitePool, args: Value) -> Result<Value> {
    let spec = optional_str(&args, "spec");
    let task = optional_str(&args, "task");

    let bundles = list_evidence_bundles(pool, spec, task, None).await?;
    Ok(json!(bundles))
}

// ─── policy_approval_request ──────────────────────────────────────────────────

pub(super) async fn handle_approval_request(pool: &SqlitePool, args: Value) -> Result<Value> {
    let task = optional_str(&args, "task");
    let spec = optional_str(&args, "spec");
    let operation = required_str(&args, "operation")?;
    let reason = required_str(&args, "reason")?;

    let (entity_kind, entity_id) = resolve_approval_entity(spec, task)?;

    let id = format!("appr-{}", Utc::now().timestamp_nanos_opt().unwrap_or(0));
    let context = json!({
        "reason": reason,
        "spec_status": "in_progress",
    });

    let approval = create_approval(
        pool,
        CreateApproval {
            id: &id,
            entity_kind,
            entity_id,
            spec,
            task,
            operation_kind: operation,
            policy_config_id: None,
            evidence_bundle_id: None,
            requested_by: "mcp-agent",
            request_context_json: &context,
            expires_at: None,
        },
    )
    .await?;

    Ok(json!(approval))
}

// ─── policy_approval_decide ───────────────────────────────────────────────────

pub(super) async fn handle_approval_decide(pool: &SqlitePool, args: Value) -> Result<Value> {
    let approval_id = required_str(&args, "approval_id")?;
    let decision_str = required_str(&args, "decision")?;
    let note = optional_str(&args, "note");

    let status = ApprovalStatus::from_str(decision_str).ok_or_else(|| {
        anyhow!(
            "Invalid decision '{}'. Valid: approved, rejected",
            decision_str
        )
    })?;

    if !status.is_terminal() {
        return Err(anyhow!(
            "Decision must be 'approved' or 'rejected', got '{}'",
            decision_str
        ));
    }

    let approval = decide_approval(
        pool,
        approval_id,
        ApprovalDecision {
            status,
            decided_by: "mcp-agent",
            decision_reason: note,
        },
    )
    .await?;

    Ok(json!(approval))
}

// ─── policy_approval_list ─────────────────────────────────────────────────────

pub(super) async fn handle_approval_list(pool: &SqlitePool, args: Value) -> Result<Value> {
    let spec = optional_str(&args, "spec");
    let task = optional_str(&args, "task");
    let status_str = optional_str(&args, "status");

    let status_filter = status_str
        .map(|s| {
            ApprovalStatus::from_str(s).ok_or_else(|| {
                anyhow!("Invalid status '{}'. Valid: pending, approved, rejected", s)
            })
        })
        .transpose()?;

    // Determine entity filter from spec/task
    let (entity_kind, entity_id) = match (spec, task) {
        (_, Some(task_id)) => (Some(ApprovalEntityKind::Task), Some(task_id)),
        (Some(spec_id), None) => (Some(ApprovalEntityKind::Spec), Some(spec_id)),
        (None, None) => (None, None),
    };

    let approvals = list_approvals(pool, entity_kind, entity_id, None, status_filter).await?;
    Ok(json!(approvals))
}

// ─── helpers ──────────────────────────────────────────────────────────────────

fn resolve_scope<'a>(
    spec: Option<&'a str>,
    task: Option<&'a str>,
) -> Result<(PolicyScopeKind, &'a str)> {
    if let Some(task_id) = task {
        return Ok((PolicyScopeKind::Task, task_id));
    }
    if let Some(spec_id) = spec {
        return Ok((PolicyScopeKind::Spec, spec_id));
    }
    Ok((PolicyScopeKind::Project, "project"))
}

fn build_rules_json(require_evidence: bool, require_approval: bool, risky_ops: &[String]) -> Value {
    let mut rules = json!({});
    if require_evidence {
        rules["require_evidence_bundle"] = json!(true);
        rules["require_rationale"] = json!(true);
    }
    if require_approval {
        rules["require_approval"] = json!(true);
    }
    if !risky_ops.is_empty() {
        let mut ops = json!({});
        for op in risky_ops {
            ops[op] = json!("require_approval");
        }
        rules["risky_operations"] = ops;
    }
    rules
}

fn build_evidence_ref(spec: Option<&str>, task: Option<&str>) -> Result<EvidenceRef> {
    match (spec, task) {
        (Some(spec_id), Some(task_id)) => Ok(EvidenceRef::for_task(spec_id, task_id)),
        (Some(spec_id), None) => Ok(EvidenceRef::for_spec(spec_id)),
        (None, Some(task_id)) => Ok(EvidenceRef::for_task(task_id, task_id)),
        (None, None) => Err(anyhow!("Either 'spec' or 'task' must be provided")),
    }
}

fn resolve_approval_entity<'a>(
    spec: Option<&'a str>,
    task: Option<&'a str>,
) -> Result<(ApprovalEntityKind, &'a str)> {
    if let Some(task_id) = task {
        return Ok((ApprovalEntityKind::Task, task_id));
    }
    if let Some(spec_id) = spec {
        return Ok((ApprovalEntityKind::Spec, spec_id));
    }
    Err(anyhow!("Either 'spec' or 'task' must be provided"))
}

// ─── tool descriptors ─────────────────────────────────────────────────────────

pub(super) fn tool_descriptors() -> Vec<Value> {
    vec![
        json!({
            "name": "policy_config_set",
            "description": "Set policy config for a spec, task, or project scope. Controls evidence requirements, approval gates, and risky operation dispositions.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "spec": {"type": "string", "description": "Spec ID to scope the policy to"},
                    "task": {"type": "string", "description": "Task ID to scope the policy to (takes precedence over spec)"},
                    "agent": {"type": "string", "description": "Agent to scope the policy to"},
                    "require_evidence": {"type": "boolean", "description": "Require an evidence bundle before completion"},
                    "require_approval": {"type": "boolean", "description": "Require human approval before completion"},
                    "risky_operations": {
                        "type": "array",
                        "items": {"type": "string"},
                        "description": "List of risky operation kinds to require approval for (e.g. destructive_command, schema_change)"
                    }
                }
            }
        }),
        json!({
            "name": "policy_config_get",
            "description": "Get the effective (resolved) policy for a spec/task/agent combination.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "spec": {"type": "string", "description": "Spec ID"},
                    "task": {"type": "string", "description": "Task ID"},
                    "agent": {"type": "string", "description": "Agent name"}
                }
            }
        }),
        json!({
            "name": "policy_evidence_add",
            "description": "Submit an evidence bundle for a task or spec. Creates or updates the bundle for the given entity.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "task": {"type": "string", "description": "Task ID"},
                    "spec": {"type": "string", "description": "Spec ID"},
                    "kind": {
                        "type": "string",
                        "enum": ["test_run", "lint", "build", "review", "manual"],
                        "description": "Kind of evidence"
                    },
                    "summary": {"type": "string", "description": "Human-readable summary of the evidence"},
                    "detail": {"type": "string", "description": "Optional detailed description"},
                    "passed": {"type": "boolean", "description": "Whether the evidence indicates success"},
                    "artifact_id": {"type": "string", "description": "Optional artifact ID to link"}
                },
                "required": ["summary"]
            }
        }),
        json!({
            "name": "policy_evidence_list",
            "description": "List evidence bundles for a task or spec.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "task": {"type": "string", "description": "Task ID filter"},
                    "spec": {"type": "string", "description": "Spec ID filter"}
                }
            }
        }),
        json!({
            "name": "policy_approval_request",
            "description": "Request approval for a risky operation on a task or spec.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "task": {"type": "string", "description": "Task ID"},
                    "spec": {"type": "string", "description": "Spec ID"},
                    "operation": {"type": "string", "description": "Operation kind (e.g. complete_task, destructive_command, schema_change)"},
                    "reason": {"type": "string", "description": "Reason for requesting approval"}
                },
                "required": ["operation", "reason"]
            }
        }),
        json!({
            "name": "policy_approval_decide",
            "description": "Approve or reject a pending approval request.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "approval_id": {"type": "string", "description": "ID of the approval to decide"},
                    "decision": {
                        "type": "string",
                        "enum": ["approved", "rejected"],
                        "description": "The decision"
                    },
                    "note": {"type": "string", "description": "Optional note explaining the decision"}
                },
                "required": ["approval_id", "decision"]
            }
        }),
        json!({
            "name": "policy_approval_list",
            "description": "List approval requests, optionally filtered by spec, task, or status.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "spec": {"type": "string", "description": "Filter by spec ID"},
                    "task": {"type": "string", "description": "Filter by task ID"},
                    "status": {
                        "type": "string",
                        "enum": ["pending", "approved", "rejected"],
                        "description": "Filter by approval status"
                    }
                }
            }
        }),
    ]
}
