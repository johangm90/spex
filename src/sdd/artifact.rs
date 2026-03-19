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
) -> Result<Artifact> {
    let now = Utc::now().to_rfc3339();
    sqlx::query(
        "INSERT INTO artifacts (id, spec, task, agent, type, path, description, created_at) \
         VALUES (?, ?, ?, ?, ?, ?, ?, ?)",
    )
    .bind(id)
    .bind(spec)
    .bind(task)
    .bind(agent)
    .bind(artifact_type)
    .bind(path)
    .bind(description)
    .bind(&now)
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
        "SELECT id, spec, task, agent, type, path, description, created_at FROM artifacts WHERE 1=1",
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
            |(id, spec, task, agent, r#type, path, description, created_at)| Artifact {
                id,
                spec,
                task,
                agent,
                r#type,
                path,
                description,
                created_at,
            },
        )
        .collect())
}
