use anyhow::{anyhow, Result};
use serde_json::{json, Value};
use sqlx::SqlitePool;

use crate::sdd::{
    event::{emit_event, query_events},
    spec::{
        create_spec, get_spec, list_specs, update_spec_ac, update_spec_agents, update_spec_status,
        SpecStatus,
    },
    task::{
        create_task, get_task, list_tasks, update_task_output_artifact, update_task_status,
        TaskStatus,
    },
};

use super::args::{optional_i64, optional_str, required_str, string_array};

pub(super) async fn handle_slice_get(pool: &SqlitePool, args: Value) -> Result<Value> {
    if let Some(id) = optional_str(&args, "id") {
        Ok(json!(get_spec(pool, id).await?))
    } else {
        Ok(json!(
            list_specs(
                pool,
                optional_i64(&args, "limit"),
                optional_i64(&args, "offset")
            )
            .await?
        ))
    }
}

pub(super) async fn handle_slice_create(pool: &SqlitePool, args: Value) -> Result<Value> {
    let id = required_str(&args, "id")?;
    let title = required_str(&args, "title")?;
    let priority = optional_str(&args, "priority").unwrap_or("P1");
    let depends_on = string_array(&args, "depends_on")?;

    let spec = create_spec(pool, id, title, priority, &depends_on).await?;

    let agents = string_array(&args, "agents")?;
    if !agents.is_empty() {
        update_spec_agents(pool, id, &agents).await?;
    }

    Ok(json!(spec))
}

pub(super) async fn handle_slice_update(pool: &SqlitePool, args: Value) -> Result<Value> {
    let id = required_str(&args, "id")?;
    let updated_by = optional_str(&args, "updated_by").unwrap_or("agent");

    if let Some(status) = optional_str(&args, "status") {
        SpecStatus::from_str(status).ok_or_else(|| {
            anyhow!(
                "Invalid spec status '{}'. Valid: draft, approved, in_progress, done, paused",
                status
            )
        })?;
        update_spec_status(pool, id, status, updated_by).await?;
    }

    if let Some(ac_total) = optional_i64(&args, "ac_total") {
        let ac_passed = optional_i64(&args, "ac_passed").unwrap_or(0);
        update_spec_ac(pool, id, ac_total, ac_passed).await?;
    } else if let Some(ac_passed) = optional_i64(&args, "ac_passed") {
        let spec = get_spec(pool, id)
            .await?
            .ok_or_else(|| anyhow!("Spec not found: {}", id))?;
        update_spec_ac(pool, id, spec.ac_total, ac_passed).await?;
    }

    let agents = string_array(&args, "agents")?;
    if !agents.is_empty() {
        update_spec_agents(pool, id, &agents).await?;
    }

    Ok(json!(get_spec(pool, id)
        .await?
        .ok_or_else(|| anyhow!("Spec not found: {}", id))?))
}

pub(super) async fn handle_task_get(pool: &SqlitePool, args: Value) -> Result<Value> {
    if let Some(id) = optional_str(&args, "id") {
        Ok(json!(get_task(pool, id).await?))
    } else {
        Ok(json!(
            list_tasks(
                pool,
                optional_str(&args, "spec"),
                optional_i64(&args, "limit"),
                optional_i64(&args, "offset"),
            )
            .await?
        ))
    }
}

pub(super) async fn handle_task_create(pool: &SqlitePool, args: Value) -> Result<Value> {
    let inputs = string_array(&args, "inputs")?;

    Ok(json!(
        create_task(
            pool,
            required_str(&args, "id")?,
            required_str(&args, "spec")?,
            required_str(&args, "title")?,
            required_str(&args, "agent")?,
            &inputs,
            optional_str(&args, "output_artifact"),
        )
        .await?
    ))
}

pub(super) async fn handle_task_update(pool: &SqlitePool, args: Value) -> Result<Value> {
    let id = required_str(&args, "id")?;

    if let Some(status) = optional_str(&args, "status") {
        TaskStatus::from_str(status).ok_or_else(|| {
            anyhow!(
                "Invalid task status '{}'. Valid: pending, in_progress, done, failed",
                status
            )
        })?;
        update_task_status(pool, id, status).await?;
    }

    if let Some(artifact) = optional_str(&args, "output_artifact") {
        update_task_output_artifact(pool, id, artifact).await?;
    }

    Ok(json!(get_task(pool, id)
        .await?
        .ok_or_else(|| anyhow!("Task not found: {}", id))?))
}

pub(super) async fn handle_event_emit(pool: &SqlitePool, args: Value) -> Result<Value> {
    let payload = args
        .get("payload")
        .map(Value::to_string)
        .unwrap_or_else(|| "{}".to_string());

    emit_event(
        pool,
        required_str(&args, "type")?,
        optional_str(&args, "spec"),
        optional_str(&args, "agent"),
        &payload,
    )
    .await?;

    Ok(json!({"ok": true}))
}

pub(super) async fn handle_event_query(pool: &SqlitePool, args: Value) -> Result<Value> {
    Ok(json!(
        query_events(
            pool,
            optional_str(&args, "type"),
            optional_str(&args, "spec"),
            optional_str(&args, "agent"),
            optional_i64(&args, "limit"),
            optional_str(&args, "since"),
            optional_str(&args, "until"),
            optional_i64(&args, "offset"),
        )
        .await?
    ))
}
