use anyhow::{bail, Context, Result};
use chrono::Utc;
use colored::Colorize;
use serde::Deserialize;
use serde_json::{json, Value};
use sqlx::SqlitePool;
use std::fs;

use crate::sdd::evals::{
    compare_eval_run_to_latest_baseline, compare_eval_runs, get_eval_run_details,
    list_eval_run_details, record_eval_run, EvalRunComparison, EvalRunDetails, EvalRunFilters,
    NewEvalRun, NewEvalRunLink, NewEvalScorecardDimension, RecordEvalRun,
};

pub struct EvalCreateOptions<'a> {
    pub id: Option<&'a str>,
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
    pub metadata_json: Option<&'a str>,
    pub dimensions_json: Option<&'a str>,
    pub dimensions_file: Option<&'a str>,
    pub links_json: Option<&'a str>,
    pub links_file: Option<&'a str>,
    pub json: bool,
}

pub struct EvalListOptions<'a> {
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
    pub json: bool,
}

pub struct EvalCompareOptions<'a> {
    pub baseline_id: Option<&'a str>,
    pub current_id: &'a str,
    pub latest_baseline: bool,
    pub json: bool,
}

#[derive(Debug, Deserialize)]
struct CliEvalDimension {
    name: String,
    status: String,
    #[serde(default)]
    score: Option<f64>,
    #[serde(default)]
    value: Option<String>,
    #[serde(default)]
    rationale: Option<String>,
    #[serde(default)]
    details: Option<Value>,
}

#[derive(Debug, Deserialize)]
struct CliEvalLink {
    kind: String,
    #[serde(rename = "ref")]
    link_ref: String,
    #[serde(default = "default_link_relation")]
    relation: String,
}

pub async fn cmd_eval_create(pool: &SqlitePool, options: EvalCreateOptions<'_>) -> Result<()> {
    let generated_id = options
        .id
        .map(str::to_string)
        .unwrap_or_else(|| format!("eval-{}", Utc::now().timestamp_nanos_opt().unwrap_or(0)));
    let metadata_json = load_optional_json_object(options.metadata_json, "metadata_json")?
        .unwrap_or_else(|| json!({}));
    let dimensions = load_eval_dimensions(options.dimensions_json, options.dimensions_file)?;
    let links = load_eval_links(options.links_json, options.links_file)?;

    let dimension_inputs = dimensions
        .iter()
        .map(|dimension| NewEvalScorecardDimension {
            eval_run_id: &generated_id,
            dimension_name: &dimension.name,
            normalized_status: &dimension.status,
            normalized_score: dimension.score,
            normalized_value: dimension.value.as_deref(),
            rationale: dimension.rationale.as_deref(),
            details_json: dimension.details.clone().unwrap_or_else(|| json!({})),
        })
        .collect::<Vec<_>>();
    let link_inputs = links
        .iter()
        .map(|link| NewEvalRunLink {
            eval_run_id: &generated_id,
            link_kind: &link.kind,
            link_ref: &link.link_ref,
            relation: &link.relation,
        })
        .collect::<Vec<_>>();

    let recorded = record_eval_run(
        pool,
        RecordEvalRun {
            run: NewEvalRun {
                id: &generated_id,
                evaluator: options.evaluator,
                target_kind: options.target_kind,
                target_ref: options.target_ref,
                spec: options.spec,
                task: options.task,
                artifact_id: options.artifact_id,
                summary: options.summary,
                rationale: options.rationale,
                outcome: options.outcome,
                overall_score: options.overall_score,
                source: options.source,
                metadata_json,
            },
            dimensions: dimension_inputs,
            links: link_inputs,
        },
    )
    .await?;

    if options.json {
        println!("{}", serde_json::to_string_pretty(&recorded)?);
        return Ok(());
    }

    println!(
        "{} Eval {} created",
        "✓".green().bold(),
        recorded.run.id.cyan()
    );
    print_eval_details_human(&recorded);
    Ok(())
}

pub async fn cmd_eval_list(pool: &SqlitePool, options: EvalListOptions<'_>) -> Result<()> {
    let details = list_eval_run_details(
        pool,
        EvalRunFilters {
            spec: options.spec,
            task: options.task,
            artifact_id: options.artifact_id,
            outcome: options.outcome,
            evaluator: options.evaluator,
            target_kind: options.target_kind,
            target_ref: options.target_ref,
            source: options.source,
            created_after: options.created_after,
            created_before: options.created_before,
            limit: options.limit,
            offset: options.offset,
        },
    )
    .await?;

    if options.json {
        println!("{}", serde_json::to_string_pretty(&details)?);
        return Ok(());
    }

    if details.is_empty() {
        println!("{}", "No evals found.".dimmed());
        return Ok(());
    }

    println!(
        "{:<22} {:<16} {:<18} {:<10} {:<10} {:<10} {}",
        "Eval ID".bold(),
        "Evaluator".bold(),
        "Target".bold(),
        "Outcome".bold(),
        "Score".bold(),
        "Dims".bold(),
        "Created".bold()
    );
    println!("{}", "─".repeat(110).dimmed());

    for detail in &details {
        println!(
            "{:<22} {:<16} {:<18} {:<10} {:<10} {:<10} {}",
            truncate(&detail.run.id, 22).cyan(),
            truncate(&detail.run.evaluator, 16),
            truncate(
                &format!("{}:{}", detail.run.target_kind, detail.run.target_ref),
                18
            ),
            detail.run.outcome.as_str(),
            format_score(detail.run.overall_score),
            detail.dimensions.len(),
            format_timestamp(&detail.run.created_at).dimmed()
        );
    }

    Ok(())
}

pub async fn cmd_eval_show(pool: &SqlitePool, id: &str, json_output: bool) -> Result<()> {
    let details = get_eval_run_details(pool, id)
        .await?
        .ok_or_else(|| anyhow::anyhow!("eval '{}' not found", id))?;

    if json_output {
        println!("{}", serde_json::to_string_pretty(&details)?);
        return Ok(());
    }

    print_eval_details_human(&details);
    Ok(())
}

pub async fn cmd_eval_compare(pool: &SqlitePool, options: EvalCompareOptions<'_>) -> Result<()> {
    if options.latest_baseline && options.baseline_id.is_some() {
        bail!("pass either --baseline-id or --latest-baseline, not both");
    }

    let comparison = match (options.baseline_id, options.latest_baseline) {
        (Some(baseline_id), false) => compare_eval_runs(pool, baseline_id, options.current_id).await?,
        (None, true) => compare_eval_run_to_latest_baseline(pool, options.current_id)
            .await?
            .ok_or_else(|| anyhow::anyhow!("no earlier baseline eval found for '{}'", options.current_id))?,
        (Some(_), true) => unreachable!("validated above"),
        (None, false) => bail!("pass either --baseline-id or --latest-baseline"),
    };

    if options.json {
        println!("{}", serde_json::to_string_pretty(&comparison)?);
        return Ok(());
    }

    print_eval_comparison_human(&comparison);
    Ok(())
}

fn print_eval_details_human(details: &EvalRunDetails) {
    println!("{}", format!("═══ Eval {} ═══", details.run.id).cyan());
    println!("  Evaluator: {}", details.run.evaluator);
    println!(
        "  Target:    {}:{}",
        details.run.target_kind, details.run.target_ref
    );
    println!("  Outcome:   {}", details.run.outcome);
    println!("  Score:     {}", format_score(details.run.overall_score));
    println!("  Source:    {}", details.run.source);
    println!("  Created:   {}", details.run.created_at);
    if let Some(spec) = details.run.spec.as_deref() {
        println!("  Spec:      {spec}");
    }
    if let Some(task) = details.run.task.as_deref() {
        println!("  Task:      {task}");
    }
    if let Some(artifact_id) = details.run.artifact_id.as_deref() {
        println!("  Artifact:  {artifact_id}");
    }
    if let Some(summary) = details.run.summary.as_deref() {
        println!("  Summary:   {summary}");
    }
    if let Some(rationale) = details.run.rationale.as_deref() {
        println!("  Rationale: {rationale}");
    }

    println!("  Metadata:");
    print_indented_json(&details.run.metadata_json);

    println!("  Dimensions:");
    if details.dimensions.is_empty() {
        println!("    {}", "none".dimmed());
    } else {
        for dimension in &details.dimensions {
            println!(
                "    - {} | status={} | score={}{}",
                dimension.dimension_name,
                dimension.normalized_status,
                format_score(dimension.normalized_score),
                dimension
                    .normalized_value
                    .as_deref()
                    .map(|value| format!(" | value={value}"))
                    .unwrap_or_default()
            );
            if let Some(rationale) = dimension.rationale.as_deref() {
                println!("      rationale: {rationale}");
            }
            if dimension.details_json != json!({}) {
                print_indented_json_with_prefix(&dimension.details_json, "      details: ");
            }
        }
    }

    println!("  Links:");
    if details.links.is_empty() {
        println!("    {}", "none".dimmed());
    } else {
        for link in &details.links {
            println!(
                "    - {}:{} ({})",
                link.link_kind, link.link_ref, link.relation
            );
        }
    }
}

fn print_eval_comparison_human(comparison: &EvalRunComparison) {
    println!(
        "{}",
        format!(
            "═══ Eval Compare {} → {} ═══",
            comparison.baseline_eval_id, comparison.current_eval_id
        )
        .cyan()
    );
    println!("  Group:      {}", comparison.comparison_group);
    println!("  Overall:    {}", comparison.overall_classification);
    println!(
        "  Score Δ:    {}",
        format_signed_score(comparison.overall_score_delta)
    );

    println!("  Dimensions:");
    if comparison.dimensions.is_empty() {
        println!("    {}", "none".dimmed());
        return;
    }

    for dimension in &comparison.dimensions {
        println!(
            "    - {} | {} | baseline={} | current={} | Δ={}",
            dimension.dimension_name,
            dimension.classification,
            format_status_with_score(dimension.baseline_status.as_deref(), dimension.baseline_score),
            format_status_with_score(dimension.current_status.as_deref(), dimension.current_score),
            format_signed_score(dimension.score_delta)
        );
    }
}

fn load_eval_dimensions(
    inline_json: Option<&str>,
    file_path: Option<&str>,
) -> Result<Vec<CliEvalDimension>> {
    load_optional_json_array(inline_json, file_path, "dimensions")?
        .map(|value| {
            serde_json::from_value(value).context("dimensions must be an array of objects")
        })
        .transpose()
        .map(|value| value.unwrap_or_default())
}

fn load_eval_links(inline_json: Option<&str>, file_path: Option<&str>) -> Result<Vec<CliEvalLink>> {
    load_optional_json_array(inline_json, file_path, "links")?
        .map(|value| serde_json::from_value(value).context("links must be an array of objects"))
        .transpose()
        .map(|value| value.unwrap_or_default())
}

fn load_optional_json_object(raw: Option<&str>, arg_name: &str) -> Result<Option<Value>> {
    raw.map(|raw| parse_json_object(raw, arg_name)).transpose()
}

fn load_optional_json_array(
    inline_json: Option<&str>,
    file_path: Option<&str>,
    arg_name: &str,
) -> Result<Option<Value>> {
    match (inline_json, file_path) {
        (Some(_), Some(_)) => bail!("pass either --{arg_name}-json or --{arg_name}-file, not both"),
        (None, None) => Ok(None),
        (Some(raw), None) => parse_json_array(raw, &format!("--{arg_name}-json")).map(Some),
        (None, Some(path)) => {
            let raw = fs::read_to_string(path)
                .with_context(|| format!("failed to read JSON from '{}'", path))?;
            parse_json_array(&raw, &format!("--{arg_name}-file")).map(Some)
        }
    }
}

fn parse_json_object(raw: &str, context_name: &str) -> Result<Value> {
    let value: Value =
        serde_json::from_str(raw).with_context(|| format!("{context_name} must be valid JSON"))?;
    if !value.is_object() {
        bail!("{context_name} must be a JSON object");
    }
    Ok(value)
}

fn parse_json_array(raw: &str, context_name: &str) -> Result<Value> {
    let value: Value =
        serde_json::from_str(raw).with_context(|| format!("{context_name} must be valid JSON"))?;
    if !value.is_array() {
        bail!("{context_name} must be a JSON array");
    }
    Ok(value)
}

fn default_link_relation() -> String {
    "context".to_string()
}

fn format_score(value: Option<f64>) -> String {
    value
        .map(|score| format!("{score:.2}"))
        .unwrap_or_else(|| "—".to_string())
}

fn format_signed_score(value: Option<f64>) -> String {
    value
        .map(|score| format!("{score:+.2}"))
        .unwrap_or_else(|| "—".to_string())
}

fn format_status_with_score(status: Option<&str>, score: Option<f64>) -> String {
    match (status, score) {
        (Some(status), Some(score)) => format!("{status} ({score:.2})"),
        (Some(status), None) => status.to_string(),
        (None, Some(score)) => format!("— ({score:.2})"),
        (None, None) => "—".to_string(),
    }
}

fn format_timestamp(ts: &str) -> String {
    if ts.len() >= 19 {
        ts[..19].replace('T', " ")
    } else {
        ts.to_string()
    }
}

fn truncate(value: &str, max: usize) -> String {
    if value.len() <= max {
        value.to_string()
    } else {
        format!("{}…", &value[..max - 1])
    }
}

fn print_indented_json(value: &Value) {
    print_indented_json_with_prefix(value, "    ");
}

fn print_indented_json_with_prefix(value: &Value, prefix: &str) {
    if let Ok(pretty) = serde_json::to_string_pretty(value) {
        for line in pretty.lines() {
            println!("{prefix}{line}");
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_json_array_requires_array() {
        let err = parse_json_array("{}", "--dimensions-json").unwrap_err();
        assert!(err.to_string().contains("JSON array"));
    }

    #[test]
    fn load_eval_links_defaults_relation() {
        let links = load_eval_links(Some(r#"[{"kind":"task","ref":"T1"}]"#), None).unwrap();
        assert_eq!(links.len(), 1);
        assert_eq!(links[0].relation, "context");
    }
}
