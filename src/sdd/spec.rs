use anyhow::{anyhow, Result};
use chrono::Utc;
use serde::{Deserialize, Serialize};
use sqlx::SqlitePool;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[allow(dead_code)]
pub enum SpecStatus {
    Draft,
    Approved,
    InProgress,
    Done,
    Paused,
}

#[allow(dead_code)]
impl SpecStatus {
    pub fn from_str(s: &str) -> Option<Self> {
        match s {
            "draft" => Some(Self::Draft),
            "approved" => Some(Self::Approved),
            "in_progress" => Some(Self::InProgress),
            "done" => Some(Self::Done),
            "paused" => Some(Self::Paused),
            _ => None,
        }
    }

    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Draft => "draft",
            Self::Approved => "approved",
            Self::InProgress => "in_progress",
            Self::Done => "done",
            Self::Paused => "paused",
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Spec {
    pub id: String,
    pub title: String,
    pub status: String,
    pub priority: String,
    pub depends_on: String, // JSON
    pub agents: String,     // JSON
    pub ac_total: i64,
    pub ac_passed: i64,
    pub created_at: String,
    pub updated_at: String,
    pub updated_by: Option<String>,
}

pub async fn create_spec(
    pool: &SqlitePool,
    id: &str,
    title: &str,
    priority: &str,
    depends_on: &[String],
) -> Result<Spec> {
    let now = Utc::now().to_rfc3339();
    let depends_json = serde_json::to_string(depends_on)?;

    sqlx::query(
        "INSERT INTO specs (id, title, status, priority, depends_on, agents, ac_total, ac_passed, created_at, updated_at) \
         VALUES (?, ?, 'draft', ?, ?, '[]', 0, 0, ?, ?)",
    )
    .bind(id)
    .bind(title)
    .bind(priority)
    .bind(&depends_json)
    .bind(&now)
    .bind(&now)
    .execute(pool)
    .await?;

    get_spec(pool, id)
        .await?
        .ok_or_else(|| anyhow!("Failed to create spec"))
}

pub async fn get_spec(pool: &SqlitePool, id: &str) -> Result<Option<Spec>> {
    let row = sqlx::query_as::<_, (String, String, String, String, String, String, i64, i64, String, String, Option<String>)>(
        "SELECT id, title, status, priority, depends_on, agents, ac_total, ac_passed, created_at, updated_at, updated_by \
         FROM specs WHERE id = ?",
    )
    .bind(id)
    .fetch_optional(pool)
    .await?;

    Ok(row.map(
        |(
            id,
            title,
            status,
            priority,
            depends_on,
            agents,
            ac_total,
            ac_passed,
            created_at,
            updated_at,
            updated_by,
        )| {
            Spec {
                id,
                title,
                status,
                priority,
                depends_on,
                agents,
                ac_total,
                ac_passed,
                created_at,
                updated_at,
                updated_by,
            }
        },
    ))
}

pub async fn list_specs(pool: &SqlitePool) -> Result<Vec<Spec>> {
    let rows = sqlx::query_as::<_, (String, String, String, String, String, String, i64, i64, String, String, Option<String>)>(
        "SELECT id, title, status, priority, depends_on, agents, ac_total, ac_passed, created_at, updated_at, updated_by \
         FROM specs ORDER BY id",
    )
    .fetch_all(pool)
    .await?;

    Ok(rows
        .into_iter()
        .map(
            |(
                id,
                title,
                status,
                priority,
                depends_on,
                agents,
                ac_total,
                ac_passed,
                created_at,
                updated_at,
                updated_by,
            )| {
                Spec {
                    id,
                    title,
                    status,
                    priority,
                    depends_on,
                    agents,
                    ac_total,
                    ac_passed,
                    created_at,
                    updated_at,
                    updated_by,
                }
            },
        )
        .collect())
}

/// Validate state machine transition.
fn validate_transition(from: &str, to: &str) -> Result<()> {
    let valid = matches!(
        (from, to),
        ("draft", "approved")
            | ("approved", "in_progress")
            | ("in_progress", "done")
            | ("in_progress", "paused")
            | ("paused", "in_progress")
    );
    if !valid {
        return Err(anyhow!("Invalid transition: {} -> {}", from, to));
    }
    Ok(())
}

pub async fn update_spec_status(
    pool: &SqlitePool,
    id: &str,
    new_status: &str,
    updated_by: &str,
) -> Result<Spec> {
    let spec = get_spec(pool, id)
        .await?
        .ok_or_else(|| anyhow!("Spec '{}' not found", id))?;

    validate_transition(&spec.status, new_status)?;

    let now = Utc::now().to_rfc3339();
    sqlx::query("UPDATE specs SET status = ?, updated_at = ?, updated_by = ? WHERE id = ?")
        .bind(new_status)
        .bind(&now)
        .bind(updated_by)
        .bind(id)
        .execute(pool)
        .await?;

    get_spec(pool, id)
        .await?
        .ok_or_else(|| anyhow!("Spec '{}' not found after update", id))
}

pub async fn update_spec_ac(
    pool: &SqlitePool,
    id: &str,
    ac_total: i64,
    ac_passed: i64,
) -> Result<Spec> {
    let now = Utc::now().to_rfc3339();
    sqlx::query("UPDATE specs SET ac_total = ?, ac_passed = ?, updated_at = ? WHERE id = ?")
        .bind(ac_total)
        .bind(ac_passed)
        .bind(&now)
        .bind(id)
        .execute(pool)
        .await?;

    get_spec(pool, id)
        .await?
        .ok_or_else(|| anyhow!("Spec '{}' not found", id))
}

pub async fn update_spec_agents(pool: &SqlitePool, id: &str, agents: &[String]) -> Result<Spec> {
    let now = Utc::now().to_rfc3339();
    let agents_json = serde_json::to_string(agents)?;
    sqlx::query("UPDATE specs SET agents = ?, updated_at = ? WHERE id = ?")
        .bind(&agents_json)
        .bind(&now)
        .bind(id)
        .execute(pool)
        .await?;

    get_spec(pool, id)
        .await?
        .ok_or_else(|| anyhow!("Spec '{}' not found", id))
}
