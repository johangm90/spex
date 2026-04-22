use anyhow::{anyhow, bail, Context, Result};
use colored::Colorize;
use serde_json::{json, Value};
use sqlx::SqlitePool;
use std::fs;

use crate::sdd::{
    artifact::query_artifacts,
    evidence::{
        attach_artifact_to_evidence_bundle, attach_validation_run_to_evidence_bundle,
        create_evidence_bundle, get_evidence_bundle_details, get_evidence_bundle_for_entity,
        get_validation_run, record_validation_run, update_evidence_bundle, EvidenceArtifactRole,
        EvidenceBundlePatch, EvidenceBundleStatus, EvidenceRef, NewEvidenceBundle,
        RecordedValidationRun, ValidationCommandAlias, ValidationRequirementLevel,
        ValidationRunSource,
    },
    policy::{
        create_policy_config, get_policy_config, list_approvals, list_policy_configs,
        update_policy_config, ApprovalEntityKind, ApprovalStatus, CreatePolicyConfig,
        EnforcementMode, PolicyConfig, PolicyScopeKind, UpdatePolicyConfig,
    },
    spec::get_spec,
    task::get_task,
    workflow::{
        decide_risky_operation_approval, ApprovalDecisionWorkflowResult,
        RiskyOperationApprovalDecisionRequest,
    },
};

#[allow(clippy::too_many_arguments)]
pub async fn cmd_policy_config_set(
    pool: &SqlitePool,
    id: &str,
    scope: &str,
    scope_ref: Option<&str>,
    agent: Option<&str>,
    mode: &str,
    enabled: bool,
    rules_json: Option<&str>,
    rules_file: Option<&str>,
    rationale: Option<&str>,
    by: &str,
) -> Result<()> {
    let config = upsert_policy_config(
        pool, id, scope, scope_ref, agent, mode, enabled, rules_json, rules_file, rationale, by,
    )
    .await?;

    println!(
        "{} Policy {} saved for {}:{}{}",
        "✓".green().bold(),
        config.id.cyan(),
        config.scope_kind.as_str(),
        config.scope_ref,
        config
            .agent
            .as_deref()
            .map(|agent| format!(" @{}", agent))
            .unwrap_or_default()
    );
    println!(
        "  Mode: {} | Enabled: {}",
        config.enforcement_mode.as_str(),
        if config.enabled { "yes" } else { "no" }
    );
    if let Some(rationale) = config.rationale.as_deref() {
        println!("  Rationale: {rationale}");
    }

    Ok(())
}

pub async fn cmd_policy_config_list(
    pool: &SqlitePool,
    scope: Option<&str>,
    scope_ref: Option<&str>,
    agent: Option<&str>,
) -> Result<()> {
    let configs = list_policy_configs(
        pool,
        scope.map(parse_policy_scope_kind).transpose()?,
        scope_ref,
        None,
    )
    .await?
    .into_iter()
    .filter(|config| agent.is_none_or(|expected| config.agent.as_deref() == Some(expected)))
    .collect::<Vec<_>>();

    if configs.is_empty() {
        println!("{}", "No policy configs found.".dimmed());
        return Ok(());
    }

    println!(
        "{:<18} {:<8} {:<18} {:<16} {:<10} {:<8}",
        "ID".bold(),
        "Scope".bold(),
        "Ref".bold(),
        "Agent".bold(),
        "Mode".bold(),
        "Enabled".bold()
    );
    println!("{}", "─".repeat(86).dimmed());
    for config in configs {
        println!(
            "{:<18} {:<8} {:<18} {:<16} {:<10} {:<8}",
            truncate(&config.id, 17).cyan(),
            config.scope_kind.as_str(),
            truncate(&config.scope_ref, 17),
            config.agent.as_deref().unwrap_or("—"),
            config.enforcement_mode.as_str(),
            if config.enabled { "yes" } else { "no" }
        );
    }

    Ok(())
}

pub async fn cmd_policy_config_show(pool: &SqlitePool, id: &str) -> Result<()> {
    let config = get_policy_config(pool, id)
        .await?
        .ok_or_else(|| anyhow!("Policy config '{}' not found", id))?;

    println!("{}", format!("═══ Policy {} ═══", config.id).cyan());
    println!(
        "  Scope:    {}:{}{}",
        config.scope_kind.as_str(),
        config.scope_ref,
        config
            .agent
            .as_deref()
            .map(|agent| format!(" @{}", agent))
            .unwrap_or_default()
    );
    println!(
        "  Mode:     {} | Enabled: {}",
        config.enforcement_mode.as_str(),
        if config.enabled { "yes" } else { "no" }
    );
    if let Some(rationale) = config.rationale.as_deref() {
        println!("  Rationale: {rationale}");
    }
    println!("  Updated:  {}", config.updated_at);
    println!("  Rules:");
    println!(
        "{}",
        serde_json::to_string_pretty(&config.rules()?)?
            .lines()
            .map(|line| format!("    {line}"))
            .collect::<Vec<_>>()
            .join("\n")
    );

    Ok(())
}

#[allow(clippy::too_many_arguments)]
pub async fn cmd_policy_evidence_submit(
    pool: &SqlitePool,
    id: &str,
    spec: &str,
    task: Option<&str>,
    summary: Option<&str>,
    behavior_change: bool,
    metadata_json: Option<&str>,
    artifacts: &[String],
    validations: &[String],
    by: &str,
) -> Result<()> {
    let details = submit_evidence_bundle(
        pool,
        id,
        spec,
        task,
        summary,
        behavior_change,
        metadata_json,
        artifacts,
        validations,
        by,
    )
    .await?;

    println!(
        "{} Evidence bundle {} submitted for {} {}",
        "✓".green().bold(),
        details.bundle.id.cyan(),
        details.bundle.entity_kind,
        details.bundle.entity_id.cyan()
    );
    println!(
        "  Status: {} | Artifacts: {} | Validations: {}",
        details.bundle.status,
        details.artifacts.len(),
        details.validations.len()
    );
    if let Some(summary) = details.bundle.summary.as_deref() {
        println!("  Summary: {summary}");
    }

    Ok(())
}

pub async fn cmd_policy_evidence_show(pool: &SqlitePool, id: &str) -> Result<()> {
    let details = get_evidence_bundle_details(pool, id)
        .await?
        .ok_or_else(|| anyhow!("Evidence bundle '{}' not found", id))?;

    println!(
        "{}",
        format!("═══ Evidence {} ═══", details.bundle.id).cyan()
    );
    println!(
        "  Entity:   {} {}",
        details.bundle.entity_kind,
        details.bundle.entity_id.cyan()
    );
    println!("  Status:   {}", details.bundle.status);
    println!("  Spec:     {}", details.bundle.spec);
    if let Some(task) = details.bundle.task.as_deref() {
        println!("  Task:     {task}");
    }
    if let Some(summary) = details.bundle.summary.as_deref() {
        println!("  Summary:  {summary}");
    }
    println!("  Behavior change: {}", details.bundle.behavior_change);
    println!("  Artifacts:");
    if details.artifacts.is_empty() {
        println!("    {}", "none".dimmed());
    } else {
        for artifact in &details.artifacts {
            println!("    - {} ({})", artifact.artifact_id, artifact.role);
        }
    }
    println!("  Validations:");
    if details.validations.is_empty() {
        println!("    {}", "none".dimmed());
    } else {
        for validation in &details.validations {
            println!(
                "    - {} ({})",
                validation.validation_run_id, validation.requirement_level
            );
        }
    }

    Ok(())
}

#[allow(clippy::too_many_arguments)]
pub async fn cmd_policy_evidence_record_validation(
    pool: &SqlitePool,
    id: &str,
    bundle_id: &str,
    alias: &str,
    command: Option<&str>,
    passed: bool,
    exit_code: Option<i64>,
    output: Option<&str>,
) -> Result<()> {
    let alias_parsed = match alias {
        "fast" => ValidationCommandAlias::Fast,
        "primary" => ValidationCommandAlias::Primary,
        "full" => ValidationCommandAlias::Full,
        _ => ValidationCommandAlias::Custom,
    };
    let requirement = match alias {
        "fast" => ValidationRequirementLevel::Fast,
        "primary" => ValidationRequirementLevel::Primary,
        "full" => ValidationRequirementLevel::Full,
        _ => ValidationRequirementLevel::Custom,
    };
    let ran_at = chrono::Utc::now().to_rfc3339();
    // We need a reference — use the bundle's entity as reference
    let bundle = get_evidence_bundle_details(pool, bundle_id)
        .await?
        .ok_or_else(|| anyhow!("Evidence bundle '{}' not found", bundle_id))?;
    let evidence_ref = if let Some(task_id) = bundle.bundle.task.as_deref() {
        EvidenceRef::for_task(bundle.bundle.spec.clone(), task_id)
    } else {
        EvidenceRef::for_spec(bundle.bundle.spec.clone())
    };
    record_validation_run(
        pool,
        RecordedValidationRun {
            id,
            evidence_bundle_id: None,
            reference: evidence_ref,
            command_alias: alias_parsed,
            source: ValidationRunSource::Cli,
            command: command.unwrap_or(""),
            ran_at: &ran_at,
            exit_code,
            success: passed,
            output_summary: output,
            metadata_json: serde_json::json!({}),
            recorded_by: None,
        },
    )
    .await?;
    attach_validation_run_to_evidence_bundle(pool, bundle_id, id, requirement).await?;
    println!(
        "{} Validation run {} ({}) attached to bundle {}",
        "✓".green().bold(),
        id.cyan(),
        alias,
        bundle_id.cyan()
    );
    println!(
        "  Passed: {} | Exit code: {}",
        if passed { "yes".green() } else { "no".red() },
        exit_code
            .map(|c| c.to_string())
            .unwrap_or_else(|| "—".to_string())
            .normal()
    );
    Ok(())
}

pub async fn cmd_policy_approval_list(
    pool: &SqlitePool,
    entity_kind: Option<&str>,
    entity_id: Option<&str>,
    operation: Option<&str>,
    status: Option<&str>,
) -> Result<()> {
    let approvals = list_approvals(
        pool,
        entity_kind.map(parse_approval_entity_kind).transpose()?,
        entity_id,
        operation,
        status.map(parse_approval_status).transpose()?,
    )
    .await?;

    if approvals.is_empty() {
        println!("{}", "No approvals found.".dimmed());
        return Ok(());
    }

    println!(
        "{:<18} {:<10} {:<18} {:<18} {:<10}",
        "ID".bold(),
        "Entity".bold(),
        "Entity ID".bold(),
        "Operation".bold(),
        "Status".bold()
    );
    println!("{}", "─".repeat(82).dimmed());
    for approval in approvals {
        println!(
            "{:<18} {:<10} {:<18} {:<18} {:<10}",
            truncate(&approval.id, 17).cyan(),
            approval.entity_kind.as_str(),
            truncate(&approval.entity_id, 17),
            truncate(&approval.operation_kind, 17),
            approval.status.as_str()
        );
    }

    Ok(())
}

pub async fn cmd_policy_approval_approve(
    pool: &SqlitePool,
    id: &str,
    by: &str,
    reason: Option<&str>,
) -> Result<()> {
    let result = apply_approval_decision(pool, id, ApprovalStatus::Approved, by, reason).await?;
    print_approval_decision_result("approved", &result);
    Ok(())
}

pub async fn cmd_policy_approval_reject(
    pool: &SqlitePool,
    id: &str,
    by: &str,
    reason: Option<&str>,
) -> Result<()> {
    let result = apply_approval_decision(pool, id, ApprovalStatus::Rejected, by, reason).await?;
    print_approval_decision_result("rejected", &result);
    Ok(())
}

#[allow(clippy::too_many_arguments)]
async fn upsert_policy_config(
    pool: &SqlitePool,
    id: &str,
    scope: &str,
    scope_ref: Option<&str>,
    agent: Option<&str>,
    mode: &str,
    enabled: bool,
    rules_json: Option<&str>,
    rules_file: Option<&str>,
    rationale: Option<&str>,
    by: &str,
) -> Result<PolicyConfig> {
    let scope_kind = parse_policy_scope_kind(scope)?;
    let scope_ref = resolve_scope_ref(scope_kind, scope_ref)?;
    let enforcement_mode = parse_enforcement_mode(mode)?;
    let rules = load_json_object_arg(rules_json, rules_file, "rules")?;

    if let Some(existing) = get_policy_config(pool, id).await? {
        if existing.scope_kind != scope_kind
            || existing.scope_ref != scope_ref
            || existing.agent.as_deref() != agent.filter(|value| !value.trim().is_empty())
        {
            bail!(
                "policy config '{}' already exists for {}:{}{}; scope changes require a new config id",
                id,
                existing.scope_kind.as_str(),
                existing.scope_ref,
                existing
                    .agent
                    .as_deref()
                    .map(|agent| format!(" @{}", agent))
                    .unwrap_or_default()
            );
        }

        update_policy_config(
            pool,
            id,
            UpdatePolicyConfig {
                enabled,
                enforcement_mode,
                rules_json: &rules,
                rationale,
                updated_by: Some(by),
            },
        )
        .await
    } else {
        create_policy_config(
            pool,
            CreatePolicyConfig {
                id,
                scope_kind,
                scope_ref: &scope_ref,
                agent,
                enabled,
                enforcement_mode,
                rules_json: &rules,
                rationale,
                created_by: Some(by),
            },
        )
        .await
    }
}

#[allow(clippy::too_many_arguments)]
async fn submit_evidence_bundle(
    pool: &SqlitePool,
    id: &str,
    spec: &str,
    task: Option<&str>,
    summary: Option<&str>,
    behavior_change: bool,
    metadata_json: Option<&str>,
    artifacts: &[String],
    validations: &[String],
    by: &str,
) -> Result<crate::sdd::evidence::EvidenceBundleDetails> {
    let reference = resolve_evidence_ref(pool, spec, task).await?;
    let metadata =
        load_optional_json_object(metadata_json, "metadata_json")?.unwrap_or_else(|| json!({}));

    let existing_for_entity = get_evidence_bundle_for_entity(pool, &reference).await?;
    let bundle = match existing_for_entity {
        Some(existing) if existing.id != id => {
            bail!(
                "entity already has evidence bundle '{}'; reuse that id or inspect it with `spex policy evidence show {}`",
                existing.id,
                existing.id
            )
        }
        Some(existing) => {
            update_evidence_bundle(
                pool,
                &existing.id,
                EvidenceBundlePatch {
                    status: EvidenceBundleStatus::Submitted,
                    summary,
                    behavior_change,
                    metadata_json: metadata,
                    updated_by: Some(by),
                },
            )
            .await?
        }
        None => {
            create_evidence_bundle(
                pool,
                NewEvidenceBundle {
                    id,
                    reference,
                    status: EvidenceBundleStatus::Submitted,
                    summary,
                    behavior_change,
                    metadata_json: metadata,
                    created_by: Some(by),
                    updated_by: Some(by),
                },
            )
            .await?
        }
    };

    let existing_details = get_evidence_bundle_details(pool, &bundle.id)
        .await?
        .ok_or_else(|| anyhow!("Evidence bundle '{}' not found after submit", bundle.id))?;

    let scoped_artifacts = query_artifacts(pool, Some(spec), task, None, None).await?;
    for artifact in artifacts {
        let (artifact_id, role) = parse_artifact_attachment(artifact)?;
        let existing = existing_details
            .artifacts
            .iter()
            .find(|link| link.artifact_id == artifact_id);
        if let Some(existing) = existing {
            if existing.role != role.as_str() {
                bail!(
                    "artifact '{}' is already linked to bundle '{}' as '{}'",
                    artifact_id,
                    bundle.id,
                    existing.role
                );
            }
            continue;
        }
        if !scoped_artifacts
            .iter()
            .any(|candidate| candidate.id == artifact_id)
        {
            bail!(
                "artifact '{}' was not found for spec '{}'{}",
                artifact_id,
                spec,
                task.map(|task| format!(" task '{}'", task))
                    .unwrap_or_default()
            );
        }
        attach_artifact_to_evidence_bundle(pool, &bundle.id, &artifact_id, role).await?;
    }

    for validation in validations {
        let (validation_id, requirement) = parse_validation_attachment(validation)?;
        let existing = existing_details
            .validations
            .iter()
            .find(|link| link.validation_run_id == validation_id);
        if let Some(existing) = existing {
            if existing.requirement_level != requirement.as_str() {
                bail!(
                    "validation '{}' is already linked to bundle '{}' as '{}'",
                    validation_id,
                    bundle.id,
                    existing.requirement_level
                );
            }
            continue;
        }
        let run = get_validation_run(pool, &validation_id)
            .await?
            .ok_or_else(|| anyhow!("validation run '{}' not found", validation_id))?;
        if run.spec != spec || run.task.as_deref() != task {
            bail!(
                "validation run '{}' belongs to spec '{}' task '{}'",
                validation_id,
                run.spec,
                run.task.as_deref().unwrap_or("—")
            );
        }
        attach_validation_run_to_evidence_bundle(pool, &bundle.id, &validation_id, requirement)
            .await?;
    }

    get_evidence_bundle_details(pool, &bundle.id)
        .await?
        .ok_or_else(|| anyhow!("Evidence bundle '{}' not found after submit", bundle.id))
}

async fn apply_approval_decision(
    pool: &SqlitePool,
    id: &str,
    status: ApprovalStatus,
    by: &str,
    reason: Option<&str>,
) -> Result<ApprovalDecisionWorkflowResult> {
    decide_risky_operation_approval(
        pool,
        RiskyOperationApprovalDecisionRequest {
            approval_id: id,
            decision: crate::sdd::policy::ApprovalDecision {
                status,
                decided_by: by,
                decision_reason: reason,
            },
        },
    )
    .await
}

fn print_approval_decision_result(action: &str, result: &ApprovalDecisionWorkflowResult) {
    println!(
        "{} Approval {} {}.",
        "✓".green().bold(),
        result.approval.id.cyan(),
        action
    );
    println!(
        "  Operation: {} | Entity: {} {}",
        result.approval.operation_kind,
        result.approval.entity_kind.as_str(),
        result.approval.entity_id.cyan()
    );
    println!("  Gate status: {}", result.evaluation.reason);
}

async fn resolve_evidence_ref(
    pool: &SqlitePool,
    spec: &str,
    task: Option<&str>,
) -> Result<EvidenceRef> {
    let spec_row = get_spec(pool, spec)
        .await?
        .ok_or_else(|| anyhow!("Spec '{}' not found", spec))?;
    if let Some(task_id) = task {
        let task_row = get_task(pool, task_id)
            .await?
            .ok_or_else(|| anyhow!("Task '{}' not found", task_id))?;
        if task_row.spec != spec_row.id {
            bail!(
                "task '{}' belongs to spec '{}', not '{}'",
                task_id,
                task_row.spec,
                spec_row.id
            );
        }
        Ok(EvidenceRef::for_task(spec, task_id))
    } else {
        Ok(EvidenceRef::for_spec(spec))
    }
}

fn load_json_object_arg(
    inline_json: Option<&str>,
    file_path: Option<&str>,
    arg_name: &str,
) -> Result<Value> {
    match (inline_json, file_path) {
        (Some(_), Some(_)) => bail!("pass either --{arg_name}-json or --{arg_name}-file, not both"),
        (None, None) => bail!("missing --{arg_name}-json or --{arg_name}-file"),
        (Some(raw), None) => parse_json_object(raw, &format!("--{arg_name}-json")),
        (None, Some(path)) => {
            let raw = fs::read_to_string(path)
                .with_context(|| format!("failed to read JSON from '{}'", path))?;
            parse_json_object(&raw, &format!("--{arg_name}-file"))
        }
    }
}

fn load_optional_json_object(raw: Option<&str>, arg_name: &str) -> Result<Option<Value>> {
    raw.map(|raw| parse_json_object(raw, arg_name)).transpose()
}

fn parse_json_object(raw: &str, context_name: &str) -> Result<Value> {
    let value: Value =
        serde_json::from_str(raw).with_context(|| format!("{context_name} must be valid JSON"))?;
    if !value.is_object() {
        bail!("{context_name} must be a JSON object");
    }
    Ok(value)
}

fn parse_policy_scope_kind(value: &str) -> Result<PolicyScopeKind> {
    match value {
        "project" => Ok(PolicyScopeKind::Project),
        "spec" => Ok(PolicyScopeKind::Spec),
        "task" => Ok(PolicyScopeKind::Task),
        _ => bail!(
            "invalid policy scope '{}'; expected project|spec|task",
            value
        ),
    }
}

fn resolve_scope_ref(scope_kind: PolicyScopeKind, scope_ref: Option<&str>) -> Result<String> {
    match scope_kind {
        PolicyScopeKind::Project => Ok("project".to_string()),
        PolicyScopeKind::Spec | PolicyScopeKind::Task => scope_ref
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .map(str::to_string)
            .ok_or_else(|| anyhow!("--scope-ref is required for {} scope", scope_kind.as_str())),
    }
}

fn parse_enforcement_mode(value: &str) -> Result<EnforcementMode> {
    match value {
        "advisory" => Ok(EnforcementMode::Advisory),
        "enforced" => Ok(EnforcementMode::Enforced),
        _ => bail!(
            "invalid enforcement mode '{}'; expected advisory|enforced",
            value
        ),
    }
}

fn parse_artifact_attachment(value: &str) -> Result<(String, EvidenceArtifactRole)> {
    let (id, role) = value.split_once(':').unwrap_or((value, "supporting"));
    let role = match role {
        "supporting" => EvidenceArtifactRole::Supporting,
        "primary_output" => EvidenceArtifactRole::PrimaryOutput,
        "test_evidence" => EvidenceArtifactRole::TestEvidence,
        _ => bail!(
            "invalid artifact role '{}' for '{}'; expected supporting|primary_output|test_evidence",
            role,
            id
        ),
    };
    Ok((id.to_string(), role))
}

fn parse_validation_attachment(value: &str) -> Result<(String, ValidationRequirementLevel)> {
    let (id, requirement) = value.split_once(':').unwrap_or((value, "primary"));
    let requirement = match requirement {
        "fast" => ValidationRequirementLevel::Fast,
        "primary" => ValidationRequirementLevel::Primary,
        "full" => ValidationRequirementLevel::Full,
        "custom" => ValidationRequirementLevel::Custom,
        _ => bail!(
            "invalid validation requirement '{}' for '{}'; expected fast|primary|full|custom",
            requirement,
            id
        ),
    };
    Ok((id.to_string(), requirement))
}

fn parse_approval_entity_kind(value: &str) -> Result<ApprovalEntityKind> {
    match value {
        "task" => Ok(ApprovalEntityKind::Task),
        "spec" => Ok(ApprovalEntityKind::Spec),
        "operation" => Ok(ApprovalEntityKind::Operation),
        _ => bail!(
            "invalid approval entity kind '{}'; expected task|spec|operation",
            value
        ),
    }
}

fn parse_approval_status(value: &str) -> Result<ApprovalStatus> {
    match value {
        "pending" => Ok(ApprovalStatus::Pending),
        "approved" => Ok(ApprovalStatus::Approved),
        "rejected" => Ok(ApprovalStatus::Rejected),
        "cancelled" => Ok(ApprovalStatus::Cancelled),
        "expired" => Ok(ApprovalStatus::Expired),
        _ => bail!(
            "invalid approval status '{}'; expected pending|approved|rejected|cancelled|expired",
            value
        ),
    }
}

fn truncate(value: &str, max: usize) -> String {
    if value.len() <= max {
        value.to_string()
    } else {
        format!("{}…", &value[..max - 1])
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::sdd::{
        artifact::register_artifact,
        evidence::{
            list_evidence_bundle_artifacts, list_evidence_bundle_validations,
            record_validation_run, RecordedValidationRun, ValidationCommandAlias,
            ValidationRunSource,
        },
        policy::{
            create_policy_config, get_approval, ApprovalState, CompletionEvidence,
            RiskyOperationKind,
        },
        spec::create_spec,
        task::create_task,
        test_helpers::make_pool,
        workflow::{request_risky_operation_approval, RiskyOperationApprovalRequest},
    };

    #[tokio::test]
    async fn upsert_policy_config_creates_then_updates_existing_record() {
        let pool = make_pool().await;

        let created = upsert_policy_config(
            &pool,
            "policy-cli-1",
            "spec",
            Some("SPEC-CLI-1"),
            Some("builder"),
            "enforced",
            true,
            Some(r#"{"require_validation":"primary"}"#),
            None,
            Some("initial"),
            "architect",
        )
        .await
        .unwrap();
        assert_eq!(created.scope_ref, "SPEC-CLI-1");
        assert_eq!(created.updated_by.as_deref(), Some("architect"));

        let updated = upsert_policy_config(
            &pool,
            "policy-cli-1",
            "spec",
            Some("SPEC-CLI-1"),
            Some("builder"),
            "advisory",
            false,
            Some(r#"{"require_rationale":true}"#),
            None,
            Some("updated"),
            "reviewer",
        )
        .await
        .unwrap();

        assert!(!updated.enabled);
        assert_eq!(updated.enforcement_mode, EnforcementMode::Advisory);
        assert_eq!(updated.updated_by.as_deref(), Some("reviewer"));
        assert_eq!(updated.rules().unwrap()["require_rationale"], true);
    }

    #[tokio::test]
    async fn submit_evidence_bundle_creates_bundle_and_links_refs() {
        let pool = make_pool().await;
        create_spec(&pool, "SPEC-CLI-E1", "Policy CLI", "P0", &[])
            .await
            .unwrap();
        create_task(
            &pool,
            "TASK-CLI-E1",
            "SPEC-CLI-E1",
            "Policy evidence",
            "builder",
            &[],
            Some("src/cli/policy.rs"),
        )
        .await
        .unwrap();
        register_artifact(
            &pool,
            "artifact-cli-1",
            Some("SPEC-CLI-E1"),
            Some("TASK-CLI-E1"),
            "builder",
            "source",
            Some("src/cli/policy.rs"),
            Some("policy cli source"),
            None,
        )
        .await
        .unwrap();
        record_validation_run(
            &pool,
            RecordedValidationRun {
                id: "validation-cli-1",
                evidence_bundle_id: None,
                reference: EvidenceRef::for_task("SPEC-CLI-E1", "TASK-CLI-E1"),
                command_alias: ValidationCommandAlias::Primary,
                command: "cargo test --all-targets",
                source: ValidationRunSource::Cli,
                exit_code: Some(0),
                success: true,
                ran_at: "2026-04-22T12:30:00Z",
                recorded_by: Some("builder"),
                output_summary: Some("ok"),
                metadata_json: json!({"suite": "policy"}),
            },
        )
        .await
        .unwrap();

        let details = submit_evidence_bundle(
            &pool,
            "bundle-cli-1",
            "SPEC-CLI-E1",
            Some("TASK-CLI-E1"),
            Some("CLI evidence"),
            true,
            Some(r#"{"source":"cli"}"#),
            &["artifact-cli-1:primary_output".to_string()],
            &["validation-cli-1:primary".to_string()],
            "builder",
        )
        .await
        .unwrap();

        assert_eq!(
            details.bundle.status,
            EvidenceBundleStatus::Submitted.as_str()
        );
        assert_eq!(details.artifacts.len(), 1);
        assert_eq!(details.validations.len(), 1);
        assert_eq!(
            list_evidence_bundle_artifacts(&pool, "bundle-cli-1")
                .await
                .unwrap()[0]
                .role,
            "primary_output"
        );
        assert_eq!(
            list_evidence_bundle_validations(&pool, "bundle-cli-1")
                .await
                .unwrap()[0]
                .requirement_level,
            "primary"
        );
    }

    #[tokio::test]
    async fn apply_approval_decision_uses_workflow_decision_logic() {
        let pool = make_pool().await;
        create_spec(&pool, "SPEC-CLI-A1", "Approval CLI", "P0", &[])
            .await
            .unwrap();
        crate::sdd::workflow::approve_spec(&pool, "SPEC-CLI-A1", "human", None)
            .await
            .unwrap();
        crate::sdd::workflow::start_spec(&pool, "SPEC-CLI-A1", "human")
            .await
            .unwrap();
        create_task(
            &pool,
            "TASK-CLI-A1",
            "SPEC-CLI-A1",
            "Approval decision",
            "builder",
            &[],
            None,
        )
        .await
        .unwrap();
        crate::sdd::workflow::start_task(&pool, "TASK-CLI-A1", "test-agent")
            .await
            .unwrap();
        create_policy_config(
            &pool,
            CreatePolicyConfig {
                id: "policy-cli-approval",
                scope_kind: PolicyScopeKind::Task,
                scope_ref: "TASK-CLI-A1",
                agent: Some("builder"),
                enabled: true,
                enforcement_mode: EnforcementMode::Enforced,
                rules_json: &json!({
                    "task_completion": {
                        "require_approval": true,
                        "require_evidence_bundle": false,
                        "require_rationale": false,
                        "require_validation": "fast"
                    },
                    "risky_operations": {
                        "complete_task": "require_approval"
                    }
                }),
                rationale: Some("manual review"),
                created_by: Some("architect"),
            },
        )
        .await
        .unwrap();
        request_risky_operation_approval(
            &pool,
            RiskyOperationApprovalRequest {
                approval_id: "approval-cli-1",
                operation: crate::sdd::policy::RiskyOperationRequest {
                    spec_status: "in_progress",
                    spec_ref: Some("SPEC-CLI-A1"),
                    task_ref: Some("TASK-CLI-A1"),
                    agent: Some("builder"),
                    entity_kind: ApprovalEntityKind::Task,
                    entity_id: "TASK-CLI-A1",
                    operation: RiskyOperationKind::CompleteTask,
                    write_path: None,
                    completion_evidence: Some(CompletionEvidence {
                        has_evidence_bundle: true,
                        has_rationale: true,
                        satisfied_validation: Some(ValidationRequirementLevel::Fast),
                    }),
                },
                requested_by: "builder",
                request_context_json: &json!({
                    "spec_status": "in_progress",
                    "agent": "builder",
                    "completion_evidence": {
                        "has_evidence_bundle": true,
                        "has_rationale": true,
                        "satisfied_validation": "fast"
                    }
                }),
                evidence_bundle_id: None,
                expires_at: None,
            },
            None,
        )
        .await
        .unwrap();

        let result = apply_approval_decision(
            &pool,
            "approval-cli-1",
            ApprovalStatus::Approved,
            "reviewer",
            Some("ship it"),
        )
        .await
        .unwrap();

        assert_eq!(result.approval.status, ApprovalStatus::Approved);
        assert!(matches!(
            result.evaluation.approval_state,
            Some(ApprovalState::Approved(_))
        ));
        assert_eq!(
            get_approval(&pool, "approval-cli-1")
                .await
                .unwrap()
                .unwrap()
                .decided_by
                .as_deref(),
            Some("reviewer")
        );
    }
}
