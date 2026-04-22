#![allow(dead_code)]
#![allow(clippy::type_complexity)]

use anyhow::{anyhow, Result};
use chrono::Utc;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use sqlx::SqlitePool;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum EvidenceEntityKind {
    Task,
    Spec,
}

impl EvidenceEntityKind {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Task => "task",
            Self::Spec => "spec",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum EvidenceBundleStatus {
    Draft,
    Submitted,
    Accepted,
    Rejected,
}

impl EvidenceBundleStatus {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Draft => "draft",
            Self::Submitted => "submitted",
            Self::Accepted => "accepted",
            Self::Rejected => "rejected",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ValidationCommandAlias {
    Fast,
    Primary,
    Full,
    Custom,
}

impl ValidationCommandAlias {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Fast => "fast",
            Self::Primary => "primary",
            Self::Full => "full",
            Self::Custom => "custom",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ValidationRunSource {
    Recorded,
    Cli,
    Mcp,
}

impl ValidationRunSource {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Recorded => "recorded",
            Self::Cli => "cli",
            Self::Mcp => "mcp",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum EvidenceArtifactRole {
    Supporting,
    PrimaryOutput,
    TestEvidence,
}

impl EvidenceArtifactRole {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Supporting => "supporting",
            Self::PrimaryOutput => "primary_output",
            Self::TestEvidence => "test_evidence",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ValidationRequirementLevel {
    Fast,
    Primary,
    Full,
    Custom,
}

impl ValidationRequirementLevel {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Fast => "fast",
            Self::Primary => "primary",
            Self::Full => "full",
            Self::Custom => "custom",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct EvidenceRef {
    pub entity_kind: EvidenceEntityKind,
    pub entity_id: String,
    pub spec: String,
    pub task: Option<String>,
}

impl EvidenceRef {
    pub fn for_task(spec: impl Into<String>, task: impl Into<String>) -> Self {
        let task = task.into();
        Self {
            entity_kind: EvidenceEntityKind::Task,
            entity_id: task.clone(),
            spec: spec.into(),
            task: Some(task),
        }
    }

    pub fn for_spec(spec: impl Into<String>) -> Self {
        let spec = spec.into();
        Self {
            entity_kind: EvidenceEntityKind::Spec,
            entity_id: spec.clone(),
            spec,
            task: None,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct EvidenceBundle {
    pub id: String,
    pub entity_kind: String,
    pub entity_id: String,
    pub spec: String,
    pub task: Option<String>,
    pub status: String,
    pub summary: Option<String>,
    pub behavior_change: bool,
    pub metadata_json: Value,
    pub created_by: Option<String>,
    pub updated_by: Option<String>,
    pub created_at: String,
    pub updated_at: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ValidationRun {
    pub id: String,
    pub evidence_bundle_id: Option<String>,
    pub entity_kind: String,
    pub entity_id: String,
    pub spec: String,
    pub task: Option<String>,
    pub command_alias: String,
    pub command: String,
    pub source: String,
    pub exit_code: Option<i64>,
    pub success: bool,
    pub ran_at: String,
    pub recorded_at: String,
    pub recorded_by: Option<String>,
    pub output_summary: Option<String>,
    pub metadata_json: Value,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct EvidenceBundleArtifact {
    pub evidence_bundle_id: String,
    pub artifact_id: String,
    pub role: String,
    pub created_at: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct EvidenceBundleValidation {
    pub evidence_bundle_id: String,
    pub validation_run_id: String,
    pub requirement_level: String,
    pub created_at: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct EvidenceBundleDetails {
    pub bundle: EvidenceBundle,
    pub artifacts: Vec<EvidenceBundleArtifact>,
    pub validations: Vec<EvidenceBundleValidation>,
}

#[derive(Debug, Clone)]
pub struct NewEvidenceBundle<'a> {
    pub id: &'a str,
    pub reference: EvidenceRef,
    pub status: EvidenceBundleStatus,
    pub summary: Option<&'a str>,
    pub behavior_change: bool,
    pub metadata_json: Value,
    pub created_by: Option<&'a str>,
    pub updated_by: Option<&'a str>,
}

#[derive(Debug, Clone)]
pub struct EvidenceBundlePatch<'a> {
    pub status: EvidenceBundleStatus,
    pub summary: Option<&'a str>,
    pub behavior_change: bool,
    pub metadata_json: Value,
    pub updated_by: Option<&'a str>,
}

#[derive(Debug, Clone)]
pub struct RecordedValidationRun<'a> {
    pub id: &'a str,
    pub evidence_bundle_id: Option<&'a str>,
    pub reference: EvidenceRef,
    pub command_alias: ValidationCommandAlias,
    pub command: &'a str,
    pub source: ValidationRunSource,
    pub exit_code: Option<i64>,
    pub success: bool,
    pub ran_at: &'a str,
    pub recorded_by: Option<&'a str>,
    pub output_summary: Option<&'a str>,
    pub metadata_json: Value,
}

fn parse_json_value(raw: &str) -> Result<Value> {
    serde_json::from_str(raw).map_err(Into::into)
}

fn map_evidence_bundle(
    row: (
        String,
        String,
        String,
        String,
        Option<String>,
        String,
        Option<String>,
        i64,
        String,
        Option<String>,
        Option<String>,
        String,
        String,
    ),
) -> Result<EvidenceBundle> {
    let (
        id,
        entity_kind,
        entity_id,
        spec,
        task,
        status,
        summary,
        behavior_change,
        metadata_json,
        created_by,
        updated_by,
        created_at,
        updated_at,
    ) = row;

    Ok(EvidenceBundle {
        id,
        entity_kind,
        entity_id,
        spec,
        task,
        status,
        summary,
        behavior_change: behavior_change != 0,
        metadata_json: parse_json_value(&metadata_json)?,
        created_by,
        updated_by,
        created_at,
        updated_at,
    })
}

fn map_validation_run(
    row: (
        String,
        Option<String>,
        String,
        String,
        String,
        Option<String>,
        String,
        String,
        String,
        Option<i64>,
        i64,
        String,
        String,
        Option<String>,
        Option<String>,
        String,
    ),
) -> Result<ValidationRun> {
    let (
        id,
        evidence_bundle_id,
        entity_kind,
        entity_id,
        spec,
        task,
        command_alias,
        command,
        source,
        exit_code,
        success,
        ran_at,
        recorded_at,
        recorded_by,
        output_summary,
        metadata_json,
    ) = row;

    Ok(ValidationRun {
        id,
        evidence_bundle_id,
        entity_kind,
        entity_id,
        spec,
        task,
        command_alias,
        command,
        source,
        exit_code,
        success: success != 0,
        ran_at,
        recorded_at,
        recorded_by,
        output_summary,
        metadata_json: parse_json_value(&metadata_json)?,
    })
}

pub async fn create_evidence_bundle(
    pool: &SqlitePool,
    new_bundle: NewEvidenceBundle<'_>,
) -> Result<EvidenceBundle> {
    let now = Utc::now().to_rfc3339();
    let metadata_json = serde_json::to_string(&new_bundle.metadata_json)?;

    sqlx::query(
        "INSERT INTO evidence_bundles \
         (id, entity_kind, entity_id, spec, task, status, summary, behavior_change, metadata_json, created_by, updated_by, created_at, updated_at) \
         VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)",
    )
    .bind(new_bundle.id)
    .bind(new_bundle.reference.entity_kind.as_str())
    .bind(&new_bundle.reference.entity_id)
    .bind(&new_bundle.reference.spec)
    .bind(new_bundle.reference.task.as_deref())
    .bind(new_bundle.status.as_str())
    .bind(new_bundle.summary)
    .bind(new_bundle.behavior_change)
    .bind(metadata_json)
    .bind(new_bundle.created_by)
    .bind(new_bundle.updated_by)
    .bind(&now)
    .bind(&now)
    .execute(pool)
    .await?;

    get_evidence_bundle(pool, new_bundle.id)
        .await?
        .ok_or_else(|| {
            anyhow!(
                "Failed to retrieve evidence bundle '{}' after creation",
                new_bundle.id
            )
        })
}

pub async fn get_evidence_bundle(pool: &SqlitePool, id: &str) -> Result<Option<EvidenceBundle>> {
    let row = sqlx::query_as::<
        _,
        (
            String,
            String,
            String,
            String,
            Option<String>,
            String,
            Option<String>,
            i64,
            String,
            Option<String>,
            Option<String>,
            String,
            String,
        ),
    >(
        "SELECT id, entity_kind, entity_id, spec, task, status, summary, behavior_change, metadata_json, created_by, updated_by, created_at, updated_at \
         FROM evidence_bundles WHERE id = ?",
    )
    .bind(id)
    .fetch_optional(pool)
    .await?;

    row.map(map_evidence_bundle).transpose()
}

pub async fn get_evidence_bundle_for_entity(
    pool: &SqlitePool,
    reference: &EvidenceRef,
) -> Result<Option<EvidenceBundle>> {
    let row = sqlx::query_as::<
        _,
        (
            String,
            String,
            String,
            String,
            Option<String>,
            String,
            Option<String>,
            i64,
            String,
            Option<String>,
            Option<String>,
            String,
            String,
        ),
    >(
        "SELECT id, entity_kind, entity_id, spec, task, status, summary, behavior_change, metadata_json, created_by, updated_by, created_at, updated_at \
         FROM evidence_bundles WHERE entity_kind = ? AND entity_id = ?",
    )
    .bind(reference.entity_kind.as_str())
    .bind(&reference.entity_id)
    .fetch_optional(pool)
    .await?;

    row.map(map_evidence_bundle).transpose()
}

pub async fn update_evidence_bundle(
    pool: &SqlitePool,
    id: &str,
    patch: EvidenceBundlePatch<'_>,
) -> Result<EvidenceBundle> {
    let now = Utc::now().to_rfc3339();
    let metadata_json = serde_json::to_string(&patch.metadata_json)?;

    let result = sqlx::query(
        "UPDATE evidence_bundles \
         SET status = ?, summary = ?, behavior_change = ?, metadata_json = ?, updated_by = ?, updated_at = ? \
         WHERE id = ?",
    )
    .bind(patch.status.as_str())
    .bind(patch.summary)
    .bind(patch.behavior_change)
    .bind(metadata_json)
    .bind(patch.updated_by)
    .bind(&now)
    .bind(id)
    .execute(pool)
    .await?;

    if result.rows_affected() == 0 {
        return Err(anyhow!("Evidence bundle '{}' not found", id));
    }

    get_evidence_bundle(pool, id)
        .await?
        .ok_or_else(|| anyhow!("Evidence bundle '{}' not found", id))
}

pub async fn list_evidence_bundles(
    pool: &SqlitePool,
    spec_filter: Option<&str>,
    task_filter: Option<&str>,
    status_filter: Option<&str>,
) -> Result<Vec<EvidenceBundle>> {
    let mut query = String::from(
        "SELECT id, entity_kind, entity_id, spec, task, status, summary, behavior_change, metadata_json, created_by, updated_by, created_at, updated_at \
         FROM evidence_bundles WHERE 1=1",
    );

    if spec_filter.is_some() {
        query.push_str(" AND spec = ?");
    }
    if task_filter.is_some() {
        query.push_str(" AND task = ?");
    }
    if status_filter.is_some() {
        query.push_str(" AND status = ?");
    }
    query.push_str(" ORDER BY updated_at DESC, id");

    let mut q = sqlx::query_as::<
        _,
        (
            String,
            String,
            String,
            String,
            Option<String>,
            String,
            Option<String>,
            i64,
            String,
            Option<String>,
            Option<String>,
            String,
            String,
        ),
    >(&query);

    if let Some(spec) = spec_filter {
        q = q.bind(spec);
    }
    if let Some(task) = task_filter {
        q = q.bind(task);
    }
    if let Some(status) = status_filter {
        q = q.bind(status);
    }

    q.fetch_all(pool)
        .await?
        .into_iter()
        .map(map_evidence_bundle)
        .collect()
}

pub async fn record_validation_run(
    pool: &SqlitePool,
    run: RecordedValidationRun<'_>,
) -> Result<ValidationRun> {
    let metadata_json = serde_json::to_string(&run.metadata_json)?;

    sqlx::query(
        "INSERT INTO validation_runs \
         (id, evidence_bundle_id, entity_kind, entity_id, spec, task, command_alias, command, source, exit_code, success, ran_at, recorded_by, output_summary, metadata_json) \
         VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)",
    )
    .bind(run.id)
    .bind(run.evidence_bundle_id)
    .bind(run.reference.entity_kind.as_str())
    .bind(&run.reference.entity_id)
    .bind(&run.reference.spec)
    .bind(run.reference.task.as_deref())
    .bind(run.command_alias.as_str())
    .bind(run.command)
    .bind(run.source.as_str())
    .bind(run.exit_code)
    .bind(run.success)
    .bind(run.ran_at)
    .bind(run.recorded_by)
    .bind(run.output_summary)
    .bind(metadata_json)
    .execute(pool)
    .await?;

    get_validation_run(pool, run.id).await?.ok_or_else(|| {
        anyhow!(
            "Failed to retrieve validation run '{}' after creation",
            run.id
        )
    })
}

pub async fn get_validation_run(pool: &SqlitePool, id: &str) -> Result<Option<ValidationRun>> {
    let row = sqlx::query_as::<
        _,
        (
            String,
            Option<String>,
            String,
            String,
            String,
            Option<String>,
            String,
            String,
            String,
            Option<i64>,
            i64,
            String,
            String,
            Option<String>,
            Option<String>,
            String,
        ),
    >(
        "SELECT id, evidence_bundle_id, entity_kind, entity_id, spec, task, command_alias, command, source, exit_code, success, ran_at, recorded_at, recorded_by, output_summary, metadata_json \
         FROM validation_runs WHERE id = ?",
    )
    .bind(id)
    .fetch_optional(pool)
    .await?;

    row.map(map_validation_run).transpose()
}

pub async fn list_validation_runs(
    pool: &SqlitePool,
    spec_filter: Option<&str>,
    task_filter: Option<&str>,
    command_alias_filter: Option<&str>,
    success_filter: Option<bool>,
) -> Result<Vec<ValidationRun>> {
    let mut query = String::from(
        "SELECT id, evidence_bundle_id, entity_kind, entity_id, spec, task, command_alias, command, source, exit_code, success, ran_at, recorded_at, recorded_by, output_summary, metadata_json \
         FROM validation_runs WHERE 1=1",
    );

    if spec_filter.is_some() {
        query.push_str(" AND spec = ?");
    }
    if task_filter.is_some() {
        query.push_str(" AND task = ?");
    }
    if command_alias_filter.is_some() {
        query.push_str(" AND command_alias = ?");
    }
    if success_filter.is_some() {
        query.push_str(" AND success = ?");
    }
    query.push_str(" ORDER BY ran_at DESC, recorded_at DESC, id");

    let mut q = sqlx::query_as::<
        _,
        (
            String,
            Option<String>,
            String,
            String,
            String,
            Option<String>,
            String,
            String,
            String,
            Option<i64>,
            i64,
            String,
            String,
            Option<String>,
            Option<String>,
            String,
        ),
    >(&query);

    if let Some(spec) = spec_filter {
        q = q.bind(spec);
    }
    if let Some(task) = task_filter {
        q = q.bind(task);
    }
    if let Some(alias) = command_alias_filter {
        q = q.bind(alias);
    }
    if let Some(success) = success_filter {
        q = q.bind(success);
    }

    q.fetch_all(pool)
        .await?
        .into_iter()
        .map(map_validation_run)
        .collect()
}

pub async fn attach_artifact_to_evidence_bundle(
    pool: &SqlitePool,
    evidence_bundle_id: &str,
    artifact_id: &str,
    role: EvidenceArtifactRole,
) -> Result<EvidenceBundleArtifact> {
    sqlx::query(
        "INSERT INTO evidence_bundle_artifacts (evidence_bundle_id, artifact_id, role) VALUES (?, ?, ?)",
    )
    .bind(evidence_bundle_id)
    .bind(artifact_id)
    .bind(role.as_str())
    .execute(pool)
    .await?;

    list_evidence_bundle_artifacts(pool, evidence_bundle_id)
        .await?
        .into_iter()
        .find(|link| link.artifact_id == artifact_id)
        .ok_or_else(|| {
            anyhow!(
                "Failed to retrieve artifact link for bundle '{}'",
                evidence_bundle_id
            )
        })
}

pub async fn attach_validation_run_to_evidence_bundle(
    pool: &SqlitePool,
    evidence_bundle_id: &str,
    validation_run_id: &str,
    requirement_level: ValidationRequirementLevel,
) -> Result<EvidenceBundleValidation> {
    sqlx::query(
        "INSERT INTO evidence_bundle_validations (evidence_bundle_id, validation_run_id, requirement_level) VALUES (?, ?, ?)",
    )
    .bind(evidence_bundle_id)
    .bind(validation_run_id)
    .bind(requirement_level.as_str())
    .execute(pool)
    .await?;

    sqlx::query("UPDATE validation_runs SET evidence_bundle_id = ? WHERE id = ?")
        .bind(evidence_bundle_id)
        .bind(validation_run_id)
        .execute(pool)
        .await?;

    list_evidence_bundle_validations(pool, evidence_bundle_id)
        .await?
        .into_iter()
        .find(|link| link.validation_run_id == validation_run_id)
        .ok_or_else(|| {
            anyhow!(
                "Failed to retrieve validation link for bundle '{}'",
                evidence_bundle_id
            )
        })
}

pub async fn list_evidence_bundle_artifacts(
    pool: &SqlitePool,
    evidence_bundle_id: &str,
) -> Result<Vec<EvidenceBundleArtifact>> {
    let rows = sqlx::query_as::<_, (String, String, String, String)>(
        "SELECT evidence_bundle_id, artifact_id, role, created_at \
         FROM evidence_bundle_artifacts WHERE evidence_bundle_id = ? \
         ORDER BY CASE role WHEN 'primary_output' THEN 0 WHEN 'test_evidence' THEN 1 ELSE 2 END, created_at, artifact_id",
    )
    .bind(evidence_bundle_id)
    .fetch_all(pool)
    .await?;

    Ok(rows
        .into_iter()
        .map(
            |(evidence_bundle_id, artifact_id, role, created_at)| EvidenceBundleArtifact {
                evidence_bundle_id,
                artifact_id,
                role,
                created_at,
            },
        )
        .collect())
}

pub async fn list_evidence_bundle_validations(
    pool: &SqlitePool,
    evidence_bundle_id: &str,
) -> Result<Vec<EvidenceBundleValidation>> {
    let rows = sqlx::query_as::<_, (String, String, String, String)>(
        "SELECT evidence_bundle_id, validation_run_id, requirement_level, created_at \
         FROM evidence_bundle_validations WHERE evidence_bundle_id = ? \
         ORDER BY CASE requirement_level WHEN 'full' THEN 0 WHEN 'primary' THEN 1 WHEN 'fast' THEN 2 ELSE 3 END, created_at, validation_run_id",
    )
    .bind(evidence_bundle_id)
    .fetch_all(pool)
    .await?;

    Ok(rows
        .into_iter()
        .map(
            |(evidence_bundle_id, validation_run_id, requirement_level, created_at)| {
                EvidenceBundleValidation {
                    evidence_bundle_id,
                    validation_run_id,
                    requirement_level,
                    created_at,
                }
            },
        )
        .collect())
}

pub async fn get_evidence_bundle_details(
    pool: &SqlitePool,
    evidence_bundle_id: &str,
) -> Result<Option<EvidenceBundleDetails>> {
    let Some(bundle) = get_evidence_bundle(pool, evidence_bundle_id).await? else {
        return Ok(None);
    };

    Ok(Some(EvidenceBundleDetails {
        artifacts: list_evidence_bundle_artifacts(pool, evidence_bundle_id).await?,
        validations: list_evidence_bundle_validations(pool, evidence_bundle_id).await?,
        bundle,
    }))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::sdd::{
        artifact::register_artifact, spec::create_spec, task::create_task, test_helpers::make_pool,
    };
    use serde_json::json;

    async fn seed_task(pool: &SqlitePool, spec_id: &str, task_id: &str) {
        create_spec(pool, spec_id, "Evidence Spec", "P0", &[])
            .await
            .unwrap();
        create_task(
            pool,
            task_id,
            spec_id,
            "Evidence Task",
            "sdd-builder",
            &[],
            None,
        )
        .await
        .unwrap();
    }

    #[tokio::test]
    async fn create_and_lookup_task_evidence_bundle_round_trips_metadata() {
        let pool = make_pool().await;
        seed_task(&pool, "SPEC-E1", "TASK-E1").await;

        let bundle = create_evidence_bundle(
            &pool,
            NewEvidenceBundle {
                id: "bundle-1",
                reference: EvidenceRef::for_task("SPEC-E1", "TASK-E1"),
                status: EvidenceBundleStatus::Draft,
                summary: Some("Primary validation passed"),
                behavior_change: true,
                metadata_json: json!({"change_summary": "updated workflow guard"}),
                created_by: Some("sdd-builder"),
                updated_by: Some("sdd-builder"),
            },
        )
        .await
        .unwrap();

        assert_eq!(bundle.entity_kind, "task");
        assert_eq!(bundle.entity_id, "TASK-E1");
        assert_eq!(bundle.task.as_deref(), Some("TASK-E1"));
        assert!(bundle.behavior_change);
        assert_eq!(
            bundle.metadata_json["change_summary"],
            "updated workflow guard"
        );

        let fetched =
            get_evidence_bundle_for_entity(&pool, &EvidenceRef::for_task("SPEC-E1", "TASK-E1"))
                .await
                .unwrap()
                .unwrap();
        assert_eq!(fetched.id, "bundle-1");
    }

    #[tokio::test]
    async fn bundle_details_follow_join_tables_for_artifacts_and_validations() {
        let pool = make_pool().await;
        seed_task(&pool, "SPEC-E2", "TASK-E2").await;

        create_evidence_bundle(
            &pool,
            NewEvidenceBundle {
                id: "bundle-2",
                reference: EvidenceRef::for_task("SPEC-E2", "TASK-E2"),
                status: EvidenceBundleStatus::Submitted,
                summary: Some("Concise change summary"),
                behavior_change: true,
                metadata_json: json!({"requires_test_evidence": true}),
                created_by: Some("sdd-builder"),
                updated_by: Some("sdd-builder"),
            },
        )
        .await
        .unwrap();

        register_artifact(
            &pool,
            "artifact-primary",
            Some("SPEC-E2"),
            Some("TASK-E2"),
            "sdd-builder",
            "source",
            Some("src/sdd/evidence.rs"),
            Some("main evidence domain module"),
            None,
        )
        .await
        .unwrap();
        register_artifact(
            &pool,
            "artifact-test",
            Some("SPEC-E2"),
            Some("TASK-E2"),
            "sdd-builder",
            "test",
            Some("src/sdd/evidence.rs"),
            Some("evidence persistence tests"),
            None,
        )
        .await
        .unwrap();

        let validation = record_validation_run(
            &pool,
            RecordedValidationRun {
                id: "validation-1",
                evidence_bundle_id: None,
                reference: EvidenceRef::for_task("SPEC-E2", "TASK-E2"),
                command_alias: ValidationCommandAlias::Primary,
                command: "cargo test --all-targets",
                source: ValidationRunSource::Recorded,
                exit_code: Some(0),
                success: true,
                ran_at: "2026-04-22T11:30:00Z",
                recorded_by: Some("sdd-builder"),
                output_summary: Some("policy tests passed"),
                metadata_json: json!({"suite": "policy-engine"}),
            },
        )
        .await
        .unwrap();

        attach_artifact_to_evidence_bundle(
            &pool,
            "bundle-2",
            "artifact-primary",
            EvidenceArtifactRole::PrimaryOutput,
        )
        .await
        .unwrap();
        attach_artifact_to_evidence_bundle(
            &pool,
            "bundle-2",
            "artifact-test",
            EvidenceArtifactRole::TestEvidence,
        )
        .await
        .unwrap();
        attach_validation_run_to_evidence_bundle(
            &pool,
            "bundle-2",
            "validation-1",
            ValidationRequirementLevel::Primary,
        )
        .await
        .unwrap();

        let details = get_evidence_bundle_details(&pool, "bundle-2")
            .await
            .unwrap()
            .unwrap();
        assert_eq!(details.artifacts.len(), 2);
        assert_eq!(details.artifacts[0].artifact_id, "artifact-primary");
        assert_eq!(details.artifacts[0].role, "primary_output");
        assert_eq!(details.validations.len(), 1);
        assert_eq!(details.validations[0].validation_run_id, validation.id);

        let stored_validation = get_validation_run(&pool, "validation-1")
            .await
            .unwrap()
            .unwrap();
        assert_eq!(
            stored_validation.evidence_bundle_id.as_deref(),
            Some("bundle-2")
        );
    }

    #[tokio::test]
    async fn list_validation_runs_filters_by_scope_alias_and_success() {
        let pool = make_pool().await;
        seed_task(&pool, "SPEC-E3", "TASK-E3").await;

        record_validation_run(
            &pool,
            RecordedValidationRun {
                id: "validation-fast",
                evidence_bundle_id: None,
                reference: EvidenceRef::for_task("SPEC-E3", "TASK-E3"),
                command_alias: ValidationCommandAlias::Fast,
                command: "cargo clippy -- -D warnings",
                source: ValidationRunSource::Cli,
                exit_code: Some(0),
                success: true,
                ran_at: "2026-04-22T11:31:00Z",
                recorded_by: Some("developer"),
                output_summary: None,
                metadata_json: json!({}),
            },
        )
        .await
        .unwrap();
        record_validation_run(
            &pool,
            RecordedValidationRun {
                id: "validation-full",
                evidence_bundle_id: None,
                reference: EvidenceRef::for_task("SPEC-E3", "TASK-E3"),
                command_alias: ValidationCommandAlias::Full,
                command: "cargo fmt --all -- --check && cargo test --all-targets",
                source: ValidationRunSource::Cli,
                exit_code: Some(1),
                success: false,
                ran_at: "2026-04-22T11:32:00Z",
                recorded_by: Some("developer"),
                output_summary: Some("fmt check failed"),
                metadata_json: json!({"failure_stage": "fmt"}),
            },
        )
        .await
        .unwrap();

        let successful_fast = list_validation_runs(
            &pool,
            Some("SPEC-E3"),
            Some("TASK-E3"),
            Some("fast"),
            Some(true),
        )
        .await
        .unwrap();

        assert_eq!(successful_fast.len(), 1);
        assert_eq!(successful_fast[0].id, "validation-fast");
    }

    #[tokio::test]
    async fn database_constraints_reject_invalid_entity_shape_and_missing_artifact_links() {
        let pool = make_pool().await;
        seed_task(&pool, "SPEC-E4", "TASK-E4").await;

        let invalid_bundle = create_evidence_bundle(
            &pool,
            NewEvidenceBundle {
                id: "bundle-invalid",
                reference: EvidenceRef::for_spec("SPEC-E4"),
                status: EvidenceBundleStatus::Draft,
                summary: None,
                behavior_change: false,
                metadata_json: json!({}),
                created_by: Some("sdd-builder"),
                updated_by: Some("sdd-builder"),
            },
        )
        .await
        .unwrap();

        let bad_link = attach_artifact_to_evidence_bundle(
            &pool,
            &invalid_bundle.id,
            "missing-artifact",
            EvidenceArtifactRole::Supporting,
        )
        .await;
        assert!(bad_link.is_err());

        let mismatched_run = record_validation_run(
            &pool,
            RecordedValidationRun {
                id: "validation-invalid",
                evidence_bundle_id: None,
                reference: EvidenceRef {
                    entity_kind: EvidenceEntityKind::Task,
                    entity_id: "TASK-E4".to_string(),
                    spec: "SPEC-E4".to_string(),
                    task: None,
                },
                command_alias: ValidationCommandAlias::Primary,
                command: "cargo test --all-targets",
                source: ValidationRunSource::Recorded,
                exit_code: Some(0),
                success: true,
                ran_at: "2026-04-22T11:33:00Z",
                recorded_by: Some("sdd-builder"),
                output_summary: None,
                metadata_json: json!({}),
            },
        )
        .await;
        assert!(mismatched_run.is_err());
    }
}
