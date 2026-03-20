use anyhow::Result;
use chrono::Utc;
use serde::{Deserialize, Serialize};
use sqlx::SqlitePool;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Artifact {
    pub id: String,
    pub spec: Option<String>,
    pub task: Option<String>,
    pub agent: String,
    pub r#type: String,
    pub path: Option<String>,
    pub description: Option<String>,
    pub created_at: String,
    pub content_hash: Option<String>,
}

#[allow(clippy::too_many_arguments)]
pub async fn register_artifact(
    pool: &SqlitePool,
    id: &str,
    spec: Option<&str>,
    task: Option<&str>,
    agent: &str,
    artifact_type: &str,
    path: Option<&str>,
    description: Option<&str>,
    content_hash: Option<&str>,
) -> Result<Artifact> {
    let now = Utc::now().to_rfc3339();
    sqlx::query(
        "INSERT INTO artifacts (id, spec, task, agent, type, path, description, created_at, content_hash) \
         VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?)",
    )
    .bind(id)
    .bind(spec)
    .bind(task)
    .bind(agent)
    .bind(artifact_type)
    .bind(path)
    .bind(description)
    .bind(&now)
    .bind(content_hash)
    .execute(pool)
    .await?;

    query_artifacts(pool, spec, task, Some(agent), None)
        .await?
        .into_iter()
        .find(|a| a.id == id)
        .ok_or_else(|| anyhow::anyhow!("Failed to retrieve artifact after creation"))
}

pub async fn query_artifacts(
    pool: &SqlitePool,
    spec_filter: Option<&str>,
    task_filter: Option<&str>,
    agent_filter: Option<&str>,
    type_filter: Option<&str>,
) -> Result<Vec<Artifact>> {
    let mut query_str = String::from(
        "SELECT id, spec, task, agent, type, path, description, created_at, content_hash FROM artifacts WHERE 1=1",
    );

    if spec_filter.is_some() {
        query_str.push_str(" AND spec = ?");
    }
    if task_filter.is_some() {
        query_str.push_str(" AND task = ?");
    }
    if agent_filter.is_some() {
        query_str.push_str(" AND agent = ?");
    }
    if type_filter.is_some() {
        query_str.push_str(" AND type = ?");
    }
    query_str.push_str(" ORDER BY created_at DESC");

    let mut q = sqlx::query_as::<
        _,
        (
            String,
            Option<String>,
            Option<String>,
            String,
            String,
            Option<String>,
            Option<String>,
            String,
            Option<String>,
        ),
    >(&query_str);

    if let Some(s) = spec_filter {
        q = q.bind(s);
    }
    if let Some(t) = task_filter {
        q = q.bind(t);
    }
    if let Some(a) = agent_filter {
        q = q.bind(a);
    }
    if let Some(ty) = type_filter {
        q = q.bind(ty);
    }

    let rows = q.fetch_all(pool).await?;

    Ok(rows
        .into_iter()
        .map(
            |(id, spec, task, agent, r#type, path, description, created_at, content_hash)| {
                Artifact {
                    id,
                    spec,
                    task,
                    agent,
                    r#type,
                    path,
                    description,
                    created_at,
                    content_hash,
                }
            },
        )
        .collect())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::sdd::test_helpers::make_pool;

    #[tokio::test]
    async fn test_register_artifact_fields() {
        let pool = make_pool().await;
        let artifact = register_artifact(
            &pool,
            "art-001",
            Some("SPEC-001"),
            Some("task-1"),
            "builder-agent",
            "code",
            Some("/src/lib.rs"),
            Some("main library"),
            None,
        )
        .await
        .unwrap();
        assert_eq!(artifact.id, "art-001");
        assert_eq!(artifact.spec.as_deref(), Some("SPEC-001"));
        assert_eq!(artifact.task.as_deref(), Some("task-1"));
        assert_eq!(artifact.agent, "builder-agent");
        assert_eq!(artifact.r#type, "code");
        assert_eq!(artifact.path.as_deref(), Some("/src/lib.rs"));
        assert_eq!(artifact.description.as_deref(), Some("main library"));
        assert!(!artifact.created_at.is_empty());
    }

    #[tokio::test]
    async fn test_query_artifacts_agent_filter() {
        let pool = make_pool().await;
        register_artifact(
            &pool,
            "a1",
            None,
            None,
            "agent-alpha",
            "doc",
            None,
            None,
            None,
        )
        .await
        .unwrap();
        register_artifact(
            &pool,
            "a2",
            None,
            None,
            "agent-beta",
            "doc",
            None,
            None,
            None,
        )
        .await
        .unwrap();
        let results = query_artifacts(&pool, None, None, Some("agent-alpha"), None)
            .await
            .unwrap();
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].agent, "agent-alpha");
    }

    #[tokio::test]
    async fn test_query_artifacts_no_filters_returns_all() {
        let pool = make_pool().await;
        register_artifact(&pool, "b1", None, None, "agent-x", "code", None, None, None)
            .await
            .unwrap();
        register_artifact(&pool, "b2", None, None, "agent-y", "doc", None, None, None)
            .await
            .unwrap();
        let results = query_artifacts(&pool, None, None, None, None)
            .await
            .unwrap();
        assert_eq!(results.len(), 2);
    }

    #[tokio::test]
    async fn test_query_artifacts_empty_db() {
        let pool = make_pool().await;
        let results = query_artifacts(&pool, None, None, None, None)
            .await
            .unwrap();
        assert!(results.is_empty());
    }

    #[tokio::test]
    async fn test_register_artifact_with_content_hash() {
        let pool = make_pool().await;
        let hash = "sha256:abc123def456";
        let artifact = register_artifact(
            &pool,
            "art-hash",
            None,
            None,
            "builder",
            "code",
            None,
            None,
            Some(hash),
        )
        .await
        .unwrap();

        assert_eq!(artifact.content_hash.as_deref(), Some(hash));
    }

    #[tokio::test]
    async fn test_register_artifact_without_content_hash() {
        let pool = make_pool().await;
        let artifact = register_artifact(
            &pool,
            "art-nohash",
            None,
            None,
            "builder",
            "code",
            None,
            None,
            None,
        )
        .await
        .unwrap();

        assert!(artifact.content_hash.is_none());
    }

    #[tokio::test]
    async fn test_query_artifacts_returns_content_hash() {
        let pool = make_pool().await;
        let hash = "sha256:deadbeef";
        register_artifact(
            &pool,
            "art-q",
            None,
            None,
            "builder",
            "code",
            None,
            None,
            Some(hash),
        )
        .await
        .unwrap();

        let results = query_artifacts(&pool, None, None, Some("builder"), None)
            .await
            .unwrap();

        assert_eq!(results.len(), 1);
        assert_eq!(results[0].content_hash.as_deref(), Some(hash));
    }
}
