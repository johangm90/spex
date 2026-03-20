use anyhow::{anyhow, Result};
use chrono::Utc;
use serde::{Deserialize, Serialize};
use sqlx::SqlitePool;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub enum SpecStatus {
    Draft,
    Approved,
    InProgress,
    Done,
    Paused,
}

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
    let result = sqlx::query(
        "UPDATE specs SET status = ?, updated_at = ?, updated_by = ? WHERE id = ? AND status = ?",
    )
    .bind(new_status)
    .bind(&now)
    .bind(updated_by)
    .bind(id)
    .bind(&spec.status)
    .execute(pool)
    .await?;

    if result.rows_affected() == 0 {
        return Err(anyhow!(
            "Spec '{}' status changed concurrently (expected '{}', no longer matches)",
            id,
            spec.status
        ));
    }

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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::sdd::test_helpers::make_pool;

    #[tokio::test]
    async fn create_spec_returns_draft_with_correct_fields() {
        let pool = make_pool().await;
        let spec = create_spec(&pool, "SPEC-001", "My Feature", "P0", &[])
            .await
            .unwrap();

        assert_eq!(spec.id, "SPEC-001", "id must match the given id");
        assert_eq!(spec.title, "My Feature", "title must match the given title");
        assert_eq!(spec.status, "draft", "new spec must have status 'draft'");
        assert_eq!(spec.priority, "P0", "priority must match the given priority");
        assert_eq!(spec.ac_total, 0, "ac_total must start at 0");
        assert_eq!(spec.ac_passed, 0, "ac_passed must start at 0");
        assert_eq!(spec.agents, "[]", "agents must start as empty JSON array");
    }

    #[tokio::test]
    async fn create_spec_with_depends_on_stores_json() {
        let pool = make_pool().await;
        let deps = vec!["SPEC-000".to_string()];
        let spec = create_spec(&pool, "SPEC-002", "Dependent Feature", "P1", &deps)
            .await
            .unwrap();

        let stored: Vec<String> =
            serde_json::from_str(&spec.depends_on).expect("depends_on must be valid JSON");
        assert_eq!(stored, deps, "depends_on must contain the given dependency");
    }

    #[tokio::test]
    async fn get_spec_existing_returns_some() {
        let pool = make_pool().await;
        create_spec(&pool, "SPEC-003", "Exists", "P2", &[])
            .await
            .unwrap();

        let result = get_spec(&pool, "SPEC-003").await.unwrap();
        assert!(
            result.is_some(),
            "get_spec must return Some for an existing id"
        );
        assert_eq!(result.unwrap().id, "SPEC-003");
    }

    #[tokio::test]
    async fn get_spec_nonexistent_returns_none() {
        let pool = make_pool().await;
        let result = get_spec(&pool, "DOES-NOT-EXIST").await.unwrap();
        assert!(
            result.is_none(),
            "get_spec must return None for an unknown id"
        );
    }

    #[tokio::test]
    async fn list_specs_empty_db_returns_empty_vec() {
        let pool = make_pool().await;
        let specs = list_specs(&pool).await.unwrap();
        assert!(
            specs.is_empty(),
            "list_specs must return empty vec on an empty DB"
        );
    }

    #[tokio::test]
    async fn list_specs_after_two_creates_returns_both_ordered_by_id() {
        let pool = make_pool().await;
        create_spec(&pool, "SPEC-B", "Beta", "P1", &[])
            .await
            .unwrap();
        create_spec(&pool, "SPEC-A", "Alpha", "P0", &[])
            .await
            .unwrap();

        let specs = list_specs(&pool).await.unwrap();
        assert_eq!(specs.len(), 2, "list_specs must return 2 specs");
        assert_eq!(
            specs[0].id, "SPEC-A",
            "first result must be SPEC-A (ORDER BY id)"
        );
        assert_eq!(
            specs[1].id, "SPEC-B",
            "second result must be SPEC-B (ORDER BY id)"
        );
    }

    #[tokio::test]
    async fn transition_draft_to_approved_is_valid() {
        validate_transition("draft", "approved").expect("draft→approved must be valid");
    }

    #[tokio::test]
    async fn transition_approved_to_in_progress_is_valid() {
        validate_transition("approved", "in_progress")
            .expect("approved→in_progress must be valid");
    }

    #[tokio::test]
    async fn transition_in_progress_to_done_is_valid() {
        validate_transition("in_progress", "done").expect("in_progress→done must be valid");
    }

    #[tokio::test]
    async fn transition_in_progress_to_paused_is_valid() {
        validate_transition("in_progress", "paused").expect("in_progress→paused must be valid");
    }

    #[tokio::test]
    async fn transition_paused_to_in_progress_is_valid() {
        validate_transition("paused", "in_progress").expect("paused→in_progress must be valid");
    }

    #[tokio::test]
    async fn transition_draft_to_done_is_invalid() {
        assert!(
            validate_transition("draft", "done").is_err(),
            "draft→done must be an invalid transition"
        );
    }

    #[tokio::test]
    async fn transition_draft_to_in_progress_is_invalid() {
        assert!(
            validate_transition("draft", "in_progress").is_err(),
            "draft→in_progress must be an invalid transition"
        );
    }

    #[tokio::test]
    async fn transition_done_to_draft_is_invalid() {
        assert!(
            validate_transition("done", "draft").is_err(),
            "done→draft must be an invalid transition"
        );
    }

    #[tokio::test]
    async fn transition_approved_to_done_is_invalid() {
        assert!(
            validate_transition("approved", "done").is_err(),
            "approved→done must be an invalid transition"
        );
    }

    #[tokio::test]
    async fn transition_paused_to_done_is_invalid() {
        assert!(
            validate_transition("paused", "done").is_err(),
            "paused→done must be an invalid transition"
        );
    }

    #[tokio::test]
    async fn transition_done_to_in_progress_is_invalid() {
        assert!(
            validate_transition("done", "in_progress").is_err(),
            "done→in_progress must be an invalid transition"
        );
    }

    #[tokio::test]
    async fn update_spec_status_valid_transition_changes_status() {
        let pool = make_pool().await;
        create_spec(&pool, "SPEC-010", "State Test", "P0", &[])
            .await
            .unwrap();

        let updated = update_spec_status(&pool, "SPEC-010", "approved", "agent-x")
            .await
            .unwrap();

        assert_eq!(
            updated.status, "approved",
            "status must be 'approved' after valid transition"
        );
        assert_eq!(
            updated.updated_by,
            Some("agent-x".to_string()),
            "updated_by must be set to the given agent"
        );
    }

    #[tokio::test]
    async fn update_spec_status_nonexistent_spec_returns_error() {
        let pool = make_pool().await;
        let result = update_spec_status(&pool, "NO-SUCH-SPEC", "approved", "agent-x").await;
        assert!(
            result.is_err(),
            "update_spec_status must return an error for a nonexistent spec"
        );
    }

    #[tokio::test]
    async fn update_spec_status_invalid_transition_returns_error() {
        let pool = make_pool().await;
        create_spec(&pool, "SPEC-011", "Bad Transition", "P1", &[])
            .await
            .unwrap();

        let result = update_spec_status(&pool, "SPEC-011", "done", "agent-x").await;
        assert!(
            result.is_err(),
            "update_spec_status must return an error for an invalid transition"
        );
    }

    #[tokio::test]
    async fn update_spec_ac_stores_correct_values() {
        let pool = make_pool().await;
        create_spec(&pool, "SPEC-020", "AC Test", "P0", &[])
            .await
            .unwrap();

        let updated = update_spec_ac(&pool, "SPEC-020", 5, 3).await.unwrap();

        assert_eq!(updated.ac_total, 5, "ac_total must equal 5 after update");
        assert_eq!(updated.ac_passed, 3, "ac_passed must equal 3 after update");
    }

    #[tokio::test]
    async fn update_spec_agents_stores_json_array() {
        let pool = make_pool().await;
        create_spec(&pool, "SPEC-030", "Agents Test", "P2", &[])
            .await
            .unwrap();

        let agents = vec!["builder".to_string(), "tester".to_string()];
        let updated = update_spec_agents(&pool, "SPEC-030", &agents)
            .await
            .unwrap();

        let stored: Vec<String> =
            serde_json::from_str(&updated.agents).expect("agents must be valid JSON");
        assert_eq!(stored, agents, "agents must match the given list");
    }
}
