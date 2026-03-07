use anyhow::{anyhow, Result};
use chrono::Utc;
use serde::{Deserialize, Serialize};
use sqlx::SqlitePool;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ReplanRequest {
    pub id: String,
    pub spec_id: String,
    pub task_id: Option<String>,
    pub agent_id: String,
    pub reason: String,
    pub impact: String,
    pub proposed_action: Option<String>,
    pub status: String,
    pub created_at: String,
    pub updated_at: String,
}

pub async fn create_replan_request(
    pool: &SqlitePool,
    id: &str,
    spec_id: &str,
    task_id: Option<&str>,
    agent_id: &str,
    reason: &str,
    impact: &[String],
    proposed_action: Option<&str>,
) -> Result<ReplanRequest> {
    let now = Utc::now().to_rfc3339();
    let impact_json = serde_json::to_string(impact)?;
    sqlx::query(
        "INSERT INTO replan_requests (id, spec_id, task_id, agent_id, reason, impact, proposed_action, status, created_at, updated_at) VALUES (?, ?, ?, ?, ?, ?, ?, 'open', ?, ?)"
    )
    .bind(id)
    .bind(spec_id)
    .bind(task_id)
    .bind(agent_id)
    .bind(reason)
    .bind(&impact_json)
    .bind(proposed_action)
    .bind(&now)
    .bind(&now)
    .execute(pool)
    .await?;
    get_replan_request(pool, id)
        .await?
        .ok_or_else(|| anyhow!("Replan request '{}' not found", id))
}

pub async fn get_replan_request(pool: &SqlitePool, id: &str) -> Result<Option<ReplanRequest>> {
    let row = sqlx::query_as::<_, (String, String, Option<String>, String, String, String, Option<String>, String, String, String)>(
        "SELECT id, spec_id, task_id, agent_id, reason, impact, proposed_action, status, created_at, updated_at FROM replan_requests WHERE id = ?"
    )
    .bind(id)
    .fetch_optional(pool)
    .await?;
    Ok(row.map(
        |(
            id,
            spec_id,
            task_id,
            agent_id,
            reason,
            impact,
            proposed_action,
            status,
            created_at,
            updated_at,
        )| ReplanRequest {
            id,
            spec_id,
            task_id,
            agent_id,
            reason,
            impact,
            proposed_action,
            status,
            created_at,
            updated_at,
        },
    ))
}

pub async fn list_replan_requests(
    pool: &SqlitePool,
    spec_filter: Option<&str>,
    status_filter: Option<&str>,
) -> Result<Vec<ReplanRequest>> {
    let mut query = String::from("SELECT id, spec_id, task_id, agent_id, reason, impact, proposed_action, status, created_at, updated_at FROM replan_requests WHERE 1=1");
    if spec_filter.is_some() {
        query.push_str(" AND spec_id = ?");
    }
    if status_filter.is_some() {
        query.push_str(" AND status = ?");
    }
    query.push_str(" ORDER BY created_at DESC");
    let mut q = sqlx::query_as::<
        _,
        (
            String,
            String,
            Option<String>,
            String,
            String,
            String,
            Option<String>,
            String,
            String,
            String,
        ),
    >(&query);
    if let Some(spec) = spec_filter {
        q = q.bind(spec);
    }
    if let Some(status) = status_filter {
        q = q.bind(status);
    }
    let rows = q.fetch_all(pool).await?;
    Ok(rows
        .into_iter()
        .map(
            |(
                id,
                spec_id,
                task_id,
                agent_id,
                reason,
                impact,
                proposed_action,
                status,
                created_at,
                updated_at,
            )| ReplanRequest {
                id,
                spec_id,
                task_id,
                agent_id,
                reason,
                impact,
                proposed_action,
                status,
                created_at,
                updated_at,
            },
        )
        .collect())
}

pub async fn update_replan_request(
    pool: &SqlitePool,
    id: &str,
    status: &str,
) -> Result<ReplanRequest> {
    let now = Utc::now().to_rfc3339();
    sqlx::query("UPDATE replan_requests SET status = ?, updated_at = ? WHERE id = ?")
        .bind(status)
        .bind(&now)
        .bind(id)
        .execute(pool)
        .await?;
    get_replan_request(pool, id)
        .await?
        .ok_or_else(|| anyhow!("Replan request '{}' not found", id))
}
