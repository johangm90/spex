use anyhow::Result;
use sqlx::SqlitePool;

pub async fn memory_set(
    pool: &SqlitePool,
    agent: &str,
    key: &str,
    value_json: &str,
    spec: Option<&str>,
) -> Result<()> {
    let spec = spec.unwrap_or("");

    sqlx::query(
        "INSERT INTO memory (agent, key, value, spec) VALUES (?, ?, ?, ?) \
         ON CONFLICT(agent, spec, key) DO UPDATE SET value = excluded.value, \
         updated_at = strftime('%Y-%m-%dT%H:%M:%fZ', 'now')",
    )
    .bind(agent)
    .bind(key)
    .bind(value_json)
    .bind(spec)
    .execute(pool)
    .await?;
    Ok(())
}

pub async fn memory_get(
    pool: &SqlitePool,
    agent: &str,
    key: &str,
    spec: Option<&str>,
) -> Result<Option<String>> {
    let row = if let Some(spec) = spec {
        sqlx::query_as::<_, (String,)>(
            "SELECT value FROM memory WHERE agent = ? AND key = ? AND spec = ?",
        )
        .bind(agent)
        .bind(key)
        .bind(spec)
        .fetch_optional(pool)
        .await?
    } else {
        sqlx::query_as::<_, (String,)>(
            "SELECT value FROM memory WHERE agent = ? AND key = ? ORDER BY updated_at DESC LIMIT 1",
        )
        .bind(agent)
        .bind(key)
        .fetch_optional(pool)
        .await?
    };

    Ok(row.map(|(v,)| v))
}

/// IMP-007: always scope to `spec` when provided to prevent cross-spec contamination.
pub async fn memory_get_all(
    pool: &SqlitePool,
    agent: &str,
    spec: Option<&str>,
) -> Result<Vec<(String, String)>> {
    let rows = if let Some(spec) = spec {
        sqlx::query_as::<_, (String, String)>(
            "SELECT key, value FROM memory WHERE agent = ? AND spec = ? ORDER BY key",
        )
        .bind(agent)
        .bind(spec)
        .fetch_all(pool)
        .await?
    } else {
        sqlx::query_as::<_, (String, String)>(
            "SELECT key, value FROM memory WHERE agent = ? ORDER BY key",
        )
        .bind(agent)
        .fetch_all(pool)
        .await?
    };

    Ok(rows)
}
