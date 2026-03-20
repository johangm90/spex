use anyhow::Result;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use sqlx::SqlitePool;

/// Full memory row, including all enhanced fields added in MEMS-001.
#[derive(Debug, Serialize, Deserialize, sqlx::FromRow)]
pub struct Memory {
    pub id: i64,
    pub agent: String,
    pub key: String,
    pub value: String,
    pub spec: String,
    pub updated_at: String,
    #[serde(rename = "type")]
    #[sqlx(rename = "type")]
    pub type_: Option<String>,
    pub deleted_at: Option<String>,
    pub expires_at: Option<String>,
    pub access_count: i64,
    pub last_accessed_at: Option<String>,
    pub revision_count: i64,
}

pub async fn memory_set(
    pool: &SqlitePool,
    agent: &str,
    key: &str,
    value_json: &str,
    spec: Option<&str>,
    mem_type: Option<&str>,
    ttl_seconds: Option<i64>,
) -> Result<()> {
    let spec = spec.unwrap_or("");

    // Compute expires_at from ttl_seconds if provided.
    // Use the same ISO-8601 format as all other timestamps (with T and Z) so that
    // SQLite string comparisons with strftime('%Y-%m-%dT%H:%M:%fZ','now') work correctly.
    let expires_at: Option<String> = ttl_seconds.map(|ttl| {
        let future = chrono::Utc::now() + chrono::Duration::seconds(ttl);
        future.format("%Y-%m-%dT%H:%M:%S%.3fZ").to_string()
    });

    sqlx::query(
        "INSERT INTO memory (agent, key, value, spec, type, expires_at, revision_count) \
         VALUES (?, ?, ?, ?, ?, ?, 1) \
         ON CONFLICT(agent, spec, key) DO UPDATE SET \
           value = excluded.value, \
           updated_at = strftime('%Y-%m-%dT%H:%M:%fZ', 'now'), \
           revision_count = revision_count + 1, \
           type = COALESCE(excluded.type, type), \
           expires_at = COALESCE(excluded.expires_at, expires_at)",
    )
    .bind(agent)
    .bind(key)
    .bind(value_json)
    .bind(spec)
    .bind(mem_type)
    .bind(expires_at)
    .execute(pool)
    .await?;
    Ok(())
}

/// Convenience wrapper: returns only the value string (no access tracking metadata).
/// Use `memory_get_full` when you need the complete Memory struct with access tracking.
#[allow(dead_code)]
pub async fn memory_get(
    pool: &SqlitePool,
    agent: &str,
    key: &str,
    spec: Option<&str>,
) -> Result<Option<String>> {
    let row = memory_get_full(pool, agent, key, spec).await?;
    Ok(row.map(|m| m.value))
}

/// Retrieve a full Memory row, filtering deleted and expired entries.
/// On a hit, bumps access_count and last_accessed_at.
pub async fn memory_get_full(
    pool: &SqlitePool,
    agent: &str,
    key: &str,
    spec: Option<&str>,
) -> Result<Option<Memory>> {
    let row: Option<Memory> = if let Some(spec) = spec {
        sqlx::query_as(
            "SELECT id, agent, key, value, spec, updated_at, type, deleted_at, expires_at, \
                    access_count, last_accessed_at, revision_count \
             FROM memory \
             WHERE agent = ? AND key = ? AND spec = ? \
               AND deleted_at IS NULL \
               AND (expires_at IS NULL OR expires_at >= strftime('%Y-%m-%dT%H:%M:%fZ', 'now'))",
        )
        .bind(agent)
        .bind(key)
        .bind(spec)
        .fetch_optional(pool)
        .await?
    } else {
        sqlx::query_as(
            "SELECT id, agent, key, value, spec, updated_at, type, deleted_at, expires_at, \
                    access_count, last_accessed_at, revision_count \
             FROM memory \
             WHERE agent = ? AND key = ? \
               AND deleted_at IS NULL \
               AND (expires_at IS NULL OR expires_at >= strftime('%Y-%m-%dT%H:%M:%fZ', 'now')) \
             ORDER BY updated_at DESC LIMIT 1",
        )
        .bind(agent)
        .bind(key)
        .fetch_optional(pool)
        .await?
    };

    if let Some(ref m) = row {
        // Bump access tracking
        sqlx::query(
            "UPDATE memory SET \
               access_count = access_count + 1, \
               last_accessed_at = strftime('%Y-%m-%dT%H:%M:%fZ', 'now') \
             WHERE id = ?",
        )
        .bind(m.id)
        .execute(pool)
        .await?;
    }

    Ok(row)
}

/// IMP-007: always scope to `spec` when provided to prevent cross-spec contamination.
pub async fn memory_get_all(
    pool: &SqlitePool,
    agent: &str,
    spec: Option<&str>,
) -> Result<Vec<(String, String)>> {
    let rows: Vec<(String, String)> = if let Some(spec) = spec {
        sqlx::query_as(
            "SELECT key, value FROM memory \
             WHERE agent = ? AND spec = ? \
               AND deleted_at IS NULL \
               AND (expires_at IS NULL OR expires_at >= strftime('%Y-%m-%dT%H:%M:%fZ', 'now')) \
             ORDER BY key",
        )
        .bind(agent)
        .bind(spec)
        .fetch_all(pool)
        .await?
    } else {
        sqlx::query_as(
            "SELECT key, value FROM memory \
             WHERE agent = ? \
               AND deleted_at IS NULL \
               AND (expires_at IS NULL OR expires_at >= strftime('%Y-%m-%dT%H:%M:%fZ', 'now')) \
             ORDER BY key",
        )
        .bind(agent)
        .fetch_all(pool)
        .await?
    };

    Ok(rows)
}

/// Full-text search across memory entries using FTS5.
pub async fn memory_search(
    pool: &SqlitePool,
    agent: &str,
    query: &str,
    spec: Option<&str>,
    mem_type: Option<&str>,
    limit: Option<i64>,
) -> Result<Vec<Memory>> {
    let limit = limit.unwrap_or(10);

    // Build SQL dynamically based on optional filters.
    // FTS5 requires the MATCH clause; additional filters are applied on the joined table.
    let rows: Vec<Memory> = if let (Some(spec), Some(mem_type)) = (spec, mem_type) {
        sqlx::query_as(
            "SELECT m.id, m.agent, m.key, m.value, m.spec, m.updated_at, \
                    m.type, m.deleted_at, m.expires_at, \
                    m.access_count, m.last_accessed_at, m.revision_count \
             FROM memory m \
             JOIN memory_fts f ON m.rowid = f.rowid \
             WHERE memory_fts MATCH ? \
               AND m.agent = ? \
               AND m.spec = ? \
               AND m.type = ? \
               AND m.deleted_at IS NULL \
               AND (m.expires_at IS NULL OR m.expires_at >= strftime('%Y-%m-%dT%H:%M:%fZ', 'now')) \
             ORDER BY rank \
             LIMIT ?",
        )
        .bind(query)
        .bind(agent)
        .bind(spec)
        .bind(mem_type)
        .bind(limit)
        .fetch_all(pool)
        .await?
    } else if let Some(spec) = spec {
        sqlx::query_as(
            "SELECT m.id, m.agent, m.key, m.value, m.spec, m.updated_at, \
                    m.type, m.deleted_at, m.expires_at, \
                    m.access_count, m.last_accessed_at, m.revision_count \
             FROM memory m \
             JOIN memory_fts f ON m.rowid = f.rowid \
             WHERE memory_fts MATCH ? \
               AND m.agent = ? \
               AND m.spec = ? \
               AND m.deleted_at IS NULL \
               AND (m.expires_at IS NULL OR m.expires_at >= strftime('%Y-%m-%dT%H:%M:%fZ', 'now')) \
             ORDER BY rank \
             LIMIT ?",
        )
        .bind(query)
        .bind(agent)
        .bind(spec)
        .bind(limit)
        .fetch_all(pool)
        .await?
    } else if let Some(mem_type) = mem_type {
        sqlx::query_as(
            "SELECT m.id, m.agent, m.key, m.value, m.spec, m.updated_at, \
                    m.type, m.deleted_at, m.expires_at, \
                    m.access_count, m.last_accessed_at, m.revision_count \
             FROM memory m \
             JOIN memory_fts f ON m.rowid = f.rowid \
             WHERE memory_fts MATCH ? \
               AND m.agent = ? \
               AND m.type = ? \
               AND m.deleted_at IS NULL \
               AND (m.expires_at IS NULL OR m.expires_at >= strftime('%Y-%m-%dT%H:%M:%fZ', 'now')) \
             ORDER BY rank \
             LIMIT ?",
        )
        .bind(query)
        .bind(agent)
        .bind(mem_type)
        .bind(limit)
        .fetch_all(pool)
        .await?
    } else {
        sqlx::query_as(
            "SELECT m.id, m.agent, m.key, m.value, m.spec, m.updated_at, \
                    m.type, m.deleted_at, m.expires_at, \
                    m.access_count, m.last_accessed_at, m.revision_count \
             FROM memory m \
             JOIN memory_fts f ON m.rowid = f.rowid \
             WHERE memory_fts MATCH ? \
               AND m.agent = ? \
               AND m.deleted_at IS NULL \
               AND (m.expires_at IS NULL OR m.expires_at >= strftime('%Y-%m-%dT%H:%M:%fZ', 'now')) \
             ORDER BY rank \
             LIMIT ?",
        )
        .bind(query)
        .bind(agent)
        .bind(limit)
        .fetch_all(pool)
        .await?
    };

    Ok(rows)
}

/// Soft-delete a memory entry. Returns true if a row was affected.
pub async fn memory_delete(
    pool: &SqlitePool,
    agent: &str,
    key: &str,
    spec: Option<&str>,
) -> Result<bool> {
    let spec = spec.unwrap_or("");
    let result = sqlx::query(
        "UPDATE memory SET deleted_at = strftime('%Y-%m-%dT%H:%M:%fZ', 'now') \
         WHERE agent = ? AND key = ? AND spec = ? AND deleted_at IS NULL",
    )
    .bind(agent)
    .bind(key)
    .bind(spec)
    .execute(pool)
    .await?;

    Ok(result.rows_affected() > 0)
}

/// Returns the most recently accessed memory entries for session recovery.
pub async fn memory_context(
    pool: &SqlitePool,
    agent: &str,
    spec: Option<&str>,
    limit: Option<i64>,
) -> Result<Vec<Memory>> {
    let limit = limit.unwrap_or(10);

    let rows: Vec<Memory> = if let Some(spec) = spec {
        sqlx::query_as(
            "SELECT id, agent, key, value, spec, updated_at, type, deleted_at, expires_at, \
                    access_count, last_accessed_at, revision_count \
             FROM memory \
             WHERE agent = ? AND spec = ? \
               AND deleted_at IS NULL \
               AND (expires_at IS NULL OR expires_at >= strftime('%Y-%m-%dT%H:%M:%fZ', 'now')) \
             ORDER BY last_accessed_at DESC, access_count DESC \
             LIMIT ?",
        )
        .bind(agent)
        .bind(spec)
        .bind(limit)
        .fetch_all(pool)
        .await?
    } else {
        sqlx::query_as(
            "SELECT id, agent, key, value, spec, updated_at, type, deleted_at, expires_at, \
                    access_count, last_accessed_at, revision_count \
             FROM memory \
             WHERE agent = ? \
               AND deleted_at IS NULL \
               AND (expires_at IS NULL OR expires_at >= strftime('%Y-%m-%dT%H:%M:%fZ', 'now')) \
             ORDER BY last_accessed_at DESC, access_count DESC \
             LIMIT ?",
        )
        .bind(agent)
        .bind(limit)
        .fetch_all(pool)
        .await?
    };

    Ok(rows)
}

/// Returns memory statistics for an agent (optionally scoped to a spec).
pub async fn memory_stats(pool: &SqlitePool, agent: &str, spec: Option<&str>) -> Result<Value> {
    let (where_clause_base, spec_bind): (&str, bool) = if spec.is_some() {
        (
            "agent = ? AND spec = ? AND deleted_at IS NULL AND (expires_at IS NULL OR expires_at >= strftime('%Y-%m-%dT%H:%M:%fZ', 'now'))",
            true,
        )
    } else {
        (
            "agent = ? AND deleted_at IS NULL AND (expires_at IS NULL OR expires_at >= strftime('%Y-%m-%dT%H:%M:%fZ', 'now'))",
            false,
        )
    };

    // Total count
    let total: i64 = if spec_bind {
        let row: (i64,) = sqlx::query_as(&format!(
            "SELECT COUNT(*) FROM memory WHERE {where_clause_base}"
        ))
        .bind(agent)
        .bind(spec.unwrap())
        .fetch_one(pool)
        .await?;
        row.0
    } else {
        let row: (i64,) = sqlx::query_as(&format!(
            "SELECT COUNT(*) FROM memory WHERE {where_clause_base}"
        ))
        .bind(agent)
        .fetch_one(pool)
        .await?;
        row.0
    };

    // By type
    let type_rows: Vec<(Option<String>, i64)> = if spec_bind {
        sqlx::query_as(&format!(
            "SELECT type, COUNT(*) as cnt FROM memory WHERE {where_clause_base} GROUP BY type"
        ))
        .bind(agent)
        .bind(spec.unwrap())
        .fetch_all(pool)
        .await?
    } else {
        sqlx::query_as(&format!(
            "SELECT type, COUNT(*) as cnt FROM memory WHERE {where_clause_base} GROUP BY type"
        ))
        .bind(agent)
        .fetch_all(pool)
        .await?
    };

    let mut by_type = serde_json::Map::new();
    for (t, cnt) in type_rows {
        let key = t.unwrap_or_else(|| "untyped".to_string());
        by_type.insert(key, Value::Number(cnt.into()));
    }

    // Most accessed key
    let most_accessed: Option<(String,)> = if spec_bind {
        sqlx::query_as(&format!(
            "SELECT key FROM memory WHERE {where_clause_base} ORDER BY access_count DESC LIMIT 1"
        ))
        .bind(agent)
        .bind(spec.unwrap())
        .fetch_optional(pool)
        .await?
    } else {
        sqlx::query_as(&format!(
            "SELECT key FROM memory WHERE {where_clause_base} ORDER BY access_count DESC LIMIT 1"
        ))
        .bind(agent)
        .fetch_optional(pool)
        .await?
    };

    // Last written at
    let last_written: Option<(String,)> = if spec_bind {
        sqlx::query_as(&format!(
            "SELECT updated_at FROM memory WHERE {where_clause_base} ORDER BY updated_at DESC LIMIT 1"
        ))
        .bind(agent)
        .bind(spec.unwrap())
        .fetch_optional(pool)
        .await?
    } else {
        sqlx::query_as(&format!(
            "SELECT updated_at FROM memory WHERE {where_clause_base} ORDER BY updated_at DESC LIMIT 1"
        ))
        .bind(agent)
        .fetch_optional(pool)
        .await?
    };

    Ok(serde_json::json!({
        "total": total,
        "by_type": by_type,
        "most_accessed_key": most_accessed.map(|(k,)| k),
        "last_written_at": last_written.map(|(t,)| t),
    }))
}

// ─── Integration Tests (MEMS-001, AC1–AC7) ───────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use crate::sdd::test_helpers::make_pool;

    // AC1 — memory_search returns FTS5 results for matching entries only.
    #[tokio::test]
    async fn ac1_search_returns_fts5_results() {
        let pool = make_pool().await;

        memory_set(
            &pool,
            "alice",
            "arch_decision",
            "we use sqlite for persistence",
            None,
            None,
            None,
        )
        .await
        .unwrap();

        memory_set(&pool, "alice", "unrelated", "hello world", None, None, None)
            .await
            .unwrap();

        let results = memory_search(&pool, "alice", "sqlite", None, None, None)
            .await
            .unwrap();

        assert_eq!(results.len(), 1, "expected exactly 1 FTS match");
        assert_eq!(results[0].key, "arch_decision");
    }

    // AC2 — memory_delete soft-deletes; deleted entries are invisible to all read paths.
    #[tokio::test]
    async fn ac2_delete_soft_deletes_and_hides_entry() {
        let pool = make_pool().await;

        memory_set(&pool, "alice", "foo", "bar", None, None, None)
            .await
            .unwrap();

        let deleted = memory_delete(&pool, "alice", "foo", None).await.unwrap();
        assert!(
            deleted,
            "memory_delete should return true when a row is affected"
        );

        let full = memory_get_full(&pool, "alice", "foo", None).await.unwrap();
        assert!(
            full.is_none(),
            "deleted entry must not be visible via memory_get_full"
        );

        let search_results = memory_search(&pool, "alice", "foo", None, None, None)
            .await
            .unwrap();
        assert!(
            search_results.is_empty(),
            "deleted entry must not appear in search results"
        );

        let all = memory_get_all(&pool, "alice", None).await.unwrap();
        assert!(
            !all.iter().any(|(k, _)| k == "foo"),
            "deleted entry must not appear in memory_get_all"
        );
    }

    // AC3 — memory_set accepts a `type` field; memory_get_full returns it.
    #[tokio::test]
    async fn ac3_memory_set_accepts_type_field() {
        let pool = make_pool().await;

        memory_set(
            &pool,
            "alice",
            "my_key",
            "my_value",
            Some("spec"),
            Some("decision"),
            None,
        )
        .await
        .unwrap();

        let mem = memory_get_full(&pool, "alice", "my_key", Some("spec"))
            .await
            .unwrap()
            .expect("entry should exist");

        assert_eq!(
            mem.type_,
            Some("decision".to_string()),
            "type_ field must equal 'decision'"
        );
    }

    // AC4 — memory_set with ttl_seconds; entry is hidden once the TTL expires.
    #[tokio::test]
    async fn ac4_ttl_expires_entry() {
        let pool = make_pool().await;

        memory_set(
            &pool,
            "alice",
            "expiring_key",
            "expiring_value",
            None,
            None,
            Some(1), // 1-second TTL
        )
        .await
        .unwrap();

        // Immediately after insert the entry must be visible.
        let before = memory_get_full(&pool, "alice", "expiring_key", None)
            .await
            .unwrap();
        assert!(before.is_some(), "entry must be visible before TTL expires");

        // Wait for the TTL to elapse.
        tokio::time::sleep(std::time::Duration::from_secs(2)).await;

        // After TTL expiry the entry must be hidden.
        let after = memory_get_full(&pool, "alice", "expiring_key", None)
            .await
            .unwrap();
        assert!(
            after.is_none(),
            "entry must not be visible after TTL has elapsed"
        );
    }

    // AC5 — memory_context returns the most recently accessed entries first.
    #[tokio::test]
    async fn ac5_context_returns_most_recently_accessed() {
        let pool = make_pool().await;

        for key in &["k1", "k2", "k3"] {
            memory_set(&pool, "alice", key, "value", None, None, None)
                .await
                .unwrap();
        }

        // Access "k2" to bump its last_accessed_at to the most recent.
        memory_get_full(&pool, "alice", "k2", None).await.unwrap();

        let ctx = memory_context(&pool, "alice", None, Some(2)).await.unwrap();

        assert_eq!(ctx.len(), 2, "context should return exactly 2 entries");
        assert_eq!(ctx[0].key, "k2", "most recently accessed key must be first");
    }

    // AC6 — memory_stats returns correct total and per-type counts.
    #[tokio::test]
    async fn ac6_stats_returns_correct_counts() {
        let pool = make_pool().await;

        memory_set(&pool, "alice", "k1", "v1", None, Some("decision"), None)
            .await
            .unwrap();
        memory_set(&pool, "alice", "k2", "v2", None, Some("pattern"), None)
            .await
            .unwrap();
        memory_set(&pool, "alice", "k3", "v3", None, None, None)
            .await
            .unwrap();

        let stats = memory_stats(&pool, "alice", None).await.unwrap();

        assert_eq!(stats["total"], 3, "total must equal 3");
        assert_eq!(
            stats["by_type"]["decision"], 1,
            "by_type[decision] must equal 1"
        );
        assert_eq!(
            stats["by_type"]["pattern"], 1,
            "by_type[pattern] must equal 1"
        );
        assert!(
            stats["most_accessed_key"].is_string(),
            "most_accessed_key must be a string"
        );
    }

    // AC7 — memory_get_full bumps access_count on every call.
    #[tokio::test]
    async fn ac7_memory_get_bumps_access_count() {
        let pool = make_pool().await;

        memory_set(&pool, "alice", "tracked", "v", None, None, None)
            .await
            .unwrap();

        // Three successive reads should each increment access_count.
        // Note: memory_get_full returns the row *before* the UPDATE that bumps the counter,
        // so the Nth call returns access_count = N-1.  After 3 calls the DB holds 3;
        // a 4th read returns 3 (the count written by the 3rd call's UPDATE).
        memory_get_full(&pool, "alice", "tracked", None)
            .await
            .unwrap();
        memory_get_full(&pool, "alice", "tracked", None)
            .await
            .unwrap();
        memory_get_full(&pool, "alice", "tracked", None)
            .await
            .unwrap();

        // 4th read — returns the access_count that was committed by call 3 = 3.
        let fourth = memory_get_full(&pool, "alice", "tracked", None)
            .await
            .unwrap()
            .expect("entry must still exist");

        assert_eq!(
            fourth.access_count, 3,
            "access_count must equal 3 after three reads (observed on the 4th read)"
        );
    }
}
