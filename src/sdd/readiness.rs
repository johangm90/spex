use anyhow::Result;
use chrono::Utc;
use serde::{Deserialize, Serialize};
use serde_json::json;
use sqlx::SqlitePool;

// ---------------------------------------------------------------------------
// Enums
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum WorkflowPhaseKind {
    Planning,
    InProgress,
    Review,
    Done,
}

impl WorkflowPhaseKind {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Planning => "planning",
            Self::InProgress => "in_progress",
            Self::Review => "review",
            Self::Done => "done",
        }
    }

    pub fn from_str(s: &str) -> Option<Self> {
        match s {
            "planning" => Some(Self::Planning),
            "in_progress" => Some(Self::InProgress),
            "review" => Some(Self::Review),
            "done" => Some(Self::Done),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ReviewRequirementKind {
    TestPass,
    LintPass,
    ReviewApproved,
    Custom,
}

impl ReviewRequirementKind {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::TestPass => "test_pass",
            Self::LintPass => "lint_pass",
            Self::ReviewApproved => "review_approved",
            Self::Custom => "custom",
        }
    }

    pub fn from_str(s: &str) -> Option<Self> {
        match s {
            "test_pass" => Some(Self::TestPass),
            "lint_pass" => Some(Self::LintPass),
            "review_approved" => Some(Self::ReviewApproved),
            "custom" => Some(Self::Custom),
            _ => None,
        }
    }
}

// ---------------------------------------------------------------------------
// Domain structs
// ---------------------------------------------------------------------------

#[derive(Debug, Clone)]
pub struct WorkflowPhase {
    pub id: String,
    pub spec_id: String,
    pub phase: String,
    pub entered_at: String,
    #[allow(dead_code)]
    pub exited_at: Option<String>,
    pub entered_by: Option<String>,
    pub notes: Option<String>,
    #[allow(dead_code)]
    pub created_at: String,
}

#[derive(Debug, Clone)]
pub struct ReviewRequirement {
    pub id: String,
    pub spec_id: String,
    pub kind: String,
    pub description: String,
    pub satisfied: bool,
    pub satisfied_at: Option<String>,
    pub satisfied_by: Option<String>,
    pub created_at: String,
}

#[derive(Debug, Clone)]
pub struct SessionCheckpoint {
    pub id: String,
    pub session_id: String,
    pub spec_id: Option<String>,
    pub task_id: Option<String>,
    pub agent: String,
    pub checkpoint_data: String,
    pub saved_at: String,
    pub label: Option<String>,
    #[allow(dead_code)]
    pub created_at: String,
}

// ---------------------------------------------------------------------------
// Row tuple types for sqlx fetch
// ---------------------------------------------------------------------------

type WorkflowPhaseRow = (
    String,
    String,
    String,
    String,
    Option<String>,
    Option<String>,
    Option<String>,
    String,
);

fn row_to_phase(r: WorkflowPhaseRow) -> WorkflowPhase {
    WorkflowPhase {
        id: r.0,
        spec_id: r.1,
        phase: r.2,
        entered_at: r.3,
        exited_at: r.4,
        entered_by: r.5,
        notes: r.6,
        created_at: r.7,
    }
}

type ReviewRequirementRow = (
    String,
    String,
    String,
    String,
    i64,
    Option<String>,
    Option<String>,
    String,
);

fn row_to_requirement(r: ReviewRequirementRow) -> ReviewRequirement {
    ReviewRequirement {
        id: r.0,
        spec_id: r.1,
        kind: r.2,
        description: r.3,
        satisfied: r.4 != 0,
        satisfied_at: r.5,
        satisfied_by: r.6,
        created_at: r.7,
    }
}

type SessionCheckpointRow = (
    String,
    String,
    Option<String>,
    Option<String>,
    String,
    String,
    String,
    Option<String>,
    String,
);

fn row_to_checkpoint(r: SessionCheckpointRow) -> SessionCheckpoint {
    SessionCheckpoint {
        id: r.0,
        session_id: r.1,
        spec_id: r.2,
        task_id: r.3,
        agent: r.4,
        checkpoint_data: r.5,
        saved_at: r.6,
        label: r.7,
        created_at: r.8,
    }
}

// ---------------------------------------------------------------------------
// Workflow phase helpers
// ---------------------------------------------------------------------------

pub async fn insert_workflow_phase(
    pool: &SqlitePool,
    id: &str,
    spec_id: &str,
    phase: WorkflowPhaseKind,
    entered_by: Option<&str>,
    notes: Option<&str>,
) -> Result<WorkflowPhase> {
    let now = Utc::now().to_rfc3339();
    let phase_str = phase.as_str();
    sqlx::query(
        "INSERT INTO workflow_phases (id, spec_id, phase, entered_at, entered_by, notes, created_at)
         VALUES (?, ?, ?, ?, ?, ?, ?)",
    )
    .bind(id)
    .bind(spec_id)
    .bind(phase_str)
    .bind(&now)
    .bind(entered_by)
    .bind(notes)
    .bind(&now)
    .execute(pool)
    .await?;

    let row: WorkflowPhaseRow = sqlx::query_as(
        "SELECT id, spec_id, phase, entered_at, exited_at, entered_by, notes, created_at
         FROM workflow_phases WHERE id = ?",
    )
    .bind(id)
    .fetch_one(pool)
    .await?;

    Ok(row_to_phase(row))
}

pub async fn close_workflow_phase(
    pool: &SqlitePool,
    phase_id: &str,
    exited_at: &str,
) -> Result<()> {
    sqlx::query("UPDATE workflow_phases SET exited_at = ? WHERE id = ?")
        .bind(exited_at)
        .bind(phase_id)
        .execute(pool)
        .await?;
    Ok(())
}

pub async fn get_current_phase(pool: &SqlitePool, spec_id: &str) -> Result<Option<WorkflowPhase>> {
    let row: Option<WorkflowPhaseRow> = sqlx::query_as(
        "SELECT id, spec_id, phase, entered_at, exited_at, entered_by, notes, created_at
         FROM workflow_phases
         WHERE spec_id = ? AND exited_at IS NULL
         ORDER BY entered_at DESC
         LIMIT 1",
    )
    .bind(spec_id)
    .fetch_optional(pool)
    .await?;

    Ok(row.map(row_to_phase))
}

#[allow(dead_code)]
pub async fn list_phases(pool: &SqlitePool, spec_id: &str) -> Result<Vec<WorkflowPhase>> {
    let rows: Vec<WorkflowPhaseRow> = sqlx::query_as(
        "SELECT id, spec_id, phase, entered_at, exited_at, entered_by, notes, created_at
         FROM workflow_phases
         WHERE spec_id = ?
         ORDER BY entered_at ASC",
    )
    .bind(spec_id)
    .fetch_all(pool)
    .await?;

    Ok(rows.into_iter().map(row_to_phase).collect())
}

// ---------------------------------------------------------------------------
// Review requirement helpers
// ---------------------------------------------------------------------------

pub async fn insert_review_requirement(
    pool: &SqlitePool,
    id: &str,
    spec_id: &str,
    kind: ReviewRequirementKind,
    description: &str,
) -> Result<ReviewRequirement> {
    let now = Utc::now().to_rfc3339();
    let kind_str = kind.as_str();
    sqlx::query(
        "INSERT INTO review_requirements (id, spec_id, kind, description, satisfied, created_at)
         VALUES (?, ?, ?, ?, 0, ?)",
    )
    .bind(id)
    .bind(spec_id)
    .bind(kind_str)
    .bind(description)
    .bind(&now)
    .execute(pool)
    .await?;

    let row: ReviewRequirementRow = sqlx::query_as(
        "SELECT id, spec_id, kind, description, satisfied, satisfied_at, satisfied_by, created_at
         FROM review_requirements WHERE id = ?",
    )
    .bind(id)
    .fetch_one(pool)
    .await?;

    Ok(row_to_requirement(row))
}

pub async fn satisfy_review_requirement(
    pool: &SqlitePool,
    req_id: &str,
    satisfied_by: Option<&str>,
) -> Result<()> {
    let now = Utc::now().to_rfc3339();
    sqlx::query(
        "UPDATE review_requirements SET satisfied = 1, satisfied_at = ?, satisfied_by = ? WHERE id = ?",
    )
    .bind(&now)
    .bind(satisfied_by)
    .bind(req_id)
    .execute(pool)
    .await?;
    Ok(())
}

pub async fn list_review_requirements(
    pool: &SqlitePool,
    spec_id: &str,
) -> Result<Vec<ReviewRequirement>> {
    let rows: Vec<ReviewRequirementRow> = sqlx::query_as(
        "SELECT id, spec_id, kind, description, satisfied, satisfied_at, satisfied_by, created_at
         FROM review_requirements
         WHERE spec_id = ?
         ORDER BY created_at ASC",
    )
    .bind(spec_id)
    .fetch_all(pool)
    .await?;

    Ok(rows.into_iter().map(row_to_requirement).collect())
}

pub async fn unsatisfied_requirements(
    pool: &SqlitePool,
    spec_id: &str,
) -> Result<Vec<ReviewRequirement>> {
    let rows: Vec<ReviewRequirementRow> = sqlx::query_as(
        "SELECT id, spec_id, kind, description, satisfied, satisfied_at, satisfied_by, created_at
         FROM review_requirements
         WHERE spec_id = ? AND satisfied = 0
         ORDER BY created_at ASC",
    )
    .bind(spec_id)
    .fetch_all(pool)
    .await?;

    Ok(rows.into_iter().map(row_to_requirement).collect())
}

// ---------------------------------------------------------------------------
// Session checkpoint helpers
// ---------------------------------------------------------------------------

#[allow(clippy::too_many_arguments)]
pub async fn save_checkpoint(
    pool: &SqlitePool,
    id: &str,
    session_id: &str,
    spec_id: Option<&str>,
    task_id: Option<&str>,
    agent: &str,
    data_json: &str,
    label: Option<&str>,
) -> Result<SessionCheckpoint> {
    let now = Utc::now().to_rfc3339();
    sqlx::query(
        "INSERT INTO session_checkpoints
         (id, session_id, spec_id, task_id, agent, checkpoint_data, saved_at, label, created_at)
         VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?)",
    )
    .bind(id)
    .bind(session_id)
    .bind(spec_id)
    .bind(task_id)
    .bind(agent)
    .bind(data_json)
    .bind(&now)
    .bind(label)
    .bind(&now)
    .execute(pool)
    .await?;

    let row: SessionCheckpointRow = sqlx::query_as(
        "SELECT id, session_id, spec_id, task_id, agent, checkpoint_data, saved_at, label, created_at
         FROM session_checkpoints WHERE id = ?",
    )
    .bind(id)
    .fetch_one(pool)
    .await?;

    Ok(row_to_checkpoint(row))
}

pub async fn get_latest_checkpoint(
    pool: &SqlitePool,
    session_id: &str,
) -> Result<Option<SessionCheckpoint>> {
    let row: Option<SessionCheckpointRow> = sqlx::query_as(
        "SELECT id, session_id, spec_id, task_id, agent, checkpoint_data, saved_at, label, created_at
         FROM session_checkpoints
         WHERE session_id = ?
         ORDER BY saved_at DESC
         LIMIT 1",
    )
    .bind(session_id)
    .fetch_optional(pool)
    .await?;

    Ok(row.map(row_to_checkpoint))
}

pub async fn get_checkpoint_by_id(
    pool: &SqlitePool,
    id: &str,
    session_id: &str,
) -> Result<Option<SessionCheckpoint>> {
    let row: Option<SessionCheckpointRow> = sqlx::query_as(
        "SELECT id, session_id, spec_id, task_id, agent, checkpoint_data, saved_at, label, created_at
         FROM session_checkpoints WHERE id = ? AND session_id = ?",
    )
    .bind(id)
    .bind(session_id)
    .fetch_optional(pool)
    .await?;

    Ok(row.map(row_to_checkpoint))
}

pub async fn list_checkpoints(
    pool: &SqlitePool,
    session_id: &str,
) -> Result<Vec<SessionCheckpoint>> {
    let rows: Vec<SessionCheckpointRow> = sqlx::query_as(
        "SELECT id, session_id, spec_id, task_id, agent, checkpoint_data, saved_at, label, created_at
         FROM session_checkpoints
         WHERE session_id = ?
         ORDER BY saved_at ASC",
    )
    .bind(session_id)
    .fetch_all(pool)
    .await?;

    Ok(rows.into_iter().map(row_to_checkpoint).collect())
}

// ---------------------------------------------------------------------------
// Phase transition service
// ---------------------------------------------------------------------------

/// Close the current open phase (if any) and open a new one, emitting a domain event.
pub async fn transition_phase(
    pool: &SqlitePool,
    spec_id: &str,
    new_phase: WorkflowPhaseKind,
    entered_by: Option<&str>,
    notes: Option<&str>,
) -> Result<WorkflowPhase> {
    let now = Utc::now().to_rfc3339();

    // Close the current open phase, if any.
    let from_phase: Option<String> = if let Some(current) = get_current_phase(pool, spec_id).await?
    {
        close_workflow_phase(pool, &current.id, &now).await?;
        Some(current.phase)
    } else {
        None
    };

    // Insert the new phase.
    let new_id = format!("phase-{}", Utc::now().timestamp_nanos_opt().unwrap_or(0));
    let phase =
        insert_workflow_phase(pool, &new_id, spec_id, new_phase.clone(), entered_by, notes).await?;

    // Emit domain event.
    let payload = json!({
        "spec_id": spec_id,
        "from_phase": from_phase,
        "to_phase": new_phase.as_str(),
        "entered_by": entered_by,
    });
    crate::sdd::event::emit_event(
        pool,
        "WorkflowPhaseTransitioned",
        Some(spec_id),
        entered_by,
        &payload.to_string(),
    )
    .await?;

    Ok(phase)
}

// ---------------------------------------------------------------------------
// Review lifecycle services
// ---------------------------------------------------------------------------

/// Enter review phase: transitions spec to 'review' and seeds default requirements if none exist.
pub async fn enter_review(
    pool: &SqlitePool,
    spec_id: &str,
    agent: Option<&str>,
) -> Result<WorkflowPhase> {
    let phase = transition_phase(pool, spec_id, WorkflowPhaseKind::Review, agent, None).await?;

    if list_review_requirements(pool, spec_id).await?.is_empty() {
        let defaults = [
            (ReviewRequirementKind::TestPass, "All tests pass"),
            (ReviewRequirementKind::LintPass, "Lint/clippy clean"),
            (
                ReviewRequirementKind::ReviewApproved,
                "Human review approved",
            ),
        ];
        for (kind, desc) in defaults {
            let id = format!(
                "rreq-{}-{}",
                kind.as_str(),
                Utc::now().timestamp_nanos_opt().unwrap_or(0)
            );
            insert_review_requirement(pool, &id, spec_id, kind, desc).await?;
        }
    }

    Ok(phase)
}

/// Returns true iff all review requirements for the spec are satisfied.
pub async fn review_complete(pool: &SqlitePool, spec_id: &str) -> Result<bool> {
    Ok(unsatisfied_requirements(pool, spec_id).await?.is_empty())
}

/// Satisfy the ReviewApproved requirement; if all requirements are then met, transition to Done.
/// Returns true if the spec transitioned to Done.
pub async fn approve_review(pool: &SqlitePool, spec_id: &str, approved_by: &str) -> Result<bool> {
    // Find the unsatisfied ReviewApproved requirement for this spec, if any.
    let unsatisfied = unsatisfied_requirements(pool, spec_id).await?;
    if let Some(req) = unsatisfied
        .iter()
        .find(|r| r.kind == ReviewRequirementKind::ReviewApproved.as_str())
    {
        satisfy_review_requirement(pool, &req.id, Some(approved_by)).await?;
    }

    if review_complete(pool, spec_id).await? {
        transition_phase(
            pool,
            spec_id,
            WorkflowPhaseKind::Done,
            Some(approved_by),
            None,
        )
        .await?;
        return Ok(true);
    }

    Ok(false)
}

// ---------------------------------------------------------------------------
// Readiness report structs
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ReadinessBlocker {
    pub kind: String, // "unsatisfied_requirement" | "tasks_incomplete" | "ac_gap" | "no_phase"
    pub description: String,
    pub spec_id: Option<String>,
    pub task_id: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SpecReadinessReport {
    pub spec_id: String,
    pub current_phase: Option<String>,
    pub review_requirements_total: usize,
    pub review_requirements_satisfied: usize,
    pub blockers: Vec<ReadinessBlocker>,
    pub ready: bool, // true iff blockers is empty
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OperatorReadinessReport {
    pub specs: Vec<SpecReadinessReport>,
    pub total_specs: usize,
    pub ready_specs: usize,
    pub blocked_specs: usize,
}

// ---------------------------------------------------------------------------
// Readiness synthesis
// ---------------------------------------------------------------------------

/// Synthesise a readiness report for a single spec.
pub async fn spec_readiness(pool: &SqlitePool, spec_id: &str) -> Result<SpecReadinessReport> {
    // 1. Current phase
    let current_phase = get_current_phase(pool, spec_id).await?.map(|p| p.phase);

    // 2. All review requirements
    let all_reqs = list_review_requirements(pool, spec_id).await?;
    let review_requirements_total = all_reqs.len();
    let review_requirements_satisfied = all_reqs.iter().filter(|r| r.satisfied).count();

    // 3. Unsatisfied requirements
    let unsatisfied = unsatisfied_requirements(pool, spec_id).await?;

    // 4. Tasks: count incomplete (pending or in_progress)
    let incomplete_tasks: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM tasks WHERE spec = ? AND status NOT IN ('done', 'failed')",
    )
    .bind(spec_id)
    .fetch_one(pool)
    .await?;

    // 5. AC counts from specs table
    let ac_row: Option<(i64, i64)> =
        sqlx::query_as("SELECT ac_total, ac_passed FROM specs WHERE id = ?")
            .bind(spec_id)
            .fetch_optional(pool)
            .await?;
    let (ac_total, ac_passed) = ac_row.unwrap_or((0, 0));

    // 6. Build blockers
    let mut blockers: Vec<ReadinessBlocker> = Vec::new();

    for req in &unsatisfied {
        blockers.push(ReadinessBlocker {
            kind: "unsatisfied_requirement".to_string(),
            description: req.description.clone(),
            spec_id: Some(spec_id.to_string()),
            task_id: None,
        });
    }

    if incomplete_tasks > 0 {
        blockers.push(ReadinessBlocker {
            kind: "tasks_incomplete".to_string(),
            description: format!("{} task(s) not done", incomplete_tasks),
            spec_id: Some(spec_id.to_string()),
            task_id: None,
        });
    }

    if ac_total > 0 && ac_passed < ac_total {
        blockers.push(ReadinessBlocker {
            kind: "ac_gap".to_string(),
            description: format!("{}/{} ACs passed", ac_passed, ac_total),
            spec_id: Some(spec_id.to_string()),
            task_id: None,
        });
    }

    let ready = blockers.is_empty();

    Ok(SpecReadinessReport {
        spec_id: spec_id.to_string(),
        current_phase,
        review_requirements_total,
        review_requirements_satisfied,
        blockers,
        ready,
    })
}

/// Synthesise a readiness report across all specs (operator view).
pub async fn operator_readiness(pool: &SqlitePool) -> Result<OperatorReadinessReport> {
    let spec_ids: Vec<(String,)> = sqlx::query_as("SELECT id FROM specs ORDER BY created_at ASC")
        .fetch_all(pool)
        .await?;

    let mut specs = Vec::with_capacity(spec_ids.len());
    for (id,) in &spec_ids {
        specs.push(spec_readiness(pool, id).await?);
    }

    let total_specs = specs.len();
    let ready_specs = specs.iter().filter(|s| s.ready).count();
    let blocked_specs = total_specs - ready_specs;

    Ok(OperatorReadinessReport {
        specs,
        total_specs,
        ready_specs,
        blocked_specs,
    })
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use crate::sdd::test_helpers::open_test_db;

    // Helper: create a minimal spec row so FK constraints are satisfied.
    async fn seed_spec(pool: &SqlitePool, spec_id: &str) {
        let now = Utc::now().to_rfc3339();
        sqlx::query(
            "INSERT OR IGNORE INTO specs (id, title, status, priority, created_at, updated_at)
             VALUES (?, 'Test Spec', 'draft', 'P1', ?, ?)",
        )
        .bind(spec_id)
        .bind(&now)
        .bind(&now)
        .execute(pool)
        .await
        .unwrap();
    }

    // Helper: create a minimal session row so FK constraints are satisfied.
    async fn seed_session(pool: &SqlitePool, session_id: &str) {
        let now = Utc::now().to_rfc3339();
        sqlx::query(
            "INSERT OR IGNORE INTO sessions (id, agent, started_at, created_at)
             VALUES (?, 'test-agent', ?, ?)",
        )
        .bind(session_id)
        .bind(&now)
        .bind(&now)
        .execute(pool)
        .await
        .unwrap();
    }

    // ------------------------------------------------------------------
    // WorkflowPhase tests
    // ------------------------------------------------------------------

    #[tokio::test]
    async fn test_insert_and_retrieve_workflow_phase() {
        let pool = open_test_db().await;
        seed_spec(&pool, "spec-wp-1").await;

        let phase = insert_workflow_phase(
            &pool,
            "phase-1",
            "spec-wp-1",
            WorkflowPhaseKind::Planning,
            Some("agent-x"),
            Some("starting planning"),
        )
        .await
        .unwrap();

        assert_eq!(phase.id, "phase-1");
        assert_eq!(phase.spec_id, "spec-wp-1");
        assert_eq!(phase.phase, "planning");
        assert_eq!(phase.entered_by.as_deref(), Some("agent-x"));
        assert_eq!(phase.notes.as_deref(), Some("starting planning"));
        assert!(phase.exited_at.is_none());
    }

    #[tokio::test]
    async fn test_close_workflow_phase_sets_exited_at() {
        let pool = open_test_db().await;
        seed_spec(&pool, "spec-wp-2").await;

        insert_workflow_phase(
            &pool,
            "phase-2",
            "spec-wp-2",
            WorkflowPhaseKind::InProgress,
            None,
            None,
        )
        .await
        .unwrap();

        let exit_time = Utc::now().to_rfc3339();
        close_workflow_phase(&pool, "phase-2", &exit_time)
            .await
            .unwrap();

        let phases = list_phases(&pool, "spec-wp-2").await.unwrap();
        assert_eq!(phases.len(), 1);
        assert_eq!(phases[0].exited_at.as_deref(), Some(exit_time.as_str()));
    }

    #[tokio::test]
    async fn test_get_current_phase_returns_none_after_close() {
        let pool = open_test_db().await;
        seed_spec(&pool, "spec-wp-3").await;

        insert_workflow_phase(
            &pool,
            "phase-3",
            "spec-wp-3",
            WorkflowPhaseKind::Review,
            None,
            None,
        )
        .await
        .unwrap();

        // Before closing — should return the phase.
        let current = get_current_phase(&pool, "spec-wp-3").await.unwrap();
        assert!(current.is_some());

        let exit_time = Utc::now().to_rfc3339();
        close_workflow_phase(&pool, "phase-3", &exit_time)
            .await
            .unwrap();

        // After closing — should return None.
        let current = get_current_phase(&pool, "spec-wp-3").await.unwrap();
        assert!(current.is_none());
    }

    #[tokio::test]
    async fn test_list_phases_returns_all() {
        let pool = open_test_db().await;
        seed_spec(&pool, "spec-wp-4").await;

        insert_workflow_phase(
            &pool,
            "ph-a",
            "spec-wp-4",
            WorkflowPhaseKind::Planning,
            None,
            None,
        )
        .await
        .unwrap();
        insert_workflow_phase(
            &pool,
            "ph-b",
            "spec-wp-4",
            WorkflowPhaseKind::InProgress,
            None,
            None,
        )
        .await
        .unwrap();

        let phases = list_phases(&pool, "spec-wp-4").await.unwrap();
        assert_eq!(phases.len(), 2);
    }

    // ------------------------------------------------------------------
    // ReviewRequirement tests
    // ------------------------------------------------------------------

    #[tokio::test]
    async fn test_insert_review_requirement() {
        let pool = open_test_db().await;
        seed_spec(&pool, "spec-rr-1").await;

        let req = insert_review_requirement(
            &pool,
            "req-1",
            "spec-rr-1",
            ReviewRequirementKind::TestPass,
            "All tests must pass",
        )
        .await
        .unwrap();

        assert_eq!(req.id, "req-1");
        assert_eq!(req.kind, "test_pass");
        assert!(!req.satisfied);
        assert!(req.satisfied_at.is_none());
    }

    #[tokio::test]
    async fn test_satisfy_review_requirement() {
        let pool = open_test_db().await;
        seed_spec(&pool, "spec-rr-2").await;

        insert_review_requirement(
            &pool,
            "req-2",
            "spec-rr-2",
            ReviewRequirementKind::LintPass,
            "Lint must pass",
        )
        .await
        .unwrap();

        satisfy_review_requirement(&pool, "req-2", Some("agent-y"))
            .await
            .unwrap();

        let reqs = list_review_requirements(&pool, "spec-rr-2").await.unwrap();
        assert_eq!(reqs.len(), 1);
        assert!(reqs[0].satisfied);
        assert!(reqs[0].satisfied_at.is_some());
        assert_eq!(reqs[0].satisfied_by.as_deref(), Some("agent-y"));
    }

    #[tokio::test]
    async fn test_unsatisfied_requirements_filters_correctly() {
        let pool = open_test_db().await;
        seed_spec(&pool, "spec-rr-3").await;

        insert_review_requirement(
            &pool,
            "req-3a",
            "spec-rr-3",
            ReviewRequirementKind::TestPass,
            "Tests",
        )
        .await
        .unwrap();
        insert_review_requirement(
            &pool,
            "req-3b",
            "spec-rr-3",
            ReviewRequirementKind::ReviewApproved,
            "Review",
        )
        .await
        .unwrap();

        // Satisfy only the first one.
        satisfy_review_requirement(&pool, "req-3a", None)
            .await
            .unwrap();

        let unsatisfied = unsatisfied_requirements(&pool, "spec-rr-3").await.unwrap();
        assert_eq!(unsatisfied.len(), 1);
        assert_eq!(unsatisfied[0].id, "req-3b");

        let all = list_review_requirements(&pool, "spec-rr-3").await.unwrap();
        assert_eq!(all.len(), 2);
    }

    // ------------------------------------------------------------------
    // SessionCheckpoint tests
    // ------------------------------------------------------------------

    #[tokio::test]
    async fn test_save_and_retrieve_checkpoint() {
        let pool = open_test_db().await;
        seed_session(&pool, "sess-1").await;

        let data = r#"{"step": "planning", "progress": 42}"#;
        let cp = save_checkpoint(
            &pool,
            "cp-1",
            "sess-1",
            None,
            None,
            "agent-z",
            data,
            Some("initial"),
        )
        .await
        .unwrap();

        assert_eq!(cp.id, "cp-1");
        assert_eq!(cp.session_id, "sess-1");
        assert_eq!(cp.checkpoint_data, data);
        assert_eq!(cp.label.as_deref(), Some("initial"));
    }

    #[tokio::test]
    async fn test_get_latest_checkpoint() {
        let pool = open_test_db().await;
        seed_session(&pool, "sess-2").await;

        save_checkpoint(
            &pool,
            "cp-2a",
            "sess-2",
            None,
            None,
            "agent-z",
            r#"{"v":1}"#,
            None,
        )
        .await
        .unwrap();
        // Small sleep to ensure ordering by saved_at.
        tokio::time::sleep(std::time::Duration::from_millis(5)).await;
        save_checkpoint(
            &pool,
            "cp-2b",
            "sess-2",
            None,
            None,
            "agent-z",
            r#"{"v":2}"#,
            Some("latest"),
        )
        .await
        .unwrap();

        let latest = get_latest_checkpoint(&pool, "sess-2").await.unwrap();
        assert!(latest.is_some());
        let latest = latest.unwrap();
        assert_eq!(latest.id, "cp-2b");
        assert_eq!(latest.checkpoint_data, r#"{"v":2}"#);
    }

    #[tokio::test]
    async fn test_checkpoint_data_roundtrip() {
        let pool = open_test_db().await;
        seed_session(&pool, "sess-3").await;

        let data = r#"{"nested":{"key":"value"},"arr":[1,2,3]}"#;
        save_checkpoint(&pool, "cp-3", "sess-3", None, None, "agent-z", data, None)
            .await
            .unwrap();

        let cps = list_checkpoints(&pool, "sess-3").await.unwrap();
        assert_eq!(cps.len(), 1);
        assert_eq!(cps[0].checkpoint_data, data);
    }

    #[tokio::test]
    async fn test_list_checkpoints_returns_all_for_session() {
        let pool = open_test_db().await;
        seed_session(&pool, "sess-4").await;

        for i in 0..3u32 {
            save_checkpoint(
                &pool,
                &format!("cp-4-{i}"),
                "sess-4",
                None,
                None,
                "agent-z",
                &format!(r#"{{"i":{i}}}"#),
                None,
            )
            .await
            .unwrap();
        }

        let cps = list_checkpoints(&pool, "sess-4").await.unwrap();
        assert_eq!(cps.len(), 3);
    }

    // ------------------------------------------------------------------
    // Enum helpers
    // ------------------------------------------------------------------

    #[test]
    fn test_workflow_phase_kind_roundtrip() {
        for kind in [
            WorkflowPhaseKind::Planning,
            WorkflowPhaseKind::InProgress,
            WorkflowPhaseKind::Review,
            WorkflowPhaseKind::Done,
        ] {
            let s = kind.as_str();
            let back = WorkflowPhaseKind::from_str(s).unwrap();
            assert_eq!(kind, back);
        }
        assert!(WorkflowPhaseKind::from_str("unknown").is_none());
    }

    #[test]
    fn test_review_requirement_kind_roundtrip() {
        for kind in [
            ReviewRequirementKind::TestPass,
            ReviewRequirementKind::LintPass,
            ReviewRequirementKind::ReviewApproved,
            ReviewRequirementKind::Custom,
        ] {
            let s = kind.as_str();
            let back = ReviewRequirementKind::from_str(s).unwrap();
            assert_eq!(kind, back);
        }
        assert!(ReviewRequirementKind::from_str("unknown").is_none());
    }

    // ------------------------------------------------------------------
    // Phase transition service tests
    // ------------------------------------------------------------------

    #[tokio::test]
    async fn test_transition_phase_closes_previous_and_opens_new() {
        let pool = open_test_db().await;
        seed_spec(&pool, "spec-tp-1").await;

        // Start in planning.
        let p1 = transition_phase(
            &pool,
            "spec-tp-1",
            WorkflowPhaseKind::Planning,
            Some("agent-a"),
            None,
        )
        .await
        .unwrap();
        assert_eq!(p1.phase, "planning");
        assert!(p1.exited_at.is_none());

        // Transition to in_progress — planning phase should be closed.
        let p2 = transition_phase(
            &pool,
            "spec-tp-1",
            WorkflowPhaseKind::InProgress,
            Some("agent-b"),
            None,
        )
        .await
        .unwrap();
        assert_eq!(p2.phase, "in_progress");
        assert!(p2.exited_at.is_none());

        // The old planning phase must now have exited_at set.
        let all = list_phases(&pool, "spec-tp-1").await.unwrap();
        assert_eq!(all.len(), 2);
        let planning = all.iter().find(|ph| ph.phase == "planning").unwrap();
        assert!(
            planning.exited_at.is_some(),
            "planning phase should be closed"
        );

        // Only one open phase.
        let current = get_current_phase(&pool, "spec-tp-1").await.unwrap();
        assert_eq!(current.unwrap().phase, "in_progress");
    }

    // ------------------------------------------------------------------
    // enter_review tests
    // ------------------------------------------------------------------

    #[tokio::test]
    async fn test_enter_review_seeds_default_requirements() {
        let pool = open_test_db().await;
        seed_spec(&pool, "spec-er-1").await;

        enter_review(&pool, "spec-er-1", Some("agent-a"))
            .await
            .unwrap();

        let reqs = list_review_requirements(&pool, "spec-er-1").await.unwrap();
        assert_eq!(reqs.len(), 3, "should seed 3 default requirements");

        let kinds: Vec<&str> = reqs.iter().map(|r| r.kind.as_str()).collect();
        assert!(kinds.contains(&"test_pass"));
        assert!(kinds.contains(&"lint_pass"));
        assert!(kinds.contains(&"review_approved"));
    }

    #[tokio::test]
    async fn test_enter_review_does_not_reseed_if_requirements_exist() {
        let pool = open_test_db().await;
        seed_spec(&pool, "spec-er-2").await;

        // Pre-seed one requirement.
        insert_review_requirement(
            &pool,
            "req-pre",
            "spec-er-2",
            ReviewRequirementKind::Custom,
            "Custom check",
        )
        .await
        .unwrap();

        enter_review(&pool, "spec-er-2", None).await.unwrap();

        // Should still be only 1 requirement (no re-seeding).
        let reqs = list_review_requirements(&pool, "spec-er-2").await.unwrap();
        assert_eq!(
            reqs.len(),
            1,
            "should not re-seed when requirements already exist"
        );
    }

    // ------------------------------------------------------------------
    // approve_review tests
    // ------------------------------------------------------------------

    #[tokio::test]
    async fn test_approve_review_satisfies_review_approved_and_transitions_to_done() {
        let pool = open_test_db().await;
        seed_spec(&pool, "spec-ar-1").await;

        // Enter review — seeds 3 default requirements.
        enter_review(&pool, "spec-ar-1", None).await.unwrap();

        // Satisfy test_pass and lint_pass manually.
        let reqs = list_review_requirements(&pool, "spec-ar-1").await.unwrap();
        for req in &reqs {
            if req.kind != "review_approved" {
                satisfy_review_requirement(&pool, &req.id, Some("ci"))
                    .await
                    .unwrap();
            }
        }

        // Now approve — should satisfy review_approved and transition to done.
        let transitioned = approve_review(&pool, "spec-ar-1", "human-reviewer")
            .await
            .unwrap();
        assert!(
            transitioned,
            "should return true when all requirements satisfied"
        );

        let current = get_current_phase(&pool, "spec-ar-1").await.unwrap();
        assert_eq!(current.unwrap().phase, "done");
    }

    #[tokio::test]
    async fn test_approve_review_returns_false_when_other_requirements_unsatisfied() {
        let pool = open_test_db().await;
        seed_spec(&pool, "spec-ar-2").await;

        // Enter review — seeds 3 default requirements (test_pass, lint_pass, review_approved).
        enter_review(&pool, "spec-ar-2", None).await.unwrap();

        // Do NOT satisfy test_pass or lint_pass — only call approve_review.
        let transitioned = approve_review(&pool, "spec-ar-2", "human-reviewer")
            .await
            .unwrap();
        assert!(
            !transitioned,
            "should return false when other requirements still unsatisfied"
        );

        // Phase should still be review, not done.
        let current = get_current_phase(&pool, "spec-ar-2").await.unwrap();
        assert_eq!(current.unwrap().phase, "review");
    }

    // ------------------------------------------------------------------
    // Readiness synthesis tests
    // ------------------------------------------------------------------

    #[tokio::test]
    async fn test_spec_readiness_ready_when_no_requirements_no_tasks_no_acs() {
        let pool = open_test_db().await;
        seed_spec(&pool, "spec-rd-1").await;

        let report = spec_readiness(&pool, "spec-rd-1").await.unwrap();
        assert!(
            report.ready,
            "spec with no requirements, tasks, or ACs should be ready"
        );
        assert!(report.blockers.is_empty());
        assert_eq!(report.review_requirements_total, 0);
        assert_eq!(report.review_requirements_satisfied, 0);
    }

    #[tokio::test]
    async fn test_spec_readiness_blocker_for_unsatisfied_requirement() {
        let pool = open_test_db().await;
        seed_spec(&pool, "spec-rd-2").await;

        insert_review_requirement(
            &pool,
            "req-rd-2",
            "spec-rd-2",
            ReviewRequirementKind::TestPass,
            "All tests must pass",
        )
        .await
        .unwrap();

        let report = spec_readiness(&pool, "spec-rd-2").await.unwrap();
        assert!(!report.ready);
        assert_eq!(report.review_requirements_total, 1);
        assert_eq!(report.review_requirements_satisfied, 0);
        let blocker = report
            .blockers
            .iter()
            .find(|b| b.kind == "unsatisfied_requirement");
        assert!(
            blocker.is_some(),
            "should have unsatisfied_requirement blocker"
        );
        assert_eq!(blocker.unwrap().description, "All tests must pass");
    }

    #[tokio::test]
    async fn test_spec_readiness_blocker_for_incomplete_tasks() {
        let pool = open_test_db().await;
        seed_spec(&pool, "spec-rd-3").await;

        // Insert a pending task for this spec.
        let now = Utc::now().to_rfc3339();
        sqlx::query(
            "INSERT INTO tasks (id, spec, title, agent, status, inputs, created_at, updated_at)
             VALUES (?, ?, 'Test task', 'agent-x', 'pending', '[]', ?, ?)",
        )
        .bind("task-rd-3")
        .bind("spec-rd-3")
        .bind(&now)
        .bind(&now)
        .execute(&pool)
        .await
        .unwrap();

        let report = spec_readiness(&pool, "spec-rd-3").await.unwrap();
        assert!(!report.ready);
        let blocker = report
            .blockers
            .iter()
            .find(|b| b.kind == "tasks_incomplete");
        assert!(blocker.is_some(), "should have tasks_incomplete blocker");
        assert!(blocker.unwrap().description.contains('1'));
    }

    #[tokio::test]
    async fn test_operator_readiness_aggregates_correctly() {
        let pool = open_test_db().await;

        // Spec A: ready (no requirements, no tasks, no ACs)
        seed_spec(&pool, "spec-op-a").await;

        // Spec B: blocked by unsatisfied requirement
        seed_spec(&pool, "spec-op-b").await;
        insert_review_requirement(
            &pool,
            "req-op-b",
            "spec-op-b",
            ReviewRequirementKind::LintPass,
            "Lint must pass",
        )
        .await
        .unwrap();

        let report = operator_readiness(&pool).await.unwrap();
        // There may be other specs from other tests in the same DB, so we check
        // that our two specs are present and correctly classified.
        let spec_a = report.specs.iter().find(|s| s.spec_id == "spec-op-a");
        let spec_b = report.specs.iter().find(|s| s.spec_id == "spec-op-b");
        assert!(spec_a.is_some());
        assert!(spec_b.is_some());
        assert!(spec_a.unwrap().ready, "spec-op-a should be ready");
        assert!(!spec_b.unwrap().ready, "spec-op-b should be blocked");
        assert!(report.total_specs >= 2);
        assert!(report.ready_specs >= 1);
        assert!(report.blocked_specs >= 1);
        assert_eq!(
            report.total_specs,
            report.ready_specs + report.blocked_specs
        );
    }
}
