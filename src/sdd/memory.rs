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
    pub related_to: String,
}

#[allow(clippy::too_many_arguments)]
pub async fn memory_set(
    pool: &SqlitePool,
    agent: &str,
    key: &str,
    value_json: &str,
    spec: Option<&str>,
    mem_type: Option<&str>,
    ttl_seconds: Option<i64>,
    related_to: Option<&str>,
) -> Result<()> {
    let spec = spec.unwrap_or("");
    let related_to = related_to.unwrap_or("[]");

    let expires_at: Option<String> = ttl_seconds.map(|ttl| {
        let future = chrono::Utc::now() + chrono::Duration::seconds(ttl);
        future.format("%Y-%m-%dT%H:%M:%S%.3fZ").to_string()
    });

    sqlx::query(
        "INSERT INTO memory (agent, key, value, spec, type, expires_at, revision_count, related_to) \
         VALUES (?, ?, ?, ?, ?, ?, 1, ?) \
         ON CONFLICT(agent, spec, key) DO UPDATE SET \
           value = excluded.value, \
           updated_at = strftime('%Y-%m-%dT%H:%M:%fZ', 'now'), \
           revision_count = revision_count + 1, \
           type = COALESCE(excluded.type, type), \
           expires_at = COALESCE(excluded.expires_at, expires_at), \
           related_to = excluded.related_to, \
           deleted_at = NULL, \
           access_count = 0, \
           last_accessed_at = NULL",
    )
    .bind(agent)
    .bind(key)
    .bind(value_json)
    .bind(spec)
    .bind(mem_type)
    .bind(expires_at)
    .bind(related_to)
    .execute(pool)
    .await?;
    Ok(())
}

/// Retrieve a full Memory row, filtering deleted and expired entries.
/// On a hit, bumps access_count and last_accessed_at.
pub async fn memory_get_full(
    pool: &SqlitePool,
    agent: &str,
    key: &str,
    spec: Option<&str>,
) -> Result<Option<Memory>> {
    let mut tx = pool.begin().await?;

    let mut qb = sqlx::QueryBuilder::<sqlx::Sqlite>::new(
        "SELECT id, agent, key, value, spec, updated_at, type, deleted_at, expires_at, \
                access_count, last_accessed_at, revision_count, related_to \
         FROM memory WHERE agent = ",
    );
    qb.push_bind(agent);
    qb.push(" AND key = ");
    qb.push_bind(key);
    if let Some(s) = spec {
        qb.push(" AND (spec = ");
        qb.push_bind(s);
        qb.push(" OR spec = '*')");
    }
    qb.push(
        " AND deleted_at IS NULL \
         AND (expires_at IS NULL OR expires_at >= strftime('%Y-%m-%dT%H:%M:%fZ', 'now'))",
    );
    if spec.is_none() {
        qb.push(" ORDER BY updated_at DESC LIMIT 1");
    }

    let row: Option<Memory> = qb.build_query_as().fetch_optional(&mut *tx).await?;

    if let Some(ref m) = row {
        sqlx::query(
            "UPDATE memory SET \
               access_count = access_count + 1, \
               last_accessed_at = strftime('%Y-%m-%dT%H:%M:%fZ', 'now') \
             WHERE id = ?",
        )
        .bind(m.id)
        .execute(&mut *tx)
        .await?;
    }

    tx.commit().await?;

    Ok(row)
}

pub async fn memory_list(
    pool: &SqlitePool,
    agent: &str,
    spec: Option<&str>,
    mem_type: Option<&str>,
    limit: Option<i64>,
    offset: Option<i64>,
) -> Result<Vec<Memory>> {
    let limit = limit.unwrap_or(100);
    let offset = offset.unwrap_or(0);

    let mut qb = sqlx::QueryBuilder::<sqlx::Sqlite>::new(
        "SELECT id, agent, key, value, spec, updated_at, type, deleted_at, expires_at, \
                access_count, last_accessed_at, revision_count, related_to \
         FROM memory WHERE agent = ",
    );
    qb.push_bind(agent);
    if let Some(s) = spec {
        qb.push(" AND (spec = ");
        qb.push_bind(s);
        qb.push(" OR spec = '*')");
    }
    if let Some(t) = mem_type {
        qb.push(" AND type = ");
        qb.push_bind(t);
    }
    qb.push(
        " AND deleted_at IS NULL \
           AND (expires_at IS NULL OR expires_at >= strftime('%Y-%m-%dT%H:%M:%fZ', 'now')) \
         ORDER BY key LIMIT ",
    );
    qb.push_bind(limit);
    qb.push(" OFFSET ");
    qb.push_bind(offset);
    let rows: Vec<Memory> = qb.build_query_as::<Memory>().fetch_all(pool).await?;

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

    let mut qb = sqlx::QueryBuilder::<sqlx::Sqlite>::new(
        "SELECT m.id, m.agent, m.key, m.value, m.spec, m.updated_at, \
                m.type, m.deleted_at, m.expires_at, \
                m.access_count, m.last_accessed_at, m.revision_count, m.related_to \
         FROM memory m \
         JOIN memory_fts f ON m.rowid = f.rowid \
         WHERE memory_fts MATCH ",
    );
    qb.push_bind(query);
    qb.push(" AND m.agent = ");
    qb.push_bind(agent);
    if let Some(s) = spec {
        qb.push(" AND (m.spec = ");
        qb.push_bind(s);
        qb.push(" OR m.spec = '*')");
    }
    if let Some(t) = mem_type {
        qb.push(" AND m.type = ");
        qb.push_bind(t);
    }
    qb.push(
        " AND m.deleted_at IS NULL \
           AND (m.expires_at IS NULL OR m.expires_at >= strftime('%Y-%m-%dT%H:%M:%fZ', 'now')) \
         ORDER BY rank \
         LIMIT ",
    );
    qb.push_bind(limit);
    let rows: Vec<Memory> = qb.build_query_as::<Memory>().fetch_all(pool).await?;

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

    let mut qb = sqlx::QueryBuilder::<sqlx::Sqlite>::new(
        "SELECT id, agent, key, value, spec, updated_at, type, deleted_at, expires_at, \
                access_count, last_accessed_at, revision_count, related_to \
         FROM memory WHERE agent = ",
    );
    qb.push_bind(agent);
    if let Some(s) = spec {
        qb.push(" AND (spec = ");
        qb.push_bind(s);
        qb.push(" OR spec = '*')");
    }
    qb.push(
        " AND deleted_at IS NULL \
         AND (expires_at IS NULL OR expires_at >= strftime('%Y-%m-%dT%H:%M:%fZ', 'now')) \
         ORDER BY (CAST(access_count AS REAL) + 1.0) / \
           (julianday('now') - julianday(COALESCE(last_accessed_at, updated_at)) + 0.01) DESC \
         LIMIT ",
    );
    qb.push_bind(limit);

    let rows: Vec<Memory> = qb.build_query_as().fetch_all(pool).await?;
    Ok(rows)
}

/// Returns memory statistics for an agent (optionally scoped to a spec).
pub async fn memory_stats(pool: &SqlitePool, agent: &str, spec: Option<&str>) -> Result<Value> {
    let alive_filter = "deleted_at IS NULL AND (expires_at IS NULL OR expires_at >= strftime('%Y-%m-%dT%H:%M:%fZ', 'now'))";

    macro_rules! stats_query {
        ($qb:ident, $select:expr, $suffix:expr) => {{
            let mut $qb = sqlx::QueryBuilder::<sqlx::Sqlite>::new(format!(
                "{} FROM memory WHERE agent = ",
                $select
            ));
            $qb.push_bind(agent);
            if let Some(s) = spec {
                $qb.push(" AND (spec = ");
                $qb.push_bind(s);
                $qb.push(" OR spec = '*')");
            }
            $qb.push(format!(" AND {} {}", alive_filter, $suffix));
            $qb
        }};
    }

    let total: i64 = {
        let mut qb = stats_query!(qb, "SELECT COUNT(*)", "");
        let row: (i64,) = qb.build_query_as().fetch_one(pool).await?;
        row.0
    };

    let type_rows: Vec<(Option<String>, i64)> = {
        let mut qb = stats_query!(qb, "SELECT type, COUNT(*) as cnt", "GROUP BY type");
        qb.build_query_as().fetch_all(pool).await?
    };

    let mut by_type = serde_json::Map::new();
    for (t, cnt) in type_rows {
        let key = t.unwrap_or_else(|| "untyped".to_string());
        by_type.insert(key, Value::Number(cnt.into()));
    }

    let most_accessed: Option<(String,)> = {
        let mut qb = stats_query!(qb, "SELECT key", "ORDER BY access_count DESC LIMIT 1");
        qb.build_query_as().fetch_optional(pool).await?
    };

    let last_written: Option<(String,)> = {
        let mut qb = stats_query!(qb, "SELECT updated_at", "ORDER BY updated_at DESC LIMIT 1");
        qb.build_query_as().fetch_optional(pool).await?
    };

    Ok(serde_json::json!({
        "total": total,
        "by_type": by_type,
        "most_accessed_key": most_accessed.map(|(k,)| k),
        "last_written_at": last_written.map(|(t,)| t),
    }))
}

pub async fn memory_find_referencing(
    pool: &SqlitePool,
    target: &str,
    spec: Option<&str>,
) -> Result<Vec<Memory>> {
    let mut qb = sqlx::QueryBuilder::<sqlx::Sqlite>::new(
        "SELECT m.id, m.agent, m.key, m.value, m.spec, m.updated_at, m.type, \
                m.deleted_at, m.expires_at, m.access_count, m.last_accessed_at, \
                m.revision_count, m.related_to \
         FROM memory m JOIN json_each(m.related_to) j ON j.type = 'text' \
         WHERE j.value = ",
    );
    qb.push_bind(target);
    qb.push(
        " AND m.deleted_at IS NULL \
         AND (m.expires_at IS NULL OR m.expires_at >= strftime('%Y-%m-%dT%H:%M:%fZ', 'now'))",
    );
    if let Some(s) = spec {
        qb.push(" AND (m.spec = ");
        qb.push_bind(s);
        qb.push(" OR m.spec = '*')");
    }
    qb.push(" ORDER BY m.updated_at DESC");

    let rows: Vec<Memory> = qb.build_query_as().fetch_all(pool).await?;
    Ok(rows)
}

#[derive(Debug, Serialize)]
pub struct GcResult {
    pub deleted_count: u64,
    pub expired_count: u64,
    pub sample_keys: Vec<String>,
}

pub async fn memory_gc(pool: &SqlitePool, dry_run: bool) -> Result<GcResult> {
    let soft_deleted: Vec<(String, String, String)> =
        sqlx::query_as("SELECT agent, key, spec FROM memory WHERE deleted_at IS NOT NULL")
            .fetch_all(pool)
            .await?;

    let expired: Vec<(String, String, String)> = sqlx::query_as(
        "SELECT agent, key, spec FROM memory \
         WHERE expires_at IS NOT NULL \
           AND expires_at < strftime('%Y-%m-%dT%H:%M:%fZ', 'now') \
           AND deleted_at IS NULL",
    )
    .fetch_all(pool)
    .await?;

    let deleted_count = soft_deleted.len() as u64;
    let expired_count = expired.len() as u64;

    let purged_ids: Vec<String> = soft_deleted
        .iter()
        .chain(expired.iter())
        .map(|(agent, key, _spec)| format!("{}/{}", agent, key))
        .collect();

    let sample_keys: Vec<String> = purged_ids.iter().take(10).cloned().collect();

    if !dry_run && (deleted_count > 0 || expired_count > 0) {
        sqlx::query("DELETE FROM memory WHERE deleted_at IS NOT NULL")
            .execute(pool)
            .await?;

        sqlx::query(
            "DELETE FROM memory \
             WHERE expires_at IS NOT NULL \
               AND expires_at < strftime('%Y-%m-%dT%H:%M:%fZ', 'now')",
        )
        .execute(pool)
        .await?;

        gc_clean_orphan_refs(pool, &purged_ids).await?;

        sqlx::query("INSERT INTO memory_fts(memory_fts) VALUES ('rebuild')")
            .execute(pool)
            .await?;
    }

    Ok(GcResult {
        deleted_count,
        expired_count,
        sample_keys,
    })
}

async fn gc_clean_orphan_refs(pool: &SqlitePool, purged_ids: &[String]) -> Result<()> {
    if purged_ids.is_empty() {
        return Ok(());
    }

    let mut qb = sqlx::QueryBuilder::<sqlx::Sqlite>::new(
        "SELECT DISTINCT m.id, m.related_to \
         FROM memory m JOIN json_each(m.related_to) j ON j.type = 'text' \
         WHERE j.value IN (",
    );
    let mut sep = qb.separated(", ");
    for id in purged_ids {
        sep.push_bind(id);
    }
    sep.push_unseparated(") AND m.deleted_at IS NULL");

    let affected: Vec<(i64, String)> = qb.build_query_as().fetch_all(pool).await?;

    for (row_id, related_json) in &affected {
        let arr: Vec<String> = serde_json::from_str(related_json).unwrap_or_default();
        let cleaned: Vec<&String> = arr.iter().filter(|r| !purged_ids.contains(r)).collect();
        let new_json = serde_json::to_string(&cleaned)?;

        sqlx::query("UPDATE memory SET related_to = ? WHERE id = ?")
            .bind(&new_json)
            .bind(row_id)
            .execute(pool)
            .await?;
    }

    Ok(())
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
            None,
        )
        .await
        .unwrap();

        memory_set(
            &pool,
            "alice",
            "unrelated",
            "hello world",
            None,
            None,
            None,
            None,
        )
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

        memory_set(&pool, "alice", "foo", "bar", None, None, None, None)
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

        let all = memory_list(&pool, "alice", None, None, None, None)
            .await
            .unwrap();
        assert!(
            !all.iter().any(|m| m.key == "foo"),
            "deleted entry must not appear in memory_list"
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
            Some(1),
            None,
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
            memory_set(&pool, "alice", key, "value", None, None, None, None)
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

        memory_set(
            &pool,
            "alice",
            "k1",
            "v1",
            None,
            Some("decision"),
            None,
            None,
        )
        .await
        .unwrap();
        memory_set(
            &pool,
            "alice",
            "k2",
            "v2",
            None,
            Some("pattern"),
            None,
            None,
        )
        .await
        .unwrap();
        memory_set(&pool, "alice", "k3", "v3", None, None, None, None)
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

        memory_set(&pool, "alice", "tracked", "v", None, None, None, None)
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

    #[tokio::test]
    async fn memory_get_full_transactional_bump_sets_last_accessed() {
        let pool = make_pool().await;

        memory_set(&pool, "alice", "txn-test", "v", None, None, None, None)
            .await
            .unwrap();

        let before = memory_get_full(&pool, "alice", "txn-test", None)
            .await
            .unwrap()
            .expect("entry must exist");
        assert_eq!(before.access_count, 0, "first read returns pre-bump count");

        let after = memory_get_full(&pool, "alice", "txn-test", None)
            .await
            .unwrap()
            .expect("entry must exist");
        assert_eq!(after.access_count, 1, "second read sees bump from first");
        assert!(
            after.last_accessed_at.is_some(),
            "last_accessed_at must be set after transactional bump"
        );
    }

    #[tokio::test]
    async fn ac8_set_after_delete_resurrects_entry() {
        let pool = make_pool().await;

        memory_set(
            &pool,
            "alice",
            "ephemeral",
            "v1",
            None,
            Some("decision"),
            None,
            None,
        )
        .await
        .unwrap();

        let deleted = memory_delete(&pool, "alice", "ephemeral", None)
            .await
            .unwrap();
        assert!(deleted);

        let gone = memory_get_full(&pool, "alice", "ephemeral", None)
            .await
            .unwrap();
        assert!(gone.is_none(), "soft-deleted entry must be invisible");

        memory_set(
            &pool,
            "alice",
            "ephemeral",
            "v2",
            None,
            Some("pattern"),
            None,
            None,
        )
        .await
        .unwrap();

        let resurrected = memory_get_full(&pool, "alice", "ephemeral", None)
            .await
            .unwrap()
            .expect("re-set entry must be visible again");

        assert_eq!(resurrected.value, "v2");
        assert_eq!(resurrected.type_.as_deref(), Some("pattern"));
        assert_eq!(
            resurrected.access_count, 0,
            "access_count must reset on resurrect"
        );
        assert!(
            resurrected.deleted_at.is_none(),
            "deleted_at must be cleared"
        );
    }

    #[tokio::test]
    async fn gc_removes_soft_deleted() {
        let pool = make_pool().await;

        memory_set(&pool, "alice", "keep", "v", None, None, None, None)
            .await
            .unwrap();
        memory_set(&pool, "alice", "trash", "v", None, None, None, None)
            .await
            .unwrap();
        memory_delete(&pool, "alice", "trash", None).await.unwrap();

        let result = memory_gc(&pool, false).await.unwrap();
        assert_eq!(result.deleted_count, 1);
        assert_eq!(result.expired_count, 0);
        assert!(result.sample_keys.contains(&"alice/trash".to_string()));

        let row: (i64,) =
            sqlx::query_as("SELECT COUNT(*) FROM memory WHERE agent = 'alice' AND key = 'trash'")
                .fetch_one(&pool)
                .await
                .unwrap();
        assert_eq!(row.0, 0, "hard-deleted row must be gone");

        let kept = memory_get_full(&pool, "alice", "keep", None).await.unwrap();
        assert!(kept.is_some(), "non-deleted entry must survive GC");
    }

    #[tokio::test]
    async fn gc_removes_expired() {
        let pool = make_pool().await;

        memory_set(&pool, "alice", "fresh", "v", None, None, None, None)
            .await
            .unwrap();
        memory_set(&pool, "alice", "stale", "v", None, None, Some(9999), None)
            .await
            .unwrap();

        sqlx::query(
            "UPDATE memory SET expires_at = '2000-01-01T00:00:00.000Z' \
             WHERE agent = 'alice' AND key = 'stale'",
        )
        .execute(&pool)
        .await
        .unwrap();

        let result = memory_gc(&pool, false).await.unwrap();
        assert_eq!(result.expired_count, 1);
        assert!(result.sample_keys.contains(&"alice/stale".to_string()));

        let row: (i64,) =
            sqlx::query_as("SELECT COUNT(*) FROM memory WHERE agent = 'alice' AND key = 'stale'")
                .fetch_one(&pool)
                .await
                .unwrap();
        assert_eq!(row.0, 0, "expired row must be gone");
    }

    #[tokio::test]
    async fn gc_dry_run_preserves_rows() {
        let pool = make_pool().await;

        memory_set(&pool, "alice", "trash", "v", None, None, None, None)
            .await
            .unwrap();
        memory_delete(&pool, "alice", "trash", None).await.unwrap();

        let result = memory_gc(&pool, true).await.unwrap();
        assert_eq!(result.deleted_count, 1);

        let row: (i64,) =
            sqlx::query_as("SELECT COUNT(*) FROM memory WHERE agent = 'alice' AND key = 'trash'")
                .fetch_one(&pool)
                .await
                .unwrap();
        assert_eq!(row.0, 1, "dry-run must not remove rows");
    }

    #[tokio::test]
    async fn gc_preserves_fts_consistency() {
        let pool = make_pool().await;

        memory_set(
            &pool,
            "alice",
            "survives",
            "important data",
            None,
            None,
            None,
            None,
        )
        .await
        .unwrap();
        memory_set(
            &pool,
            "alice",
            "dies",
            "doomed data",
            None,
            None,
            None,
            None,
        )
        .await
        .unwrap();
        memory_delete(&pool, "alice", "dies", None).await.unwrap();

        memory_gc(&pool, false).await.unwrap();

        let results = memory_search(&pool, "alice", "important", None, None, None)
            .await
            .unwrap();
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].key, "survives");

        let ghost = memory_search(&pool, "alice", "doomed", None, None, None)
            .await
            .unwrap();
        assert!(
            ghost.is_empty(),
            "GC'd entry must not appear in FTS results"
        );
    }

    #[tokio::test]
    async fn memory_set_with_related_to_stores_links() {
        let pool = make_pool().await;

        let links = r#"["agent1/key-a","agent1/key-b"]"#;
        memory_set(&pool, "alice", "linked", "v", None, None, None, Some(links))
            .await
            .unwrap();

        let mem = memory_get_full(&pool, "alice", "linked", None)
            .await
            .unwrap()
            .expect("entry should exist");

        assert_eq!(mem.related_to, links);
    }

    #[tokio::test]
    async fn memory_set_without_related_to_defaults_to_empty_array() {
        let pool = make_pool().await;

        memory_set(&pool, "alice", "solo", "v", None, None, None, None)
            .await
            .unwrap();

        let mem = memory_get_full(&pool, "alice", "solo", None)
            .await
            .unwrap()
            .expect("entry should exist");

        assert_eq!(mem.related_to, "[]");
    }

    #[tokio::test]
    async fn memory_search_includes_related_to() {
        let pool = make_pool().await;

        let links = r#"["alice/other"]"#;
        memory_set(
            &pool,
            "alice",
            "searchable",
            "findme data",
            None,
            None,
            None,
            Some(links),
        )
        .await
        .unwrap();

        let results = memory_search(&pool, "alice", "findme", None, None, None)
            .await
            .unwrap();

        assert_eq!(results.len(), 1);
        assert_eq!(results[0].related_to, links);
    }

    #[tokio::test]
    async fn cross_spec_global_entry_visible_when_querying_specific_spec() {
        let pool = make_pool().await;

        memory_set(
            &pool,
            "alice",
            "local",
            "local-val",
            Some("SPEC-001"),
            None,
            None,
            None,
        )
        .await
        .unwrap();
        memory_set(
            &pool,
            "alice",
            "global",
            "global-val",
            Some("*"),
            None,
            None,
            None,
        )
        .await
        .unwrap();
        memory_set(
            &pool,
            "alice",
            "other",
            "other-val",
            Some("SPEC-002"),
            None,
            None,
            None,
        )
        .await
        .unwrap();

        let results = memory_list(&pool, "alice", Some("SPEC-001"), None, None, None)
            .await
            .unwrap();

        let keys: Vec<&str> = results.iter().map(|m| m.key.as_str()).collect();
        assert!(keys.contains(&"local"), "spec-local entry must appear");
        assert!(keys.contains(&"global"), "global (*) entry must appear");
        assert!(!keys.contains(&"other"), "other-spec entry must not appear");
    }

    #[tokio::test]
    async fn cross_spec_global_entry_visible_in_search() {
        let pool = make_pool().await;

        memory_set(
            &pool,
            "alice",
            "spec-data",
            "shared pattern",
            Some("SPEC-001"),
            None,
            None,
            None,
        )
        .await
        .unwrap();
        memory_set(
            &pool,
            "alice",
            "global-data",
            "shared pattern",
            Some("*"),
            None,
            None,
            None,
        )
        .await
        .unwrap();

        let results = memory_search(&pool, "alice", "shared", Some("SPEC-001"), None, None)
            .await
            .unwrap();

        let keys: Vec<&str> = results.iter().map(|m| m.key.as_str()).collect();
        assert!(keys.contains(&"spec-data"));
        assert!(keys.contains(&"global-data"));
    }

    #[tokio::test]
    async fn cross_spec_global_entry_visible_in_context() {
        let pool = make_pool().await;

        memory_set(
            &pool,
            "alice",
            "ctx-local",
            "v",
            Some("SPEC-001"),
            None,
            None,
            None,
        )
        .await
        .unwrap();
        memory_set(
            &pool,
            "alice",
            "ctx-global",
            "v",
            Some("*"),
            None,
            None,
            None,
        )
        .await
        .unwrap();

        memory_get_full(&pool, "alice", "ctx-local", Some("SPEC-001"))
            .await
            .unwrap();
        memory_get_full(&pool, "alice", "ctx-global", Some("*"))
            .await
            .unwrap();

        let results = memory_context(&pool, "alice", Some("SPEC-001"), None)
            .await
            .unwrap();

        let keys: Vec<&str> = results.iter().map(|m| m.key.as_str()).collect();
        assert!(keys.contains(&"ctx-local"));
        assert!(keys.contains(&"ctx-global"));
    }

    #[tokio::test]
    async fn cross_spec_stats_includes_global_entries() {
        let pool = make_pool().await;

        memory_set(
            &pool,
            "alice",
            "s1",
            "v",
            Some("SPEC-001"),
            None,
            None,
            None,
        )
        .await
        .unwrap();
        memory_set(&pool, "alice", "g1", "v", Some("*"), None, None, None)
            .await
            .unwrap();

        let stats = memory_stats(&pool, "alice", Some("SPEC-001"))
            .await
            .unwrap();
        assert_eq!(
            stats["total"], 2,
            "stats must count both spec-local and global entries"
        );
    }

    #[tokio::test]
    async fn context_frecency_fresh_write_appears_without_access() {
        let pool = make_pool().await;

        memory_set(&pool, "alice", "old", "v", None, None, None, None)
            .await
            .unwrap();
        // Access "old" many times to inflate its access_count.
        for _ in 0..5 {
            memory_get_full(&pool, "alice", "old", None).await.unwrap();
        }

        // Tiny sleep to ensure "fresh" has a later updated_at.
        tokio::time::sleep(std::time::Duration::from_millis(50)).await;

        memory_set(&pool, "alice", "fresh", "v", None, None, None, None)
            .await
            .unwrap();
        // "fresh" has access_count=0, last_accessed_at=NULL — frecency uses updated_at fallback.

        let ctx = memory_context(&pool, "alice", None, Some(2)).await.unwrap();
        assert_eq!(ctx.len(), 2);
        // Fresh entry should appear in results (not be excluded by NULL last_accessed_at).
        let keys: Vec<&str> = ctx.iter().map(|m| m.key.as_str()).collect();
        assert!(
            keys.contains(&"fresh"),
            "fresh write must appear in context despite NULL last_accessed_at, got: {:?}",
            keys
        );
    }

    #[tokio::test]
    async fn context_frecency_recent_beats_stale_high_count() {
        let pool = make_pool().await;

        memory_set(&pool, "alice", "stale", "v", None, None, None, None)
            .await
            .unwrap();
        // Give "stale" a high access_count but old timestamps.
        for _ in 0..10 {
            memory_get_full(&pool, "alice", "stale", None)
                .await
                .unwrap();
        }
        // Backdate stale's last_accessed_at to make it old.
        sqlx::query(
            "UPDATE memory SET last_accessed_at = '2020-01-01T00:00:00.000Z', \
                               updated_at = '2020-01-01T00:00:00.000Z' \
             WHERE agent = 'alice' AND key = 'stale'",
        )
        .execute(&pool)
        .await
        .unwrap();

        memory_set(&pool, "alice", "recent", "v", None, None, None, None)
            .await
            .unwrap();
        memory_get_full(&pool, "alice", "recent", None)
            .await
            .unwrap();

        let ctx = memory_context(&pool, "alice", None, Some(1)).await.unwrap();
        assert_eq!(ctx.len(), 1);
        assert_eq!(
            ctx[0].key, "recent",
            "recent entry with 1 access must beat stale entry with 10 accesses from years ago"
        );
    }

    #[tokio::test]
    async fn find_referencing_returns_entries_that_link_to_target() {
        let pool = make_pool().await;

        memory_set(
            &pool,
            "alice",
            "parent",
            "v",
            None,
            None,
            None,
            Some(r#"["bob/child"]"#),
        )
        .await
        .unwrap();
        memory_set(&pool, "bob", "child", "v", None, None, None, None)
            .await
            .unwrap();
        memory_set(
            &pool,
            "carol",
            "other",
            "v",
            None,
            None,
            None,
            Some(r#"["bob/child","alice/parent"]"#),
        )
        .await
        .unwrap();

        let refs = memory_find_referencing(&pool, "bob/child", None)
            .await
            .unwrap();
        let keys: Vec<&str> = refs.iter().map(|m| m.key.as_str()).collect();
        assert!(
            keys.contains(&"parent"),
            "alice/parent references bob/child"
        );
        assert!(keys.contains(&"other"), "carol/other references bob/child");
        assert_eq!(refs.len(), 2);
    }

    #[tokio::test]
    async fn find_referencing_excludes_deleted_entries() {
        let pool = make_pool().await;

        memory_set(
            &pool,
            "alice",
            "linker",
            "v",
            None,
            None,
            None,
            Some(r#"["bob/target"]"#),
        )
        .await
        .unwrap();
        memory_delete(&pool, "alice", "linker", None).await.unwrap();

        let refs = memory_find_referencing(&pool, "bob/target", None)
            .await
            .unwrap();
        assert!(refs.is_empty(), "deleted entries must be excluded");
    }

    #[tokio::test]
    async fn find_referencing_respects_spec_filter() {
        let pool = make_pool().await;

        memory_set(
            &pool,
            "alice",
            "s1-link",
            "v",
            Some("SPEC-001"),
            None,
            None,
            Some(r#"["bob/target"]"#),
        )
        .await
        .unwrap();
        memory_set(
            &pool,
            "alice",
            "s2-link",
            "v",
            Some("SPEC-002"),
            None,
            None,
            Some(r#"["bob/target"]"#),
        )
        .await
        .unwrap();

        let refs = memory_find_referencing(&pool, "bob/target", Some("SPEC-001"))
            .await
            .unwrap();
        assert_eq!(refs.len(), 1);
        assert_eq!(refs[0].key, "s1-link");
    }

    #[tokio::test]
    async fn gc_cleans_orphan_related_to_refs() {
        let pool = make_pool().await;

        memory_set(
            &pool,
            "alice",
            "parent",
            "v",
            None,
            None,
            None,
            Some(r#"["bob/child","carol/keeper"]"#),
        )
        .await
        .unwrap();
        memory_set(&pool, "bob", "child", "v", None, None, None, None)
            .await
            .unwrap();
        memory_set(&pool, "carol", "keeper", "v", None, None, None, None)
            .await
            .unwrap();

        memory_delete(&pool, "bob", "child", None).await.unwrap();

        let result = memory_gc(&pool, false).await.unwrap();
        assert_eq!(result.deleted_count, 1);

        let parent = memory_get_full(&pool, "alice", "parent", None)
            .await
            .unwrap()
            .expect("parent must survive");
        let refs: Vec<String> = serde_json::from_str(&parent.related_to).unwrap();
        assert_eq!(
            refs,
            vec!["carol/keeper"],
            "orphan ref bob/child must be removed"
        );
    }
}
