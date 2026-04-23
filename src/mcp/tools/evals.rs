use anyhow::{anyhow, bail, Context, Result};
use chrono::Utc;
use serde::Deserialize;
use serde_json::{json, Value};
use sqlx::SqlitePool;

use crate::sdd::evals::{
    compare_eval_run_to_latest_baseline, compare_eval_runs, get_eval_run_details,
    list_eval_run_details, record_eval_run, EvalRunFilters, NewEvalRun, NewEvalRunLink,
    NewEvalScorecardDimension, RecordEvalRun,
};

use super::args::{optional_i64, optional_str, required_str};

#[derive(Debug, Deserialize)]
struct ToolEvalDimension {
    name: String,
    status: String,
    #[serde(default)]
    score: Option<f64>,
    #[serde(default)]
    value: Option<String>,
    #[serde(default)]
    rationale: Option<String>,
    #[serde(default)]
    details: Option<Value>,
}

#[derive(Debug, Deserialize)]
struct ToolEvalLink {
    kind: String,
    #[serde(rename = "ref")]
    link_ref: String,
    #[serde(default = "default_link_relation")]
    relation: String,
}

pub(super) fn tool_descriptors() -> Vec<Value> {
    vec![
        json!({
            "name": "state_eval_create",
            "description": "Create a structured eval run with scorecard dimensions and provenance links.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "id": {"type": "string", "description": "Optional stable eval ID; auto-generated if omitted"},
                    "evaluator": {"type": "string", "description": "Evaluator identity"},
                    "target_kind": {"type": "string", "enum": ["spec", "task", "artifact", "scope"]},
                    "target_ref": {"type": "string", "description": "Target spec/task/artifact/scope reference"},
                    "spec": {"type": "string"},
                    "task": {"type": "string"},
                    "artifact_id": {"type": "string"},
                    "summary": {"type": "string"},
                    "rationale": {"type": "string"},
                    "outcome": {"type": "string", "enum": ["pass", "warn", "fail", "mixed", "unknown"]},
                    "overall_score": {"type": "number"},
                    "source": {"type": "string", "enum": ["recorded", "cli", "mcp"]},
                    "metadata": {"type": "object"},
                    "dimensions": {
                        "type": "array",
                        "items": {
                            "type": "object",
                            "properties": {
                                "name": {"type": "string"},
                                "status": {"type": "string"},
                                "score": {"type": "number"},
                                "value": {"type": "string"},
                                "rationale": {"type": "string"},
                                "details": {"type": "object"}
                            },
                            "required": ["name", "status"]
                        }
                    },
                    "links": {
                        "type": "array",
                        "items": {
                            "type": "object",
                            "properties": {
                                "kind": {"type": "string"},
                                "ref": {"type": "string"},
                                "relation": {"type": "string"}
                            },
                            "required": ["kind", "ref"]
                        }
                    }
                },
                "required": ["evaluator", "target_kind", "target_ref", "outcome"]
            }
        }),
        json!({
            "name": "state_eval_list",
            "description": "List eval runs with spec/task/artifact, status, source, target, and time-range filters.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "spec": {"type": "string"},
                    "task": {"type": "string"},
                    "artifact_id": {"type": "string"},
                    "outcome": {"type": "string"},
                    "evaluator": {"type": "string"},
                    "target_kind": {"type": "string"},
                    "target_ref": {"type": "string"},
                    "source": {"type": "string"},
                    "created_after": {"type": "string"},
                    "created_before": {"type": "string"},
                    "limit": {"type": "integer"},
                    "offset": {"type": "integer"}
                }
            }
        }),
        json!({
            "name": "state_eval_get",
            "description": "Fetch a single eval run with scorecard dimensions and provenance links.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "id": {"type": "string", "description": "Eval ID"}
                },
                "required": ["id"]
            }
        }),
        json!({
            "name": "state_eval_compare",
            "description": "Compare one eval run against another baseline and return per-dimension deltas.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "baseline_eval_id": {"type": "string"},
                    "current_eval_id": {"type": "string"}
                },
                "required": ["baseline_eval_id", "current_eval_id"]
            }
        }),
        json!({
            "name": "state_eval_latest_baseline",
            "description": "Compare an eval run against the latest earlier baseline for the same logical scope.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "current_eval_id": {"type": "string"}
                },
                "required": ["current_eval_id"]
            }
        }),
    ]
}

pub(super) async fn handle(pool: &SqlitePool, tool_name: &str, args: &Value) -> Option<Result<Value>> {
    match tool_name {
        "state_eval_create" => Some(handle_create(pool, args).await),
        "state_eval_list" => Some(handle_list(pool, args).await),
        "state_eval_get" => Some(handle_get(pool, args).await),
        "state_eval_compare" => Some(handle_compare(pool, args).await),
        "state_eval_latest_baseline" => Some(handle_latest_baseline(pool, args).await),
        _ => None,
    }
}

async fn handle_create(pool: &SqlitePool, args: &Value) -> Result<Value> {
    let id = args
        .get("id")
        .and_then(Value::as_str)
        .map(str::to_string)
        .unwrap_or_else(|| format!("eval-{}", Utc::now().timestamp_nanos_opt().unwrap_or(0)));
    let metadata = optional_object(args, "metadata")?.unwrap_or_else(|| json!({}));
    let dimensions = parse_dimensions(args.get("dimensions"))?;
    let links = parse_links(args.get("links"))?;

    let dimension_inputs = dimensions
        .iter()
        .map(|dimension| NewEvalScorecardDimension {
            eval_run_id: &id,
            dimension_name: &dimension.name,
            normalized_status: &dimension.status,
            normalized_score: dimension.score,
            normalized_value: dimension.value.as_deref(),
            rationale: dimension.rationale.as_deref(),
            details_json: dimension.details.clone().unwrap_or_else(|| json!({})),
        })
        .collect::<Vec<_>>();
    let link_inputs = links
        .iter()
        .map(|link| NewEvalRunLink {
            eval_run_id: &id,
            link_kind: &link.kind,
            link_ref: &link.link_ref,
            relation: &link.relation,
        })
        .collect::<Vec<_>>();

    let recorded = record_eval_run(
        pool,
        RecordEvalRun {
            run: NewEvalRun {
                id: &id,
                evaluator: required_str(args, "evaluator")?,
                target_kind: required_str(args, "target_kind")?,
                target_ref: required_str(args, "target_ref")?,
                spec: optional_str(args, "spec"),
                task: optional_str(args, "task"),
                artifact_id: optional_str(args, "artifact_id"),
                summary: optional_str(args, "summary"),
                rationale: optional_str(args, "rationale"),
                outcome: required_str(args, "outcome")?,
                overall_score: args.get("overall_score").and_then(Value::as_f64),
                source: optional_str(args, "source").unwrap_or("mcp"),
                metadata_json: metadata,
            },
            dimensions: dimension_inputs,
            links: link_inputs,
        },
    )
    .await?;

    Ok(json!(recorded))
}

async fn handle_list(pool: &SqlitePool, args: &Value) -> Result<Value> {
    Ok(json!(list_eval_run_details(
        pool,
        EvalRunFilters {
            spec: optional_str(args, "spec"),
            task: optional_str(args, "task"),
            artifact_id: optional_str(args, "artifact_id"),
            outcome: optional_str(args, "outcome"),
            evaluator: optional_str(args, "evaluator"),
            target_kind: optional_str(args, "target_kind"),
            target_ref: optional_str(args, "target_ref"),
            source: optional_str(args, "source"),
            created_after: optional_str(args, "created_after"),
            created_before: optional_str(args, "created_before"),
            limit: optional_i64(args, "limit"),
            offset: optional_i64(args, "offset"),
        },
    )
    .await?))
}

async fn handle_get(pool: &SqlitePool, args: &Value) -> Result<Value> {
    let id = required_str(args, "id")?;
    get_eval_run_details(pool, id)
        .await?
        .map(|details| json!(details))
        .ok_or_else(|| anyhow!("eval '{}' not found", id))
}

async fn handle_compare(pool: &SqlitePool, args: &Value) -> Result<Value> {
    Ok(json!(compare_eval_runs(
        pool,
        required_str(args, "baseline_eval_id")?,
        required_str(args, "current_eval_id")?,
    )
    .await?))
}

async fn handle_latest_baseline(pool: &SqlitePool, args: &Value) -> Result<Value> {
    let current_eval_id = required_str(args, "current_eval_id")?;
    let comparison = compare_eval_run_to_latest_baseline(pool, current_eval_id)
        .await?
        .ok_or_else(|| anyhow!("no earlier baseline eval found for '{}'", current_eval_id))?;
    Ok(json!(comparison))
}

fn optional_object(args: &Value, field: &str) -> Result<Option<Value>> {
    let Some(value) = args.get(field) else {
        return Ok(None);
    };

    if !value.is_object() {
        bail!("{} must be a JSON object", field);
    }

    Ok(Some(value.clone()))
}

fn parse_dimensions(value: Option<&Value>) -> Result<Vec<ToolEvalDimension>> {
    let Some(value) = value else {
        return Ok(Vec::new());
    };

    if !value.is_array() {
        bail!("dimensions must be a JSON array of objects");
    }

    serde_json::from_value(value.clone()).context("dimensions must be an array of objects")
}

fn parse_links(value: Option<&Value>) -> Result<Vec<ToolEvalLink>> {
    let Some(value) = value else {
        return Ok(Vec::new());
    };

    if !value.is_array() {
        bail!("links must be a JSON array of objects");
    }

    serde_json::from_value(value.clone()).context("links must be an array of objects")
}

fn default_link_relation() -> String {
    "context".to_string()
}
