use anyhow::{anyhow, Result};
use chrono::Utc;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use sqlx::{Sqlite, SqlitePool, Transaction};

use crate::sdd::{
    event::{emit_event, emit_event_tx},
    evidence::{get_evidence_bundle, get_validation_run},
    policy::get_approval,
    sessions::get_session,
    spec::get_spec,
    task::get_task,
};

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct EvalRun {
    pub id: String,
    pub evaluator: String,
    pub target_kind: String,
    pub target_ref: String,
    pub spec: Option<String>,
    pub task: Option<String>,
    pub artifact_id: Option<String>,
    pub summary: Option<String>,
    pub rationale: Option<String>,
    pub outcome: String,
    pub overall_score: Option<f64>,
    pub source: String,
    pub metadata_json: Value,
    pub created_at: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct EvalScorecardDimension {
    pub eval_run_id: String,
    pub dimension_name: String,
    pub normalized_status: String,
    pub normalized_score: Option<f64>,
    pub normalized_value: Option<String>,
    pub rationale: Option<String>,
    pub details_json: Value,
    pub created_at: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct EvalRunLink {
    pub id: i64,
    pub eval_run_id: String,
    pub link_kind: String,
    pub link_ref: String,
    pub relation: String,
    pub created_at: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct EvalRunDetails {
    pub run: EvalRun,
    pub dimensions: Vec<EvalScorecardDimension>,
    pub links: Vec<EvalRunLink>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct EvalDimensionComparison {
    pub dimension_name: String,
    pub baseline_status: Option<String>,
    pub current_status: Option<String>,
    pub baseline_score: Option<f64>,
    pub current_score: Option<f64>,
    pub score_delta: Option<f64>,
    pub classification: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct EvalRunComparison {
    pub baseline_eval_id: String,
    pub current_eval_id: String,
    pub comparison_group: String,
    pub overall_classification: String,
    pub overall_score_delta: Option<f64>,
    pub dimensions: Vec<EvalDimensionComparison>,
}

#[derive(Debug, Clone, Default)]
pub struct EvalRunFilters<'a> {
    pub spec: Option<&'a str>,
    pub task: Option<&'a str>,
    pub artifact_id: Option<&'a str>,
    pub outcome: Option<&'a str>,
    pub evaluator: Option<&'a str>,
    pub target_kind: Option<&'a str>,
    pub target_ref: Option<&'a str>,
    pub source: Option<&'a str>,
    pub created_after: Option<&'a str>,
    pub created_before: Option<&'a str>,
    pub limit: Option<i64>,
    pub offset: Option<i64>,
}

#[derive(Debug, Clone)]
pub struct RecordEvalRun<'a> {
    pub run: NewEvalRun<'a>,
    pub dimensions: Vec<NewEvalScorecardDimension<'a>>,
    pub links: Vec<NewEvalRunLink<'a>>,
}

#[derive(Debug, Clone)]
pub struct NewEvalRun<'a> {
    pub id: &'a str,
    pub evaluator: &'a str,
    pub target_kind: &'a str,
    pub target_ref: &'a str,
    pub spec: Option<&'a str>,
    pub task: Option<&'a str>,
    pub artifact_id: Option<&'a str>,
    pub summary: Option<&'a str>,
    pub rationale: Option<&'a str>,
    pub outcome: &'a str,
    pub overall_score: Option<f64>,
    pub source: &'a str,
    pub metadata_json: Value,
}

#[derive(Debug, Clone)]
pub struct NewEvalScorecardDimension<'a> {
    pub eval_run_id: &'a str,
    pub dimension_name: &'a str,
    pub normalized_status: &'a str,
    pub normalized_score: Option<f64>,
    pub normalized_value: Option<&'a str>,
    pub rationale: Option<&'a str>,
    pub details_json: Value,
}

#[derive(Debug, Clone)]
pub struct NewEvalRunLink<'a> {
    pub eval_run_id: &'a str,
    pub link_kind: &'a str,
    pub link_ref: &'a str,
    pub relation: &'a str,
}

type EvalRunRow = (
    String,
    String,
    String,
    String,
    Option<String>,
    Option<String>,
    Option<String>,
    Option<String>,
    Option<String>,
    String,
    Option<f64>,
    String,
    String,
    String,
);

type EvalDimensionRow = (
    String,
    String,
    String,
    Option<f64>,
    Option<String>,
    Option<String>,
    String,
    String,
);

type EvalLinkRow = (i64, String, String, String, String, String);

#[derive(Debug, Clone, PartialEq, Eq)]
struct ResolvedEvalScope {
    spec: Option<String>,
    task: Option<String>,
    artifact_id: Option<String>,
}

#[derive(Debug, Clone)]
struct ArtifactScope {
    spec: Option<String>,
    task: Option<String>,
}

#[derive(Debug, Clone)]
struct NormalizedEvalDimension {
    dimension_name: String,
    normalized_status: String,
    normalized_score: Option<f64>,
    normalized_value: Option<String>,
    rationale: Option<String>,
    details_json: Value,
}

fn parse_json_value(raw: &str) -> Result<Value> {
    serde_json::from_str(raw).map_err(Into::into)
}

fn map_eval_run(row: EvalRunRow) -> Result<EvalRun> {
    Ok(EvalRun {
        id: row.0,
        evaluator: row.1,
        target_kind: row.2,
        target_ref: row.3,
        spec: row.4,
        task: row.5,
        artifact_id: row.6,
        summary: row.7,
        rationale: row.8,
        outcome: row.9,
        overall_score: row.10,
        source: row.11,
        metadata_json: parse_json_value(&row.12)?,
        created_at: row.13,
    })
}

fn map_eval_dimension(row: EvalDimensionRow) -> Result<EvalScorecardDimension> {
    Ok(EvalScorecardDimension {
        eval_run_id: row.0,
        dimension_name: row.1,
        normalized_status: row.2,
        normalized_score: row.3,
        normalized_value: row.4,
        rationale: row.5,
        details_json: parse_json_value(&row.6)?,
        created_at: row.7,
    })
}

fn map_eval_link(row: EvalLinkRow) -> EvalRunLink {
    EvalRunLink {
        id: row.0,
        eval_run_id: row.1,
        link_kind: row.2,
        link_ref: row.3,
        relation: row.4,
        created_at: row.5,
    }
}

pub async fn record_eval_run(
    pool: &SqlitePool,
    request: RecordEvalRun<'_>,
) -> Result<EvalRunDetails> {
    validate_new_eval_run(&request.run)?;
    validate_eval_dimensions(&request.run, &request.dimensions)?;
    validate_eval_links_for_record(&request.run, &request.links)?;
    let normalized_dimensions = normalize_eval_dimensions(&request.run, &request.dimensions)?;

    let resolved_scope = resolve_eval_scope(pool, &request.run).await?;
    validate_link_references(pool, &request.links).await?;

    let mut tx = pool.begin().await?;
    insert_eval_run_tx(&mut tx, &request.run, &resolved_scope).await?;
    insert_eval_dimensions_tx(&mut tx, request.run.id, &normalized_dimensions).await?;
    insert_eval_links_tx(&mut tx, &request.links).await?;
    emit_eval_created_event_tx(
        &mut tx,
        &request.run,
        &resolved_scope,
        &normalized_dimensions,
    )
    .await?;
    tx.commit().await?;

    get_eval_run_details(pool, request.run.id)
        .await?
        .ok_or_else(|| {
            anyhow!(
                "Failed to retrieve eval run '{}' after recording",
                request.run.id
            )
        })
}

pub async fn get_eval_run_details(pool: &SqlitePool, id: &str) -> Result<Option<EvalRunDetails>> {
    let Some(run) = get_eval_run(pool, id).await? else {
        return Ok(None);
    };

    let dimensions = list_eval_scorecard_dimensions(pool, id).await?;
    let links = list_eval_run_links(pool, id).await?;

    Ok(Some(EvalRunDetails {
        run,
        dimensions,
        links,
    }))
}

pub async fn list_eval_run_details(
    pool: &SqlitePool,
    filters: EvalRunFilters<'_>,
) -> Result<Vec<EvalRunDetails>> {
    let runs = list_eval_runs_with_filters(pool, &filters).await?;
    let mut details = Vec::with_capacity(runs.len());

    for run in runs {
        details.push(EvalRunDetails {
            dimensions: list_eval_scorecard_dimensions(pool, &run.id).await?,
            links: list_eval_run_links(pool, &run.id).await?,
            run,
        });
    }

    Ok(details)
}

pub async fn compare_eval_runs(
    pool: &SqlitePool,
    baseline_eval_id: &str,
    current_eval_id: &str,
) -> Result<EvalRunComparison> {
    let baseline = get_eval_run_details(pool, baseline_eval_id)
        .await?
        .ok_or_else(|| anyhow!("baseline eval '{}' not found", baseline_eval_id))?;
    let current = get_eval_run_details(pool, current_eval_id)
        .await?
        .ok_or_else(|| anyhow!("current eval '{}' not found", current_eval_id))?;

    let comparison = compare_eval_details(&baseline, &current)?;
    emit_eval_compared_event(pool, &baseline, &current, &comparison).await?;
    Ok(comparison)
}

pub async fn compare_eval_run_to_latest_baseline(
    pool: &SqlitePool,
    current_eval_id: &str,
) -> Result<Option<EvalRunComparison>> {
    let Some(current) = get_eval_run(pool, current_eval_id).await? else {
        return Err(anyhow!("current eval '{}' not found", current_eval_id));
    };

    let Some(baseline) = find_latest_baseline_eval(pool, &current).await? else {
        return Ok(None);
    };

    compare_eval_runs(pool, &baseline.id, current_eval_id)
        .await
        .map(Some)
}

#[cfg(test)]
pub async fn create_eval_run(pool: &SqlitePool, new_eval_run: NewEvalRun<'_>) -> Result<EvalRun> {
    validate_new_eval_run(&new_eval_run)?;
    let now = Utc::now().to_rfc3339();
    let metadata_json = serde_json::to_string(&new_eval_run.metadata_json)?;

    sqlx::query(
        "INSERT INTO eval_runs (id, evaluator, target_kind, target_ref, spec, task, artifact_id, summary, rationale, outcome, overall_score, source, metadata_json, created_at) \
         VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)",
    )
    .bind(new_eval_run.id)
    .bind(new_eval_run.evaluator)
    .bind(new_eval_run.target_kind)
    .bind(new_eval_run.target_ref)
    .bind(new_eval_run.spec)
    .bind(new_eval_run.task)
    .bind(new_eval_run.artifact_id)
    .bind(new_eval_run.summary)
    .bind(new_eval_run.rationale)
    .bind(new_eval_run.outcome)
    .bind(new_eval_run.overall_score)
    .bind(new_eval_run.source)
    .bind(metadata_json)
    .bind(now)
    .execute(pool)
    .await?;

    get_eval_run(pool, new_eval_run.id).await?.ok_or_else(|| {
        anyhow!(
            "Failed to retrieve eval run '{}' after creation",
            new_eval_run.id
        )
    })
}

pub async fn get_eval_run(pool: &SqlitePool, id: &str) -> Result<Option<EvalRun>> {
    let row = sqlx::query_as::<_, EvalRunRow>(
        "SELECT id, evaluator, target_kind, target_ref, spec, task, artifact_id, summary, rationale, outcome, overall_score, source, metadata_json, created_at \
         FROM eval_runs WHERE id = ?",
    )
    .bind(id)
    .fetch_optional(pool)
    .await?;

    row.map(map_eval_run).transpose()
}

#[cfg(test)]
pub async fn list_eval_runs(
    pool: &SqlitePool,
    spec_filter: Option<&str>,
    task_filter: Option<&str>,
    artifact_filter: Option<&str>,
    outcome_filter: Option<&str>,
    limit: Option<i64>,
    offset: Option<i64>,
) -> Result<Vec<EvalRun>> {
    let mut qb = sqlx::QueryBuilder::<sqlx::Sqlite>::new(
        "SELECT id, evaluator, target_kind, target_ref, spec, task, artifact_id, summary, rationale, outcome, overall_score, source, metadata_json, created_at FROM eval_runs WHERE 1=1",
    );

    if let Some(spec) = spec_filter {
        qb.push(" AND spec = ");
        qb.push_bind(spec);
    }
    if let Some(task) = task_filter {
        qb.push(" AND task = ");
        qb.push_bind(task);
    }
    if let Some(artifact_id) = artifact_filter {
        qb.push(" AND artifact_id = ");
        qb.push_bind(artifact_id);
    }
    if let Some(outcome) = outcome_filter {
        qb.push(" AND outcome = ");
        qb.push_bind(outcome);
    }

    qb.push(" ORDER BY created_at DESC, id DESC");

    if let Some(limit) = limit {
        qb.push(" LIMIT ");
        qb.push_bind(limit);
        if let Some(offset) = offset {
            qb.push(" OFFSET ");
            qb.push_bind(offset);
        }
    }

    qb.build_query_as::<EvalRunRow>()
        .fetch_all(pool)
        .await?
        .into_iter()
        .map(map_eval_run)
        .collect()
}

async fn list_eval_runs_with_filters(
    pool: &SqlitePool,
    filters: &EvalRunFilters<'_>,
) -> Result<Vec<EvalRun>> {
    let mut qb = sqlx::QueryBuilder::<sqlx::Sqlite>::new(
        "SELECT id, evaluator, target_kind, target_ref, spec, task, artifact_id, summary, rationale, outcome, overall_score, source, metadata_json, created_at FROM eval_runs WHERE 1=1",
    );

    if let Some(spec) = filters.spec {
        qb.push(" AND spec = ");
        qb.push_bind(spec);
    }
    if let Some(task) = filters.task {
        qb.push(" AND task = ");
        qb.push_bind(task);
    }
    if let Some(artifact_id) = filters.artifact_id {
        qb.push(" AND artifact_id = ");
        qb.push_bind(artifact_id);
    }
    if let Some(outcome) = filters.outcome {
        qb.push(" AND outcome = ");
        qb.push_bind(outcome);
    }
    if let Some(evaluator) = filters.evaluator {
        qb.push(" AND evaluator = ");
        qb.push_bind(evaluator);
    }
    if let Some(target_kind) = filters.target_kind {
        qb.push(" AND target_kind = ");
        qb.push_bind(target_kind);
    }
    if let Some(target_ref) = filters.target_ref {
        qb.push(" AND target_ref = ");
        qb.push_bind(target_ref);
    }
    if let Some(source) = filters.source {
        qb.push(" AND source = ");
        qb.push_bind(source);
    }
    if let Some(created_after) = filters.created_after {
        qb.push(" AND created_at >= ");
        qb.push_bind(created_after);
    }
    if let Some(created_before) = filters.created_before {
        qb.push(" AND created_at <= ");
        qb.push_bind(created_before);
    }

    qb.push(" ORDER BY created_at DESC, id DESC");

    if let Some(limit) = filters.limit {
        qb.push(" LIMIT ");
        qb.push_bind(limit);
        if let Some(offset) = filters.offset {
            qb.push(" OFFSET ");
            qb.push_bind(offset);
        }
    }

    qb.build_query_as::<EvalRunRow>()
        .fetch_all(pool)
        .await?
        .into_iter()
        .map(map_eval_run)
        .collect()
}

#[cfg(test)]
pub async fn insert_eval_scorecard_dimensions(
    pool: &SqlitePool,
    dimensions: &[NewEvalScorecardDimension<'_>],
) -> Result<()> {
    for dimension in dimensions {
        validate_new_eval_dimension(dimension)?;
        let details_json = serde_json::to_string(&dimension.details_json)?;
        let now = Utc::now().to_rfc3339();

        sqlx::query(
            "INSERT INTO eval_scorecard_dimensions (eval_run_id, dimension_name, normalized_status, normalized_score, normalized_value, rationale, details_json, created_at) \
             VALUES (?, ?, ?, ?, ?, ?, ?, ?)",
        )
        .bind(dimension.eval_run_id)
        .bind(dimension.dimension_name)
        .bind(dimension.normalized_status)
        .bind(dimension.normalized_score)
        .bind(dimension.normalized_value)
        .bind(dimension.rationale)
        .bind(details_json)
        .bind(now)
        .execute(pool)
        .await?;
    }

    Ok(())
}

pub async fn list_eval_scorecard_dimensions(
    pool: &SqlitePool,
    eval_run_id: &str,
) -> Result<Vec<EvalScorecardDimension>> {
    sqlx::query_as::<_, EvalDimensionRow>(
        "SELECT eval_run_id, dimension_name, normalized_status, normalized_score, normalized_value, rationale, details_json, created_at \
         FROM eval_scorecard_dimensions WHERE eval_run_id = ? ORDER BY dimension_name",
    )
    .bind(eval_run_id)
    .fetch_all(pool)
    .await?
    .into_iter()
    .map(map_eval_dimension)
    .collect()
}

#[cfg(test)]
pub async fn insert_eval_run_links(pool: &SqlitePool, links: &[NewEvalRunLink<'_>]) -> Result<()> {
    for link in links {
        validate_new_eval_link(link)?;
        let now = Utc::now().to_rfc3339();

        sqlx::query(
            "INSERT INTO eval_run_links (eval_run_id, link_kind, link_ref, relation, created_at) VALUES (?, ?, ?, ?, ?)",
        )
        .bind(link.eval_run_id)
        .bind(link.link_kind)
        .bind(link.link_ref)
        .bind(link.relation)
        .bind(now)
        .execute(pool)
        .await?;
    }

    Ok(())
}

pub async fn list_eval_run_links(pool: &SqlitePool, eval_run_id: &str) -> Result<Vec<EvalRunLink>> {
    Ok(sqlx::query_as::<_, EvalLinkRow>(
        "SELECT id, eval_run_id, link_kind, link_ref, relation, created_at FROM eval_run_links WHERE eval_run_id = ? ORDER BY id",
    )
    .bind(eval_run_id)
    .fetch_all(pool)
    .await?
    .into_iter()
    .map(map_eval_link)
    .collect())
}

fn validate_new_eval_run(new_eval_run: &NewEvalRun<'_>) -> Result<()> {
    if new_eval_run.id.trim().is_empty() {
        return Err(anyhow!("eval id is required"));
    }
    if new_eval_run.evaluator.trim().is_empty() {
        return Err(anyhow!("evaluator is required"));
    }
    if new_eval_run.target_kind.trim().is_empty() {
        return Err(anyhow!("target_kind is required"));
    }
    if new_eval_run.target_ref.trim().is_empty() {
        return Err(anyhow!("target_ref is required"));
    }
    if !matches!(
        new_eval_run.target_kind,
        "spec" | "task" | "artifact" | "scope"
    ) {
        return Err(anyhow!(
            "target_kind must be one of: spec, task, artifact, scope"
        ));
    }
    if !matches!(
        new_eval_run.outcome,
        "pass" | "warn" | "fail" | "mixed" | "unknown"
    ) {
        return Err(anyhow!(
            "outcome must be one of: pass, warn, fail, mixed, unknown"
        ));
    }
    if !matches!(new_eval_run.source, "recorded" | "cli" | "mcp") {
        return Err(anyhow!("source must be one of: recorded, cli, mcp"));
    }
    Ok(())
}

fn validate_new_eval_dimension(dimension: &NewEvalScorecardDimension<'_>) -> Result<()> {
    if dimension.eval_run_id.trim().is_empty() {
        return Err(anyhow!("eval_run_id is required"));
    }
    if dimension.dimension_name.trim().is_empty() {
        return Err(anyhow!("dimension_name is required"));
    }
    if dimension.normalized_status.trim().is_empty() {
        return Err(anyhow!("normalized_status is required"));
    }
    if let Some(score) = dimension.normalized_score {
        if !(0.0..=1.0).contains(&score) {
            return Err(anyhow!(
                "normalized_score for '{}' must be between 0.0 and 1.0",
                dimension.dimension_name
            ));
        }
    }
    Ok(())
}

fn validate_new_eval_link(link: &NewEvalRunLink<'_>) -> Result<()> {
    if link.eval_run_id.trim().is_empty() {
        return Err(anyhow!("eval_run_id is required"));
    }
    if link.link_kind.trim().is_empty() {
        return Err(anyhow!("link_kind is required"));
    }
    if link.link_ref.trim().is_empty() {
        return Err(anyhow!("link_ref is required"));
    }
    if link.relation.trim().is_empty() {
        return Err(anyhow!("relation is required"));
    }
    Ok(())
}

fn validate_eval_dimensions(
    run: &NewEvalRun<'_>,
    dimensions: &[NewEvalScorecardDimension<'_>],
) -> Result<()> {
    for dimension in dimensions {
        validate_new_eval_dimension(dimension)?;
        if dimension.eval_run_id != run.id {
            return Err(anyhow!(
                "dimension '{}' must reference eval run '{}'",
                dimension.dimension_name,
                run.id
            ));
        }
    }
    Ok(())
}

fn normalize_eval_dimensions(
    run: &NewEvalRun<'_>,
    dimensions: &[NewEvalScorecardDimension<'_>],
) -> Result<Vec<NormalizedEvalDimension>> {
    let mut seen = std::collections::BTreeSet::new();
    let mut normalized = Vec::with_capacity(dimensions.len());

    for dimension in dimensions {
        let canonical_name = normalize_dimension_name(dimension.dimension_name)?;
        if !seen.insert(canonical_name.clone()) {
            return Err(anyhow!(
                "eval '{}' contains duplicate normalized dimension '{}'",
                run.id,
                canonical_name
            ));
        }

        normalized.push(NormalizedEvalDimension {
            dimension_name: canonical_name,
            normalized_status: normalize_dimension_status(dimension.normalized_status)?.to_string(),
            normalized_score: dimension.normalized_score,
            normalized_value: normalize_dimension_value(dimension.normalized_value),
            rationale: dimension
                .rationale
                .map(str::trim)
                .filter(|v| !v.is_empty())
                .map(str::to_string),
            details_json: dimension.details_json.clone(),
        });
    }

    Ok(normalized)
}

fn normalize_dimension_name(value: &str) -> Result<String> {
    let normalized = value.trim().to_ascii_lowercase().replace([' ', '-'], "_");
    match normalized.as_str() {
        "correctness" => Ok("correctness".to_string()),
        "validation" | "validation_coverage" => Ok("validation_coverage".to_string()),
        "policy" | "policy_compliance" => Ok("policy_compliance".to_string()),
        "risk" | "risk_blast_radius" | "blast_radius" => Ok("risk".to_string()),
        _ => Err(anyhow!(
            "unsupported score dimension '{}' (expected correctness, validation_coverage, policy_compliance, or risk)",
            value
        )),
    }
}

fn normalize_dimension_status(value: &str) -> Result<&'static str> {
    let normalized = value.trim().to_ascii_lowercase().replace([' ', '-'], "_");
    match normalized.as_str() {
        "pass" | "green" | "ok" => Ok("pass"),
        "warn" | "warning" | "yellow" | "mixed" => Ok("warn"),
        "fail" | "failed" | "red" => Ok("fail"),
        "not_applicable" | "na" | "n_a" => Ok("not_applicable"),
        "unknown" | "pending" | "unscored" => Ok("unknown"),
        _ => Err(anyhow!(
            "unsupported normalized_status '{}' (expected pass, warn, fail, not_applicable, or unknown)",
            value
        )),
    }
}

fn normalize_dimension_value(value: Option<&str>) -> Option<String> {
    value
        .map(str::trim)
        .filter(|v| !v.is_empty())
        .map(str::to_string)
}

fn validate_eval_links_for_record(
    run: &NewEvalRun<'_>,
    links: &[NewEvalRunLink<'_>],
) -> Result<()> {
    let mut seen = std::collections::BTreeSet::new();

    for link in links {
        validate_new_eval_link(link)?;
        if link.eval_run_id != run.id {
            return Err(anyhow!(
                "link '{}:{}' must reference eval run '{}'",
                link.link_kind,
                link.link_ref,
                run.id
            ));
        }

        let key = (
            link.link_kind.to_string(),
            link.link_ref.to_string(),
            link.relation.to_string(),
        );
        if !seen.insert(key) {
            return Err(anyhow!(
                "duplicate eval link '{}:{}' with relation '{}'",
                link.link_kind,
                link.link_ref,
                link.relation
            ));
        }
    }

    Ok(())
}

async fn resolve_eval_scope(pool: &SqlitePool, run: &NewEvalRun<'_>) -> Result<ResolvedEvalScope> {
    let mut resolved = ResolvedEvalScope {
        spec: run.spec.map(str::to_string),
        task: run.task.map(str::to_string),
        artifact_id: run.artifact_id.map(str::to_string),
    };

    match run.target_kind {
        "spec" => {
            ensure_spec_exists(pool, run.target_ref).await?;
            merge_scope_value(&mut resolved.spec, run.target_ref, "spec")?;
        }
        "task" => {
            let task = get_task(pool, run.target_ref)
                .await?
                .ok_or_else(|| anyhow!("task '{}' not found", run.target_ref))?;
            merge_scope_value(&mut resolved.task, &task.id, "task")?;
            merge_scope_value(&mut resolved.spec, &task.spec, "spec")?;
        }
        "artifact" => {
            let artifact = get_artifact_scope(pool, run.target_ref)
                .await?
                .ok_or_else(|| anyhow!("artifact '{}' not found", run.target_ref))?;
            merge_scope_value(&mut resolved.artifact_id, run.target_ref, "artifact_id")?;
            if let Some(spec) = artifact.spec.as_deref() {
                merge_scope_value(&mut resolved.spec, spec, "spec")?;
            }
            if let Some(task) = artifact.task.as_deref() {
                merge_scope_value(&mut resolved.task, task, "task")?;
            }
        }
        "scope" => {}
        _ => unreachable!("validate_new_eval_run should reject unsupported target kinds"),
    }

    if let Some(task_id) = resolved.task.clone() {
        let task = get_task(pool, &task_id)
            .await?
            .ok_or_else(|| anyhow!("task '{}' not found", task_id))?;
        merge_scope_value(&mut resolved.spec, &task.spec, "spec")?;
    }

    if let Some(artifact_id) = resolved.artifact_id.clone() {
        let artifact = get_artifact_scope(pool, &artifact_id)
            .await?
            .ok_or_else(|| anyhow!("artifact '{}' not found", artifact_id))?;
        if let Some(spec) = artifact.spec.as_deref() {
            merge_scope_value(&mut resolved.spec, spec, "spec")?;
        }
        if let Some(task) = artifact.task.as_deref() {
            merge_scope_value(&mut resolved.task, task, "task")?;
        }
    }

    if let Some(spec_id) = resolved.spec.as_deref() {
        ensure_spec_exists(pool, spec_id).await?;
    }

    Ok(resolved)
}

fn merge_scope_value(slot: &mut Option<String>, candidate: &str, field: &str) -> Result<()> {
    if let Some(existing) = slot.as_deref() {
        if existing != candidate {
            return Err(anyhow!(
                "eval {} '{}' conflicts with resolved value '{}'",
                field,
                existing,
                candidate
            ));
        }
    } else {
        *slot = Some(candidate.to_string());
    }

    Ok(())
}

async fn ensure_spec_exists(pool: &SqlitePool, spec_id: &str) -> Result<()> {
    if get_spec(pool, spec_id).await?.is_none() {
        return Err(anyhow!("spec '{}' not found", spec_id));
    }
    Ok(())
}

async fn get_artifact_scope(pool: &SqlitePool, artifact_id: &str) -> Result<Option<ArtifactScope>> {
    let row = sqlx::query_as::<_, (Option<String>, Option<String>)>(
        "SELECT spec, task FROM artifacts WHERE id = ?",
    )
    .bind(artifact_id)
    .fetch_optional(pool)
    .await?;

    Ok(row.map(|(spec, task)| ArtifactScope { spec, task }))
}

async fn validate_link_references(pool: &SqlitePool, links: &[NewEvalRunLink<'_>]) -> Result<()> {
    for link in links {
        match link.link_kind {
            "evidence_bundle" => {
                if get_evidence_bundle(pool, link.link_ref).await?.is_none() {
                    return Err(anyhow!("evidence bundle '{}' not found", link.link_ref));
                }
            }
            "validation_run" => {
                if get_validation_run(pool, link.link_ref).await?.is_none() {
                    return Err(anyhow!("validation run '{}' not found", link.link_ref));
                }
            }
            "session" => {
                if get_session(pool, link.link_ref).await?.is_none() {
                    return Err(anyhow!("session '{}' not found", link.link_ref));
                }
            }
            "event" => {
                let event_id = link.link_ref.parse::<i64>().map_err(|_| {
                    anyhow!(
                        "event link_ref '{}' must be a numeric event id",
                        link.link_ref
                    )
                })?;
                let exists =
                    sqlx::query_scalar::<_, i64>("SELECT COUNT(1) FROM events WHERE id = ?")
                        .bind(event_id)
                        .fetch_one(pool)
                        .await?;
                if exists == 0 {
                    return Err(anyhow!("event '{}' not found", link.link_ref));
                }
            }
            "artifact" => {
                if get_artifact_scope(pool, link.link_ref).await?.is_none() {
                    return Err(anyhow!("artifact '{}' not found", link.link_ref));
                }
            }
            "approval" => {
                if get_approval(pool, link.link_ref).await?.is_none() {
                    return Err(anyhow!("approval '{}' not found", link.link_ref));
                }
            }
            "spec" => ensure_spec_exists(pool, link.link_ref).await?,
            "task" => {
                if get_task(pool, link.link_ref).await?.is_none() {
                    return Err(anyhow!("task '{}' not found", link.link_ref));
                }
            }
            "eval_run" => {
                if get_eval_run(pool, link.link_ref).await?.is_none() {
                    return Err(anyhow!("eval run '{}' not found", link.link_ref));
                }
            }
            "custom" => {}
            _ => unreachable!("database constraints should reject unsupported link kinds"),
        }
    }

    Ok(())
}

async fn find_latest_baseline_eval(
    pool: &SqlitePool,
    current: &EvalRun,
) -> Result<Option<EvalRun>> {
    let row = sqlx::query_as::<_, EvalRunRow>(
        "SELECT id, evaluator, target_kind, target_ref, spec, task, artifact_id, summary, rationale, outcome, overall_score, source, metadata_json, created_at \
         FROM eval_runs \
         WHERE target_kind = ? AND target_ref = ? AND ((created_at < ?) OR (created_at = ? AND id < ?)) \
         ORDER BY created_at DESC, id DESC LIMIT 1",
    )
    .bind(&current.target_kind)
    .bind(&current.target_ref)
    .bind(&current.created_at)
    .bind(&current.created_at)
    .bind(&current.id)
    .fetch_optional(pool)
    .await?;

    row.map(map_eval_run).transpose()
}

fn compare_eval_details(
    baseline: &EvalRunDetails,
    current: &EvalRunDetails,
) -> Result<EvalRunComparison> {
    if baseline.run.target_kind != current.run.target_kind
        || baseline.run.target_ref != current.run.target_ref
    {
        return Err(anyhow!(
            "cannot compare eval '{}' against '{}' because they target different scopes",
            current.run.id,
            baseline.run.id
        ));
    }

    let baseline_dimensions = baseline
        .dimensions
        .iter()
        .map(|dimension| (dimension.dimension_name.as_str(), dimension))
        .collect::<std::collections::BTreeMap<_, _>>();
    let current_dimensions = current
        .dimensions
        .iter()
        .map(|dimension| (dimension.dimension_name.as_str(), dimension))
        .collect::<std::collections::BTreeMap<_, _>>();

    let dimension_names = baseline_dimensions
        .keys()
        .chain(current_dimensions.keys())
        .copied()
        .collect::<std::collections::BTreeSet<_>>();

    let mut comparisons = Vec::with_capacity(dimension_names.len());
    let mut cumulative_delta = 0.0;

    for dimension_name in dimension_names {
        let baseline_dimension = baseline_dimensions.get(dimension_name).copied();
        let current_dimension = current_dimensions.get(dimension_name).copied();
        let baseline_merit = dimension_merit(baseline_dimension);
        let current_merit = dimension_merit(current_dimension);
        let delta = current_merit - baseline_merit;
        cumulative_delta += delta;

        comparisons.push(EvalDimensionComparison {
            dimension_name: dimension_name.to_string(),
            baseline_status: baseline_dimension
                .map(|dimension| dimension.normalized_status.clone()),
            current_status: current_dimension.map(|dimension| dimension.normalized_status.clone()),
            baseline_score: baseline_dimension.and_then(|dimension| dimension.normalized_score),
            current_score: current_dimension.and_then(|dimension| dimension.normalized_score),
            score_delta: match (baseline_dimension, current_dimension) {
                (Some(baseline_dimension), Some(current_dimension)) => {
                    match (
                        baseline_dimension.normalized_score,
                        current_dimension.normalized_score,
                    ) {
                        (Some(baseline_score), Some(current_score)) => {
                            Some(current_score - baseline_score)
                        }
                        _ => None,
                    }
                }
                _ => None,
            },
            classification: classify_delta(delta).to_string(),
        });
    }

    let baseline_overall = baseline
        .run
        .overall_score
        .unwrap_or_else(|| average_dimension_merit(&baseline.dimensions));
    let current_overall = current
        .run
        .overall_score
        .unwrap_or_else(|| average_dimension_merit(&current.dimensions));
    let overall_delta = current_overall - baseline_overall;
    let overall_classification = if overall_delta.abs() > 0.001 {
        classify_delta(overall_delta)
    } else {
        classify_delta(cumulative_delta)
    };

    Ok(EvalRunComparison {
        baseline_eval_id: baseline.run.id.clone(),
        current_eval_id: current.run.id.clone(),
        comparison_group: format!("{}:{}", current.run.target_kind, current.run.target_ref),
        overall_classification: overall_classification.to_string(),
        overall_score_delta: Some(overall_delta),
        dimensions: comparisons,
    })
}

fn average_dimension_merit(dimensions: &[EvalScorecardDimension]) -> f64 {
    if dimensions.is_empty() {
        return status_merit("unknown");
    }

    dimensions
        .iter()
        .map(|dimension| dimension_merit(Some(dimension)))
        .sum::<f64>()
        / dimensions.len() as f64
}

fn dimension_merit(dimension: Option<&EvalScorecardDimension>) -> f64 {
    match dimension {
        Some(dimension) => dimension
            .normalized_score
            .unwrap_or_else(|| status_merit(&dimension.normalized_status)),
        None => status_merit("unknown"),
    }
}

fn status_merit(status: &str) -> f64 {
    match status {
        "pass" => 1.0,
        "warn" => 0.65,
        "unknown" => 0.5,
        "not_applicable" => 0.5,
        "fail" => 0.0,
        _ => 0.5,
    }
}

fn classify_delta(delta: f64) -> &'static str {
    if delta > 0.001 {
        "improved"
    } else if delta < -0.001 {
        "regressed"
    } else {
        "unchanged"
    }
}

async fn emit_eval_created_event_tx(
    tx: &mut Transaction<'_, Sqlite>,
    run: &NewEvalRun<'_>,
    scope: &ResolvedEvalScope,
    dimensions: &[NormalizedEvalDimension],
) -> Result<()> {
    let payload = serde_json::json!({
        "eval_id": run.id,
        "target_kind": run.target_kind,
        "target_ref": run.target_ref,
        "spec": scope.spec,
        "task": scope.task,
        "artifact_id": scope.artifact_id,
        "outcome": run.outcome,
        "overall_score": run.overall_score,
        "source": run.source,
        "dimension_count": dimensions.len(),
        "dimensions": dimensions.iter().map(|dimension| serde_json::json!({
            "name": dimension.dimension_name,
            "status": dimension.normalized_status,
            "score": dimension.normalized_score,
        })).collect::<Vec<_>>()
    });

    emit_event_tx(
        tx,
        "EvalCreated",
        scope.spec.as_deref(),
        Some(run.evaluator),
        &payload.to_string(),
    )
    .await
}

async fn emit_eval_compared_event(
    pool: &SqlitePool,
    baseline: &EvalRunDetails,
    current: &EvalRunDetails,
    comparison: &EvalRunComparison,
) -> Result<()> {
    let payload = serde_json::json!({
        "baseline_eval_id": comparison.baseline_eval_id,
        "current_eval_id": comparison.current_eval_id,
        "comparison_group": comparison.comparison_group,
        "overall_classification": comparison.overall_classification,
        "overall_score_delta": comparison.overall_score_delta,
        "target_kind": current.run.target_kind,
        "target_ref": current.run.target_ref,
        "baseline_created_at": baseline.run.created_at,
        "current_created_at": current.run.created_at,
        "dimensions": comparison.dimensions.iter().map(|dimension| serde_json::json!({
            "name": dimension.dimension_name,
            "classification": dimension.classification,
            "score_delta": dimension.score_delta,
            "baseline_status": dimension.baseline_status,
            "current_status": dimension.current_status,
        })).collect::<Vec<_>>()
    });

    emit_event(
        pool,
        "EvalCompared",
        current.run.spec.as_deref(),
        Some(current.run.evaluator.as_str()),
        &payload.to_string(),
    )
    .await
}

async fn insert_eval_run_tx(
    tx: &mut Transaction<'_, Sqlite>,
    run: &NewEvalRun<'_>,
    scope: &ResolvedEvalScope,
) -> Result<()> {
    let now = Utc::now().to_rfc3339();
    let metadata_json = serde_json::to_string(&run.metadata_json)?;

    sqlx::query(
        "INSERT INTO eval_runs (id, evaluator, target_kind, target_ref, spec, task, artifact_id, summary, rationale, outcome, overall_score, source, metadata_json, created_at) \
         VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)",
    )
    .bind(run.id)
    .bind(run.evaluator)
    .bind(run.target_kind)
    .bind(run.target_ref)
    .bind(scope.spec.as_deref())
    .bind(scope.task.as_deref())
    .bind(scope.artifact_id.as_deref())
    .bind(run.summary)
    .bind(run.rationale)
    .bind(run.outcome)
    .bind(run.overall_score)
    .bind(run.source)
    .bind(metadata_json)
    .bind(now)
    .execute(&mut **tx)
    .await?;

    Ok(())
}

async fn insert_eval_dimensions_tx(
    tx: &mut Transaction<'_, Sqlite>,
    eval_run_id: &str,
    dimensions: &[NormalizedEvalDimension],
) -> Result<()> {
    for dimension in dimensions {
        let now = Utc::now().to_rfc3339();
        let details_json = serde_json::to_string(&dimension.details_json)?;

        sqlx::query(
            "INSERT INTO eval_scorecard_dimensions (eval_run_id, dimension_name, normalized_status, normalized_score, normalized_value, rationale, details_json, created_at) \
             VALUES (?, ?, ?, ?, ?, ?, ?, ?)",
        )
        .bind(eval_run_id)
        .bind(&dimension.dimension_name)
        .bind(&dimension.normalized_status)
        .bind(dimension.normalized_score)
        .bind(&dimension.normalized_value)
        .bind(&dimension.rationale)
        .bind(details_json)
        .bind(now)
        .execute(&mut **tx)
        .await?;
    }

    Ok(())
}

async fn insert_eval_links_tx(
    tx: &mut Transaction<'_, Sqlite>,
    links: &[NewEvalRunLink<'_>],
) -> Result<()> {
    for link in links {
        let now = Utc::now().to_rfc3339();

        sqlx::query(
            "INSERT INTO eval_run_links (eval_run_id, link_kind, link_ref, relation, created_at) VALUES (?, ?, ?, ?, ?)",
        )
        .bind(link.eval_run_id)
        .bind(link.link_kind)
        .bind(link.link_ref)
        .bind(link.relation)
        .bind(now)
        .execute(&mut **tx)
        .await?;
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::sdd::{
        artifact::register_artifact, event::query_events, spec::create_spec, task::create_task,
        test_helpers::make_pool,
    };

    async fn seed_scope(pool: &SqlitePool) {
        create_spec(pool, "SPEC-EVAL", "Eval spec", "P1", &[])
            .await
            .unwrap();
        create_task(
            pool,
            "TASK-EVAL",
            "SPEC-EVAL",
            "Eval task",
            "builder",
            &[],
            None,
        )
        .await
        .unwrap();
        register_artifact(
            pool,
            "ART-EVAL",
            Some("SPEC-EVAL"),
            Some("TASK-EVAL"),
            "builder",
            "test",
            Some("tests/evals.rs"),
            Some("Eval test artifact"),
            None,
        )
        .await
        .unwrap();
    }

    #[tokio::test]
    async fn create_and_get_eval_run_round_trip() {
        let pool = make_pool().await;
        seed_scope(&pool).await;

        let created = create_eval_run(
            &pool,
            NewEvalRun {
                id: "eval-001",
                evaluator: "reviewer",
                target_kind: "task",
                target_ref: "TASK-EVAL",
                spec: Some("SPEC-EVAL"),
                task: Some("TASK-EVAL"),
                artifact_id: Some("ART-EVAL"),
                summary: Some("Looks good"),
                rationale: Some("All dimensions pass"),
                outcome: "pass",
                overall_score: Some(0.95),
                source: "recorded",
                metadata_json: serde_json::json!({"judge":"human"}),
            },
        )
        .await
        .unwrap();

        assert_eq!(created.id, "eval-001");
        assert_eq!(created.target_kind, "task");
        assert_eq!(created.task.as_deref(), Some("TASK-EVAL"));
        assert_eq!(created.artifact_id.as_deref(), Some("ART-EVAL"));

        let fetched = get_eval_run(&pool, "eval-001").await.unwrap().unwrap();
        assert_eq!(fetched.metadata_json["judge"], "human");
    }

    #[tokio::test]
    async fn list_eval_runs_applies_filters() {
        let pool = make_pool().await;
        seed_scope(&pool).await;

        create_eval_run(
            &pool,
            NewEvalRun {
                id: "eval-002",
                evaluator: "reviewer",
                target_kind: "task",
                target_ref: "TASK-EVAL",
                spec: Some("SPEC-EVAL"),
                task: Some("TASK-EVAL"),
                artifact_id: None,
                summary: None,
                rationale: None,
                outcome: "pass",
                overall_score: None,
                source: "cli",
                metadata_json: serde_json::json!({}),
            },
        )
        .await
        .unwrap();

        create_eval_run(
            &pool,
            NewEvalRun {
                id: "eval-003",
                evaluator: "reviewer",
                target_kind: "spec",
                target_ref: "SPEC-EVAL",
                spec: Some("SPEC-EVAL"),
                task: None,
                artifact_id: None,
                summary: None,
                rationale: None,
                outcome: "warn",
                overall_score: None,
                source: "mcp",
                metadata_json: serde_json::json!({}),
            },
        )
        .await
        .unwrap();

        let task_results = list_eval_runs(&pool, None, Some("TASK-EVAL"), None, None, None, None)
            .await
            .unwrap();
        assert_eq!(task_results.len(), 1);
        assert_eq!(task_results[0].id, "eval-002");

        let warn_results = list_eval_runs(&pool, None, None, None, Some("warn"), None, None)
            .await
            .unwrap();
        assert_eq!(warn_results.len(), 1);
        assert_eq!(warn_results[0].id, "eval-003");
    }

    #[tokio::test]
    async fn insert_and_list_dimensions_and_links() {
        let pool = make_pool().await;
        seed_scope(&pool).await;

        create_eval_run(
            &pool,
            NewEvalRun {
                id: "eval-004",
                evaluator: "reviewer",
                target_kind: "artifact",
                target_ref: "ART-EVAL",
                spec: Some("SPEC-EVAL"),
                task: Some("TASK-EVAL"),
                artifact_id: Some("ART-EVAL"),
                summary: None,
                rationale: None,
                outcome: "mixed",
                overall_score: Some(0.7),
                source: "recorded",
                metadata_json: serde_json::json!({}),
            },
        )
        .await
        .unwrap();

        insert_eval_scorecard_dimensions(
            &pool,
            &[
                NewEvalScorecardDimension {
                    eval_run_id: "eval-004",
                    dimension_name: "correctness",
                    normalized_status: "pass",
                    normalized_score: Some(0.9),
                    normalized_value: Some("green"),
                    rationale: Some("No issues found"),
                    details_json: serde_json::json!({"notes":1}),
                },
                NewEvalScorecardDimension {
                    eval_run_id: "eval-004",
                    dimension_name: "risk",
                    normalized_status: "warn",
                    normalized_score: Some(0.5),
                    normalized_value: None,
                    rationale: Some("Moderate blast radius"),
                    details_json: serde_json::json!({}),
                },
            ],
        )
        .await
        .unwrap();

        insert_eval_run_links(
            &pool,
            &[
                NewEvalRunLink {
                    eval_run_id: "eval-004",
                    link_kind: "artifact",
                    link_ref: "ART-EVAL",
                    relation: "subject",
                },
                NewEvalRunLink {
                    eval_run_id: "eval-004",
                    link_kind: "task",
                    link_ref: "TASK-EVAL",
                    relation: "context",
                },
            ],
        )
        .await
        .unwrap();

        let dimensions = list_eval_scorecard_dimensions(&pool, "eval-004")
            .await
            .unwrap();
        assert_eq!(dimensions.len(), 2);
        assert_eq!(dimensions[0].dimension_name, "correctness");

        let links = list_eval_run_links(&pool, "eval-004").await.unwrap();
        assert_eq!(links.len(), 2);
        assert_eq!(links[0].link_kind, "artifact");
    }

    #[tokio::test]
    async fn record_eval_run_resolves_scope_and_returns_details() {
        let pool = make_pool().await;
        seed_scope(&pool).await;

        let recorded = record_eval_run(
            &pool,
            RecordEvalRun {
                run: NewEvalRun {
                    id: "eval-005",
                    evaluator: "reviewer",
                    target_kind: "task",
                    target_ref: "TASK-EVAL",
                    spec: None,
                    task: None,
                    artifact_id: None,
                    summary: Some("Structured review"),
                    rationale: Some("Task-scoped eval"),
                    outcome: "pass",
                    overall_score: Some(0.88),
                    source: "recorded",
                    metadata_json: serde_json::json!({"round":1}),
                },
                dimensions: vec![NewEvalScorecardDimension {
                    eval_run_id: "eval-005",
                    dimension_name: "correctness",
                    normalized_status: "green",
                    normalized_score: Some(0.88),
                    normalized_value: Some("green"),
                    rationale: Some("Matches expected output"),
                    details_json: serde_json::json!({"checks":3}),
                }],
                links: vec![
                    NewEvalRunLink {
                        eval_run_id: "eval-005",
                        link_kind: "task",
                        link_ref: "TASK-EVAL",
                        relation: "subject",
                    },
                    NewEvalRunLink {
                        eval_run_id: "eval-005",
                        link_kind: "artifact",
                        link_ref: "ART-EVAL",
                        relation: "context",
                    },
                ],
            },
        )
        .await
        .unwrap();

        assert_eq!(recorded.run.spec.as_deref(), Some("SPEC-EVAL"));
        assert_eq!(recorded.run.task.as_deref(), Some("TASK-EVAL"));
        assert_eq!(recorded.dimensions.len(), 1);
        assert_eq!(recorded.links.len(), 2);
        assert_eq!(recorded.dimensions[0].normalized_status, "pass");
    }

    #[tokio::test]
    async fn record_eval_run_rejects_missing_scope_without_partial_persistence() {
        let pool = make_pool().await;
        seed_scope(&pool).await;

        let err = record_eval_run(
            &pool,
            RecordEvalRun {
                run: NewEvalRun {
                    id: "eval-missing",
                    evaluator: "reviewer",
                    target_kind: "task",
                    target_ref: "TASK-DOES-NOT-EXIST",
                    spec: None,
                    task: None,
                    artifact_id: None,
                    summary: None,
                    rationale: None,
                    outcome: "fail",
                    overall_score: None,
                    source: "cli",
                    metadata_json: serde_json::json!({}),
                },
                dimensions: vec![],
                links: vec![],
            },
        )
        .await
        .unwrap_err();

        assert!(err.to_string().contains("not found"));
        assert!(get_eval_run(&pool, "eval-missing").await.unwrap().is_none());
    }

    #[tokio::test]
    async fn record_eval_run_rejects_invalid_link_without_partial_persistence() {
        let pool = make_pool().await;
        seed_scope(&pool).await;

        let err = record_eval_run(
            &pool,
            RecordEvalRun {
                run: NewEvalRun {
                    id: "eval-bad-link",
                    evaluator: "reviewer",
                    target_kind: "spec",
                    target_ref: "SPEC-EVAL",
                    spec: Some("SPEC-EVAL"),
                    task: None,
                    artifact_id: None,
                    summary: None,
                    rationale: None,
                    outcome: "warn",
                    overall_score: None,
                    source: "mcp",
                    metadata_json: serde_json::json!({}),
                },
                dimensions: vec![],
                links: vec![NewEvalRunLink {
                    eval_run_id: "eval-bad-link",
                    link_kind: "session",
                    link_ref: "sess-missing",
                    relation: "context",
                }],
            },
        )
        .await
        .unwrap_err();

        assert!(err.to_string().contains("session 'sess-missing' not found"));
        assert!(get_eval_run(&pool, "eval-bad-link")
            .await
            .unwrap()
            .is_none());
    }

    #[tokio::test]
    async fn list_eval_run_details_applies_extended_filters() {
        let pool = make_pool().await;
        seed_scope(&pool).await;

        record_eval_run(
            &pool,
            RecordEvalRun {
                run: NewEvalRun {
                    id: "eval-006",
                    evaluator: "reviewer",
                    target_kind: "task",
                    target_ref: "TASK-EVAL",
                    spec: None,
                    task: None,
                    artifact_id: None,
                    summary: None,
                    rationale: None,
                    outcome: "pass",
                    overall_score: None,
                    source: "recorded",
                    metadata_json: serde_json::json!({}),
                },
                dimensions: vec![],
                links: vec![],
            },
        )
        .await
        .unwrap();

        record_eval_run(
            &pool,
            RecordEvalRun {
                run: NewEvalRun {
                    id: "eval-007",
                    evaluator: "reviewer",
                    target_kind: "spec",
                    target_ref: "SPEC-EVAL",
                    spec: Some("SPEC-EVAL"),
                    task: None,
                    artifact_id: None,
                    summary: None,
                    rationale: None,
                    outcome: "warn",
                    overall_score: None,
                    source: "cli",
                    metadata_json: serde_json::json!({}),
                },
                dimensions: vec![],
                links: vec![],
            },
        )
        .await
        .unwrap();

        let filtered = list_eval_run_details(
            &pool,
            EvalRunFilters {
                target_kind: Some("task"),
                outcome: Some("pass"),
                ..EvalRunFilters::default()
            },
        )
        .await
        .unwrap();

        assert_eq!(filtered.len(), 1);
        assert_eq!(filtered[0].run.id, "eval-006");
    }

    #[tokio::test]
    async fn record_eval_run_rejects_unsupported_dimension() {
        let pool = make_pool().await;
        seed_scope(&pool).await;

        let err = record_eval_run(
            &pool,
            RecordEvalRun {
                run: NewEvalRun {
                    id: "eval-unsupported-dimension",
                    evaluator: "reviewer",
                    target_kind: "task",
                    target_ref: "TASK-EVAL",
                    spec: None,
                    task: None,
                    artifact_id: None,
                    summary: None,
                    rationale: None,
                    outcome: "warn",
                    overall_score: None,
                    source: "recorded",
                    metadata_json: serde_json::json!({}),
                },
                dimensions: vec![NewEvalScorecardDimension {
                    eval_run_id: "eval-unsupported-dimension",
                    dimension_name: "latency",
                    normalized_status: "pass",
                    normalized_score: Some(0.8),
                    normalized_value: None,
                    rationale: None,
                    details_json: serde_json::json!({}),
                }],
                links: vec![],
            },
        )
        .await
        .unwrap_err();

        assert!(err.to_string().contains("unsupported score dimension"));
    }

    #[tokio::test]
    async fn compare_eval_runs_reports_improvement_and_latest_baseline() {
        let pool = make_pool().await;
        seed_scope(&pool).await;

        record_eval_run(
            &pool,
            RecordEvalRun {
                run: NewEvalRun {
                    id: "eval-baseline",
                    evaluator: "reviewer",
                    target_kind: "task",
                    target_ref: "TASK-EVAL",
                    spec: None,
                    task: None,
                    artifact_id: None,
                    summary: None,
                    rationale: None,
                    outcome: "warn",
                    overall_score: Some(0.45),
                    source: "recorded",
                    metadata_json: serde_json::json!({}),
                },
                dimensions: vec![NewEvalScorecardDimension {
                    eval_run_id: "eval-baseline",
                    dimension_name: "validation",
                    normalized_status: "warn",
                    normalized_score: Some(0.45),
                    normalized_value: None,
                    rationale: None,
                    details_json: serde_json::json!({}),
                }],
                links: vec![],
            },
        )
        .await
        .unwrap();

        record_eval_run(
            &pool,
            RecordEvalRun {
                run: NewEvalRun {
                    id: "eval-current",
                    evaluator: "reviewer",
                    target_kind: "task",
                    target_ref: "TASK-EVAL",
                    spec: None,
                    task: None,
                    artifact_id: None,
                    summary: None,
                    rationale: None,
                    outcome: "pass",
                    overall_score: Some(0.91),
                    source: "recorded",
                    metadata_json: serde_json::json!({}),
                },
                dimensions: vec![NewEvalScorecardDimension {
                    eval_run_id: "eval-current",
                    dimension_name: "validation_coverage",
                    normalized_status: "pass",
                    normalized_score: Some(0.91),
                    normalized_value: None,
                    rationale: None,
                    details_json: serde_json::json!({}),
                }],
                links: vec![],
            },
        )
        .await
        .unwrap();

        let comparison = compare_eval_runs(&pool, "eval-baseline", "eval-current")
            .await
            .unwrap();
        assert_eq!(comparison.overall_classification, "improved");
        assert_eq!(
            comparison.dimensions[0].dimension_name,
            "validation_coverage"
        );
        assert_eq!(comparison.dimensions[0].classification, "improved");

        let latest_baseline = compare_eval_run_to_latest_baseline(&pool, "eval-current")
            .await
            .unwrap()
            .unwrap();
        assert_eq!(latest_baseline.baseline_eval_id, "eval-baseline");

        let comparison_events = query_events(
            &pool,
            Some("EvalCompared"),
            Some("SPEC-EVAL"),
            None,
            None,
            None,
            None,
            None,
        )
        .await
        .unwrap();
        assert!(!comparison_events.is_empty());
        assert!(comparison_events[0].payload.contains("eval-current"));
    }

    #[tokio::test]
    async fn compare_eval_runs_rejects_mismatched_scope() {
        let pool = make_pool().await;
        seed_scope(&pool).await;

        record_eval_run(
            &pool,
            RecordEvalRun {
                run: NewEvalRun {
                    id: "eval-task-scope",
                    evaluator: "reviewer",
                    target_kind: "task",
                    target_ref: "TASK-EVAL",
                    spec: None,
                    task: None,
                    artifact_id: None,
                    summary: None,
                    rationale: None,
                    outcome: "warn",
                    overall_score: None,
                    source: "recorded",
                    metadata_json: serde_json::json!({}),
                },
                dimensions: vec![],
                links: vec![],
            },
        )
        .await
        .unwrap();

        record_eval_run(
            &pool,
            RecordEvalRun {
                run: NewEvalRun {
                    id: "eval-spec-scope",
                    evaluator: "reviewer",
                    target_kind: "spec",
                    target_ref: "SPEC-EVAL",
                    spec: Some("SPEC-EVAL"),
                    task: None,
                    artifact_id: None,
                    summary: None,
                    rationale: None,
                    outcome: "pass",
                    overall_score: None,
                    source: "recorded",
                    metadata_json: serde_json::json!({}),
                },
                dimensions: vec![],
                links: vec![],
            },
        )
        .await
        .unwrap();

        let err = compare_eval_runs(&pool, "eval-task-scope", "eval-spec-scope")
            .await
            .unwrap_err();
        assert!(err.to_string().contains("different scopes"));
    }

    #[tokio::test]
    async fn record_eval_run_emits_eval_created_event() {
        let pool = make_pool().await;
        seed_scope(&pool).await;

        record_eval_run(
            &pool,
            RecordEvalRun {
                run: NewEvalRun {
                    id: "eval-created-event",
                    evaluator: "reviewer",
                    target_kind: "task",
                    target_ref: "TASK-EVAL",
                    spec: None,
                    task: None,
                    artifact_id: None,
                    summary: Some("event check"),
                    rationale: Some("ensure audit event exists"),
                    outcome: "pass",
                    overall_score: Some(0.9),
                    source: "recorded",
                    metadata_json: serde_json::json!({}),
                },
                dimensions: vec![NewEvalScorecardDimension {
                    eval_run_id: "eval-created-event",
                    dimension_name: "correctness",
                    normalized_status: "pass",
                    normalized_score: Some(0.9),
                    normalized_value: None,
                    rationale: None,
                    details_json: serde_json::json!({}),
                }],
                links: vec![],
            },
        )
        .await
        .unwrap();

        let events = query_events(
            &pool,
            Some("EvalCreated"),
            Some("SPEC-EVAL"),
            Some("reviewer"),
            None,
            None,
            None,
            None,
        )
        .await
        .unwrap();
        assert!(!events.is_empty());
        assert!(events[0].payload.contains("eval-created-event"));
        assert!(events[0].payload.contains("correctness"));
    }
}
