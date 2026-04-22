use anyhow::Result;
use serde_json::{json, Value};
use sqlx::SqlitePool;

use crate::sdd::memory::{
    memory_context, memory_delete, memory_find_referencing, memory_gc, memory_get_full,
    memory_list, memory_search, memory_set, memory_stats,
};

use super::args::{optional_bool, optional_i64, optional_str, related_to_json, required_str};

fn parse_memory_value(raw: &str) -> Value {
    serde_json::from_str(raw).unwrap_or_else(|_| Value::String(raw.to_string()))
}

pub(super) async fn handle_set(pool: &SqlitePool, args: Value) -> Result<Value> {
    let value = args
        .get("value")
        .ok_or_else(|| anyhow::anyhow!("Missing field: value"))?
        .to_string();
    let related_to = related_to_json(&args)?;

    memory_set(
        pool,
        required_str(&args, "agent")?,
        required_str(&args, "key")?,
        &value,
        optional_str(&args, "spec"),
        optional_str(&args, "type"),
        optional_i64(&args, "ttl_seconds"),
        related_to.as_deref(),
    )
    .await?;

    Ok(json!({"ok": true}))
}

pub(super) async fn handle_get(pool: &SqlitePool, args: Value) -> Result<Value> {
    let agent = required_str(&args, "agent")?;
    let spec = optional_str(&args, "spec");

    if let Some(key) = optional_str(&args, "key") {
        if let Some(memory) = memory_get_full(pool, agent, key, spec).await? {
            let related_to: Value =
                serde_json::from_str(&memory.related_to).unwrap_or_else(|_| json!([]));
            Ok(json!({
                "value": parse_memory_value(&memory.value),
                "type": memory.type_,
                "access_count": memory.access_count,
                "last_accessed_at": memory.last_accessed_at,
                "revision_count": memory.revision_count,
                "expires_at": memory.expires_at,
                "updated_at": memory.updated_at,
                "related_to": related_to,
            }))
        } else {
            Ok(json!({"value": null}))
        }
    } else {
        let entries = memory_list(pool, agent, spec, None, None, None).await?;
        Ok(json!({
            "entries": entries
                .into_iter()
                .map(|entry| json!({"key": entry.key, "value": parse_memory_value(&entry.value)}))
                .collect::<Vec<_>>()
        }))
    }
}

pub(super) async fn handle_search(pool: &SqlitePool, args: Value) -> Result<Value> {
    Ok(json!(
        memory_search(
            pool,
            required_str(&args, "agent")?,
            required_str(&args, "query")?,
            optional_str(&args, "spec"),
            optional_str(&args, "type"),
            optional_i64(&args, "limit"),
        )
        .await?
    ))
}

pub(super) async fn handle_delete(pool: &SqlitePool, args: Value) -> Result<Value> {
    Ok(json!({
        "deleted": memory_delete(
            pool,
            required_str(&args, "agent")?,
            required_str(&args, "key")?,
            optional_str(&args, "spec"),
        )
        .await?
    }))
}

pub(super) async fn handle_context(pool: &SqlitePool, args: Value) -> Result<Value> {
    Ok(json!(
        memory_context(
            pool,
            required_str(&args, "agent")?,
            optional_str(&args, "spec"),
            optional_i64(&args, "limit"),
        )
        .await?
    ))
}

pub(super) async fn handle_stats(pool: &SqlitePool, args: Value) -> Result<Value> {
    memory_stats(
        pool,
        required_str(&args, "agent")?,
        optional_str(&args, "spec"),
    )
    .await
}

pub(super) async fn handle_list(pool: &SqlitePool, args: Value) -> Result<Value> {
    Ok(json!(
        memory_list(
            pool,
            required_str(&args, "agent")?,
            optional_str(&args, "spec"),
            optional_str(&args, "type"),
            optional_i64(&args, "limit"),
            optional_i64(&args, "offset"),
        )
        .await?
    ))
}

pub(super) async fn handle_find_related(pool: &SqlitePool, args: Value) -> Result<Value> {
    Ok(json!(
        memory_find_referencing(
            pool,
            required_str(&args, "target")?,
            optional_str(&args, "spec"),
        )
        .await?
    ))
}

pub(super) async fn handle_gc(pool: &SqlitePool, args: Value) -> Result<Value> {
    let dry_run = optional_bool(&args, "dry_run").unwrap_or(false);
    let result = memory_gc(pool, dry_run).await?;
    Ok(json!({
        "deleted_count": result.deleted_count,
        "expired_count": result.expired_count,
        "sample_keys": result.sample_keys,
        "dry_run": dry_run,
    }))
}
