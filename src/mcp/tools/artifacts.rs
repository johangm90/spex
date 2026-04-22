use anyhow::Result;
use serde_json::{json, Value};
use sqlx::SqlitePool;

use crate::sdd::artifact::{query_artifacts, register_artifact};

use super::args::{optional_str, required_str};

pub(super) async fn handle_register(pool: &SqlitePool, args: Value) -> Result<Value> {
    let artifact = register_artifact(
        pool,
        required_str(&args, "id")?,
        optional_str(&args, "spec"),
        optional_str(&args, "task"),
        required_str(&args, "agent")?,
        required_str(&args, "type")?,
        optional_str(&args, "path"),
        optional_str(&args, "description"),
        optional_str(&args, "content_hash"),
    )
    .await?;

    Ok(json!(artifact))
}

pub(super) async fn handle_query(pool: &SqlitePool, args: Value) -> Result<Value> {
    let artifacts = query_artifacts(
        pool,
        optional_str(&args, "spec"),
        optional_str(&args, "task"),
        optional_str(&args, "agent"),
        optional_str(&args, "type"),
    )
    .await?;

    Ok(json!(artifacts))
}
