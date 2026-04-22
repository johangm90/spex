#![allow(dead_code)]

use anyhow::{anyhow, Result};
use chrono::Utc;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use sqlx::{Sqlite, SqlitePool, Transaction};
use std::{collections::BTreeMap, path::Path};
use thiserror::Error;

use crate::sdd::evidence::ValidationRequirementLevel;

pub const PROJECT_SCOPE_REF: &str = "project";

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
pub enum PolicyScopeKind {
    Project,
    Spec,
    Task,
}

impl PolicyScopeKind {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Project => "project",
            Self::Spec => "spec",
            Self::Task => "task",
        }
    }

    pub fn from_str(value: &str) -> Option<Self> {
        match value {
            "project" => Some(Self::Project),
            "spec" => Some(Self::Spec),
            "task" => Some(Self::Task),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
pub enum EnforcementMode {
    Advisory,
    Enforced,
}

impl EnforcementMode {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Advisory => "advisory",
            Self::Enforced => "enforced",
        }
    }

    pub fn from_str(value: &str) -> Option<Self> {
        match value {
            "advisory" => Some(Self::Advisory),
            "enforced" => Some(Self::Enforced),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
pub enum ApprovalEntityKind {
    Task,
    Spec,
    Operation,
}

impl ApprovalEntityKind {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Task => "task",
            Self::Spec => "spec",
            Self::Operation => "operation",
        }
    }

    pub fn from_str(value: &str) -> Option<Self> {
        match value {
            "task" => Some(Self::Task),
            "spec" => Some(Self::Spec),
            "operation" => Some(Self::Operation),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
pub enum ApprovalStatus {
    Pending,
    Approved,
    Rejected,
    Cancelled,
    Expired,
}

impl ApprovalStatus {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Pending => "pending",
            Self::Approved => "approved",
            Self::Rejected => "rejected",
            Self::Cancelled => "cancelled",
            Self::Expired => "expired",
        }
    }

    pub fn from_str(value: &str) -> Option<Self> {
        match value {
            "pending" => Some(Self::Pending),
            "approved" => Some(Self::Approved),
            "rejected" => Some(Self::Rejected),
            "cancelled" => Some(Self::Cancelled),
            "expired" => Some(Self::Expired),
            _ => None,
        }
    }

    pub fn is_terminal(self) -> bool {
        self != Self::Pending
    }
}

#[derive(Debug, Error)]
pub enum PolicyError {
    #[error("invalid scope reference '{scope_ref}' for scope kind '{scope_kind}'")]
    BadScopeRef {
        scope_kind: &'static str,
        scope_ref: String,
    },
    #[error("policy rules_json must be a JSON object")]
    BadRulesJson,
    #[error("approval request_context_json must be a JSON object")]
    BadRequestContext,
    #[error("invalid approval entity linkage for '{0}'")]
    BadApprovalEntity(String),
    #[error("approval '{id}' cannot transition from '{current}' to '{next}'")]
    TransitionRejected {
        id: String,
        current: &'static str,
        next: &'static str,
    },
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct PolicyScopeKey {
    pub scope_kind: PolicyScopeKind,
    pub scope_ref: String,
    pub agent: Option<String>,
}

impl PolicyScopeKey {
    pub fn new(
        scope_kind: PolicyScopeKind,
        scope_ref: impl Into<String>,
        agent: Option<&str>,
    ) -> Result<Self> {
        let scope_ref = normalize_scope_ref(scope_kind, scope_ref.into())?;
        Ok(Self {
            scope_kind,
            scope_ref,
            agent: normalize_agent(agent),
        })
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct PolicyConfig {
    pub id: String,
    pub scope_kind: PolicyScopeKind,
    pub scope_ref: String,
    pub agent: Option<String>,
    pub enabled: bool,
    pub enforcement_mode: EnforcementMode,
    pub rules_json: String,
    pub rationale: Option<String>,
    pub created_by: Option<String>,
    pub updated_by: Option<String>,
    pub created_at: String,
    pub updated_at: String,
}

impl PolicyConfig {
    pub fn scope_key(&self) -> PolicyScopeKey {
        PolicyScopeKey {
            scope_kind: self.scope_kind,
            scope_ref: self.scope_ref.clone(),
            agent: self.agent.clone(),
        }
    }

    pub fn rules(&self) -> Result<Value> {
        Ok(serde_json::from_str(&self.rules_json)?)
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct ApprovalRecord {
    pub id: String,
    pub entity_kind: ApprovalEntityKind,
    pub entity_id: String,
    pub spec: Option<String>,
    pub task: Option<String>,
    pub operation_kind: String,
    pub status: ApprovalStatus,
    pub policy_config_id: Option<String>,
    pub evidence_bundle_id: Option<String>,
    pub requested_by: String,
    pub decided_by: Option<String>,
    pub decision_reason: Option<String>,
    pub request_context_json: String,
    pub created_at: String,
    pub decided_at: Option<String>,
    pub expires_at: Option<String>,
}

impl ApprovalRecord {
    pub fn request_context(&self) -> Result<Value> {
        Ok(serde_json::from_str(&self.request_context_json)?)
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub enum ApprovalState {
    NotRequested,
    Pending(ApprovalRecord),
    Approved(ApprovalRecord),
    Rejected(ApprovalRecord),
    Cancelled(ApprovalRecord),
    Expired(ApprovalRecord),
}

impl ApprovalState {
    fn from_latest(record: Option<ApprovalRecord>) -> Self {
        match record {
            None => Self::NotRequested,
            Some(record) => match record.status {
                ApprovalStatus::Pending => Self::Pending(record),
                ApprovalStatus::Approved => Self::Approved(record),
                ApprovalStatus::Rejected => Self::Rejected(record),
                ApprovalStatus::Cancelled => Self::Cancelled(record),
                ApprovalStatus::Expired => Self::Expired(record),
            },
        }
    }
}

#[derive(Debug, Clone)]
pub struct CreatePolicyConfig<'a> {
    pub id: &'a str,
    pub scope_kind: PolicyScopeKind,
    pub scope_ref: &'a str,
    pub agent: Option<&'a str>,
    pub enabled: bool,
    pub enforcement_mode: EnforcementMode,
    pub rules_json: &'a Value,
    pub rationale: Option<&'a str>,
    pub created_by: Option<&'a str>,
}

#[derive(Debug, Clone)]
pub struct UpdatePolicyConfig<'a> {
    pub enabled: bool,
    pub enforcement_mode: EnforcementMode,
    pub rules_json: &'a Value,
    pub rationale: Option<&'a str>,
    pub updated_by: Option<&'a str>,
}

#[derive(Debug, Clone)]
pub struct CreateApproval<'a> {
    pub id: &'a str,
    pub entity_kind: ApprovalEntityKind,
    pub entity_id: &'a str,
    pub spec: Option<&'a str>,
    pub task: Option<&'a str>,
    pub operation_kind: &'a str,
    pub policy_config_id: Option<&'a str>,
    pub evidence_bundle_id: Option<&'a str>,
    pub requested_by: &'a str,
    pub request_context_json: &'a Value,
    pub expires_at: Option<&'a str>,
}

#[derive(Debug, Clone)]
pub struct ApprovalDecision<'a> {
    pub status: ApprovalStatus,
    pub decided_by: &'a str,
    pub decision_reason: Option<&'a str>,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, PartialOrd, Ord)]
#[serde(rename_all = "snake_case")]
pub enum RiskyOperationKind {
    DestructiveCommand,
    WriteOutsideAllowedScope,
    SchemaChange,
    GlobalConfigChange,
    CompleteTask,
    CompleteSpec,
}

impl RiskyOperationKind {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::DestructiveCommand => "destructive_command",
            Self::WriteOutsideAllowedScope => "write_outside_allowed_scope",
            Self::SchemaChange => "schema_change",
            Self::GlobalConfigChange => "global_config_change",
            Self::CompleteTask => "complete_task",
            Self::CompleteSpec => "complete_spec",
        }
    }

    pub fn from_str(value: &str) -> Option<Self> {
        match value {
            "destructive_command" => Some(Self::DestructiveCommand),
            "write_outside_allowed_scope" => Some(Self::WriteOutsideAllowedScope),
            "schema_change" => Some(Self::SchemaChange),
            "global_config_change" => Some(Self::GlobalConfigChange),
            "complete_task" => Some(Self::CompleteTask),
            "complete_spec" => Some(Self::CompleteSpec),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum RiskDisposition {
    Allow,
    RequireApproval,
    Deny,
}

impl RiskDisposition {
    fn from_str(value: &str) -> Option<Self> {
        match value {
            "allow" => Some(Self::Allow),
            "require_approval" => Some(Self::RequireApproval),
            "deny" => Some(Self::Deny),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CompletionPolicy {
    pub require_evidence_bundle: bool,
    pub require_rationale: bool,
    pub require_validation: Option<ValidationRequirementLevel>,
    pub require_approval: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ResolvedPolicySource {
    pub id: String,
    pub scope_kind: PolicyScopeKind,
    pub scope_ref: String,
    pub agent: Option<String>,
    pub enforcement_mode: EnforcementMode,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EffectivePolicy {
    pub spec_ref: Option<String>,
    pub task_ref: Option<String>,
    pub agent: Option<String>,
    pub enforcement_mode: EnforcementMode,
    pub fail_closed: bool,
    pub allowed_write_scopes: Vec<String>,
    pub task_completion: CompletionPolicy,
    pub spec_completion: CompletionPolicy,
    pub risky_operations: BTreeMap<RiskyOperationKind, RiskDisposition>,
    pub sources: Vec<ResolvedPolicySource>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct CompletionEvidence {
    pub has_evidence_bundle: bool,
    pub has_rationale: bool,
    pub satisfied_validation: Option<ValidationRequirementLevel>,
}

#[derive(Debug, Clone, Copy)]
pub struct RiskyOperationRequest<'a> {
    pub spec_status: &'a str,
    pub spec_ref: Option<&'a str>,
    pub task_ref: Option<&'a str>,
    pub agent: Option<&'a str>,
    pub entity_kind: ApprovalEntityKind,
    pub entity_id: &'a str,
    pub operation: RiskyOperationKind,
    pub write_path: Option<&'a str>,
    pub completion_evidence: Option<CompletionEvidence>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RiskyOperationOutcome {
    Allowed,
    ApprovalRequired,
    Denied,
}

#[derive(Debug, Clone, PartialEq)]
pub struct RiskyOperationEvaluation {
    pub outcome: RiskyOperationOutcome,
    pub reason: String,
    pub effective_policy: EffectivePolicy,
    pub approval_state: Option<ApprovalState>,
}

pub async fn resolve_effective_policy(
    pool: &SqlitePool,
    spec_ref: Option<&str>,
    task_ref: Option<&str>,
    agent: Option<&str>,
    spec_status: &str,
) -> Result<EffectivePolicy> {
    let configs = list_policy_configs_for_resolution(pool, spec_ref, task_ref, agent).await?;
    let fail_closed = policy_rollout_applies(spec_status);
    let mut policy = EffectivePolicy::defaults(spec_ref, task_ref, agent, fail_closed);

    for config in configs.into_iter().filter(|config| config.enabled) {
        let overlay = parse_policy_overlay(&config.rules()?)?;
        policy.enforcement_mode = config.enforcement_mode;
        policy.apply_overlay(overlay);
        policy.sources.push(ResolvedPolicySource {
            id: config.id,
            scope_kind: config.scope_kind,
            scope_ref: config.scope_ref,
            agent: config.agent,
            enforcement_mode: config.enforcement_mode,
        });
    }

    Ok(policy)
}

pub async fn evaluate_risky_operation(
    pool: &SqlitePool,
    request: RiskyOperationRequest<'_>,
) -> Result<RiskyOperationEvaluation> {
    let fail_closed = policy_rollout_applies(request.spec_status);
    let effective_policy = match resolve_effective_policy(
        pool,
        request.spec_ref,
        request.task_ref,
        request.agent,
        request.spec_status,
    )
    .await
    {
        Ok(policy) => policy,
        Err(error) if fail_closed => {
            return Ok(RiskyOperationEvaluation {
                outcome: RiskyOperationOutcome::Denied,
                reason: format!(
                    "invalid policy input for approved spec; failing closed: {}",
                    error
                ),
                effective_policy: EffectivePolicy::defaults(
                    request.spec_ref,
                    request.task_ref,
                    request.agent,
                    true,
                ),
                approval_state: None,
            })
        }
        Err(error) => return Err(error),
    };

    if !effective_policy.fail_closed {
        return Ok(RiskyOperationEvaluation {
            outcome: RiskyOperationOutcome::Allowed,
            reason: format!(
                "policy rollout not enforced for spec status '{}'",
                request.spec_status
            ),
            effective_policy,
            approval_state: None,
        });
    }

    match request.operation {
        RiskyOperationKind::WriteOutsideAllowedScope => {
            let Some(write_path) = request.write_path else {
                return Ok(RiskyOperationEvaluation {
                    outcome: RiskyOperationOutcome::Denied,
                    reason: "missing write path for governed scope check".to_string(),
                    effective_policy,
                    approval_state: None,
                });
            };

            let allowed =
                is_path_within_allowed_scopes(write_path, &effective_policy.allowed_write_scopes);
            if !allowed {
                return Ok(RiskyOperationEvaluation {
                    outcome: RiskyOperationOutcome::Denied,
                    reason: format!(
                        "write path '{}' is outside allowed policy scope",
                        write_path
                    ),
                    effective_policy,
                    approval_state: None,
                });
            }

            Ok(RiskyOperationEvaluation {
                outcome: RiskyOperationOutcome::Allowed,
                reason: format!("write path '{}' is within allowed policy scope", write_path),
                effective_policy,
                approval_state: None,
            })
        }
        RiskyOperationKind::CompleteTask | RiskyOperationKind::CompleteSpec => {
            let completion_policy = if request.operation == RiskyOperationKind::CompleteTask {
                &effective_policy.task_completion
            } else {
                &effective_policy.spec_completion
            };
            let evidence = request.completion_evidence.unwrap_or_default();

            if completion_policy.require_evidence_bundle && !evidence.has_evidence_bundle {
                return Ok(RiskyOperationEvaluation {
                    outcome: RiskyOperationOutcome::Denied,
                    reason: format!(
                        "{} requires an evidence bundle before completion",
                        request.operation.as_str()
                    ),
                    effective_policy,
                    approval_state: None,
                });
            }
            if completion_policy.require_rationale && !evidence.has_rationale {
                return Ok(RiskyOperationEvaluation {
                    outcome: RiskyOperationOutcome::Denied,
                    reason: format!(
                        "{} requires completion rationale evidence",
                        request.operation.as_str()
                    ),
                    effective_policy,
                    approval_state: None,
                });
            }
            if let Some(required_validation) = completion_policy.require_validation {
                if !validation_satisfies(evidence.satisfied_validation, required_validation) {
                    return Ok(RiskyOperationEvaluation {
                        outcome: RiskyOperationOutcome::Denied,
                        reason: format!(
                            "{} requires '{}' validation evidence",
                            request.operation.as_str(),
                            required_validation.as_str()
                        ),
                        effective_policy,
                        approval_state: None,
                    });
                }
            }
            if completion_policy.require_approval {
                return evaluate_approval_gate(pool, request, effective_policy).await;
            }

            Ok(RiskyOperationEvaluation {
                outcome: RiskyOperationOutcome::Allowed,
                reason: format!(
                    "{} satisfied effective evidence requirements",
                    request.operation.as_str()
                ),
                effective_policy,
                approval_state: None,
            })
        }
        _ => match effective_policy
            .risky_operations
            .get(&request.operation)
            .copied()
            .unwrap_or(RiskDisposition::Deny)
        {
            RiskDisposition::Allow => Ok(RiskyOperationEvaluation {
                outcome: RiskyOperationOutcome::Allowed,
                reason: format!(
                    "{} is allowed by effective policy",
                    request.operation.as_str()
                ),
                effective_policy,
                approval_state: None,
            }),
            RiskDisposition::Deny => Ok(RiskyOperationEvaluation {
                outcome: RiskyOperationOutcome::Denied,
                reason: format!(
                    "{} is denied by effective policy",
                    request.operation.as_str()
                ),
                effective_policy,
                approval_state: None,
            }),
            RiskDisposition::RequireApproval => {
                evaluate_approval_gate(pool, request, effective_policy).await
            }
        },
    }
}

pub async fn create_policy_config(
    pool: &SqlitePool,
    input: CreatePolicyConfig<'_>,
) -> Result<PolicyConfig> {
    let scope_ref = normalize_scope_ref(input.scope_kind, input.scope_ref.to_string())?;
    let agent = normalize_agent(input.agent);
    let rules_json = normalize_json_object(input.rules_json, PolicyError::BadRulesJson)?;
    let now = Utc::now().to_rfc3339();

    sqlx::query(
        "INSERT INTO policy_configs (id, scope_kind, scope_ref, agent, enabled, enforcement_mode, rules_json, rationale, created_by, updated_by, created_at, updated_at) \
         VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)",
    )
    .bind(input.id)
    .bind(input.scope_kind.as_str())
    .bind(&scope_ref)
    .bind(agent.as_deref().unwrap_or(""))
    .bind(input.enabled)
    .bind(input.enforcement_mode.as_str())
    .bind(&rules_json)
    .bind(input.rationale)
    .bind(input.created_by)
    .bind(input.created_by)
    .bind(&now)
    .bind(&now)
    .execute(pool)
    .await?;

    get_policy_config(pool, input.id)
        .await?
        .ok_or_else(|| anyhow!("Policy config '{}' not found after creation", input.id))
}

pub async fn get_policy_config(pool: &SqlitePool, id: &str) -> Result<Option<PolicyConfig>> {
    let row = sqlx::query_as::<_, PolicyConfigRow>(
        "SELECT id, scope_kind, scope_ref, agent, enabled, enforcement_mode, rules_json, rationale, created_by, updated_by, created_at, updated_at \
         FROM policy_configs WHERE id = ?",
    )
    .bind(id)
    .fetch_optional(pool)
    .await?;

    row.map(TryInto::try_into).transpose()
}

pub async fn get_policy_config_by_scope_key(
    pool: &SqlitePool,
    key: &PolicyScopeKey,
) -> Result<Option<PolicyConfig>> {
    let row = sqlx::query_as::<_, PolicyConfigRow>(
        "SELECT id, scope_kind, scope_ref, agent, enabled, enforcement_mode, rules_json, rationale, created_by, updated_by, created_at, updated_at \
         FROM policy_configs WHERE scope_kind = ? AND scope_ref = ? AND agent = ?",
    )
    .bind(key.scope_kind.as_str())
    .bind(&key.scope_ref)
    .bind(key.agent.as_deref().unwrap_or(""))
    .fetch_optional(pool)
    .await?;

    row.map(TryInto::try_into).transpose()
}

pub async fn update_policy_config(
    pool: &SqlitePool,
    id: &str,
    input: UpdatePolicyConfig<'_>,
) -> Result<PolicyConfig> {
    let rules_json = normalize_json_object(input.rules_json, PolicyError::BadRulesJson)?;
    let now = Utc::now().to_rfc3339();

    sqlx::query(
        "UPDATE policy_configs \
         SET enabled = ?, enforcement_mode = ?, rules_json = ?, rationale = ?, updated_by = ?, updated_at = ? \
         WHERE id = ?",
    )
    .bind(input.enabled)
    .bind(input.enforcement_mode.as_str())
    .bind(&rules_json)
    .bind(input.rationale)
    .bind(input.updated_by)
    .bind(&now)
    .bind(id)
    .execute(pool)
    .await?;

    get_policy_config(pool, id)
        .await?
        .ok_or_else(|| anyhow!("Policy config '{}' not found", id))
}

pub async fn delete_policy_config(pool: &SqlitePool, id: &str) -> Result<bool> {
    let result = sqlx::query("DELETE FROM policy_configs WHERE id = ?")
        .bind(id)
        .execute(pool)
        .await?;
    Ok(result.rows_affected() > 0)
}

pub async fn list_policy_configs(
    pool: &SqlitePool,
    scope_kind: Option<PolicyScopeKind>,
    scope_ref: Option<&str>,
    agent: Option<Option<&str>>,
) -> Result<Vec<PolicyConfig>> {
    let mut qb = sqlx::QueryBuilder::<sqlx::Sqlite>::new(
        "SELECT id, scope_kind, scope_ref, agent, enabled, enforcement_mode, rules_json, rationale, created_by, updated_by, created_at, updated_at \
         FROM policy_configs WHERE 1=1",
    );

    if let Some(scope_kind) = scope_kind {
        qb.push(" AND scope_kind = ");
        qb.push_bind(scope_kind.as_str());
    }
    if let Some(scope_ref) = scope_ref {
        qb.push(" AND scope_ref = ");
        qb.push_bind(normalize_optional_scope_ref(scope_kind, scope_ref)?);
    }
    if let Some(agent_filter) = agent {
        let agent_filter = normalize_agent(agent_filter).unwrap_or_default();
        qb.push(" AND agent = ");
        qb.push_bind(agent_filter);
    }

    qb.push(" ORDER BY scope_kind, scope_ref, agent, id");
    let rows: Vec<PolicyConfigRow> = qb.build_query_as().fetch_all(pool).await?;
    rows.into_iter().map(TryInto::try_into).collect()
}

pub async fn list_policy_configs_for_resolution(
    pool: &SqlitePool,
    spec_ref: Option<&str>,
    task_ref: Option<&str>,
    agent: Option<&str>,
) -> Result<Vec<PolicyConfig>> {
    let mut query = String::from(
        "SELECT id, scope_kind, scope_ref, agent, enabled, enforcement_mode, rules_json, rationale, created_by, updated_by, created_at, updated_at \
         FROM policy_configs WHERE ((scope_kind = ? AND scope_ref = ?)",
    );
    if spec_ref.is_some() {
        query.push_str(" OR (scope_kind = ? AND scope_ref = ?)");
    }
    if task_ref.is_some() {
        query.push_str(" OR (scope_kind = ? AND scope_ref = ?)");
    }
    query.push_str(") AND agent IN (?, ?) ORDER BY CASE scope_kind WHEN 'project' THEN 0 WHEN 'spec' THEN 1 ELSE 2 END, CASE WHEN agent = '' THEN 0 ELSE 1 END, id",
    );

    let resolved_agent = normalize_agent(agent).unwrap_or_default();
    let mut sql = sqlx::query_as::<_, PolicyConfigRow>(&query)
        .bind(PolicyScopeKind::Project.as_str())
        .bind(PROJECT_SCOPE_REF);

    if let Some(spec_ref) = spec_ref {
        sql = sql
            .bind(PolicyScopeKind::Spec.as_str())
            .bind(normalize_scope_ref(
                PolicyScopeKind::Spec,
                spec_ref.to_string(),
            )?);
    }
    if let Some(task_ref) = task_ref {
        sql = sql
            .bind(PolicyScopeKind::Task.as_str())
            .bind(normalize_scope_ref(
                PolicyScopeKind::Task,
                task_ref.to_string(),
            )?);
    }

    let rows: Vec<PolicyConfigRow> = sql.bind("").bind(resolved_agent).fetch_all(pool).await?;
    rows.into_iter().map(TryInto::try_into).collect()
}

pub async fn create_approval(
    pool: &SqlitePool,
    input: CreateApproval<'_>,
) -> Result<ApprovalRecord> {
    let mut tx = pool.begin().await?;
    let record = create_approval_tx(&mut tx, input).await?;
    tx.commit().await?;
    Ok(record)
}

pub(crate) async fn create_approval_tx(
    tx: &mut Transaction<'_, Sqlite>,
    input: CreateApproval<'_>,
) -> Result<ApprovalRecord> {
    validate_approval_entity(input.entity_kind, input.entity_id, input.spec, input.task)?;
    let request_context_json =
        normalize_json_object(input.request_context_json, PolicyError::BadRequestContext)?;
    let now = Utc::now().to_rfc3339();

    sqlx::query(
        "INSERT INTO approvals (id, entity_kind, entity_id, spec, task, operation_kind, status, policy_config_id, evidence_bundle_id, requested_by, request_context_json, created_at, expires_at) \
         VALUES (?, ?, ?, ?, ?, ?, 'pending', ?, ?, ?, ?, ?, ?)",
    )
    .bind(input.id)
    .bind(input.entity_kind.as_str())
    .bind(input.entity_id)
    .bind(input.spec)
    .bind(input.task)
    .bind(input.operation_kind)
    .bind(input.policy_config_id)
    .bind(input.evidence_bundle_id)
    .bind(input.requested_by)
    .bind(&request_context_json)
    .bind(&now)
    .bind(input.expires_at)
    .execute(&mut **tx)
    .await?;

    get_approval_tx(tx, input.id)
        .await?
        .ok_or_else(|| anyhow!("Approval '{}' not found after creation", input.id))
}

pub async fn get_approval(pool: &SqlitePool, id: &str) -> Result<Option<ApprovalRecord>> {
    let row = sqlx::query_as::<_, ApprovalRow>(
        "SELECT id, entity_kind, entity_id, spec, task, operation_kind, status, policy_config_id, evidence_bundle_id, requested_by, decided_by, decision_reason, request_context_json, created_at, decided_at, expires_at \
         FROM approvals WHERE id = ?",
    )
    .bind(id)
    .fetch_optional(pool)
    .await?;

    row.map(TryInto::try_into).transpose()
}

pub async fn list_approvals(
    pool: &SqlitePool,
    entity_kind: Option<ApprovalEntityKind>,
    entity_id: Option<&str>,
    operation_kind: Option<&str>,
    status: Option<ApprovalStatus>,
) -> Result<Vec<ApprovalRecord>> {
    let mut qb = sqlx::QueryBuilder::<sqlx::Sqlite>::new(
        "SELECT id, entity_kind, entity_id, spec, task, operation_kind, status, policy_config_id, evidence_bundle_id, requested_by, decided_by, decision_reason, request_context_json, created_at, decided_at, expires_at \
         FROM approvals WHERE 1=1",
    );

    if let Some(entity_kind) = entity_kind {
        qb.push(" AND entity_kind = ");
        qb.push_bind(entity_kind.as_str());
    }
    if let Some(entity_id) = entity_id {
        qb.push(" AND entity_id = ");
        qb.push_bind(entity_id);
    }
    if let Some(operation_kind) = operation_kind {
        qb.push(" AND operation_kind = ");
        qb.push_bind(operation_kind);
    }
    if let Some(status) = status {
        qb.push(" AND status = ");
        qb.push_bind(status.as_str());
    }

    qb.push(" ORDER BY CASE WHEN status = 'pending' THEN 0 ELSE 1 END, created_at DESC, id DESC");
    let rows: Vec<ApprovalRow> = qb.build_query_as().fetch_all(pool).await?;
    rows.into_iter().map(TryInto::try_into).collect()
}

pub async fn decide_approval(
    pool: &SqlitePool,
    id: &str,
    decision: ApprovalDecision<'_>,
) -> Result<ApprovalRecord> {
    let mut tx = pool.begin().await?;
    let record = decide_approval_tx(&mut tx, id, decision).await?;
    tx.commit().await?;
    Ok(record)
}

pub(crate) async fn decide_approval_tx(
    tx: &mut Transaction<'_, Sqlite>,
    id: &str,
    decision: ApprovalDecision<'_>,
) -> Result<ApprovalRecord> {
    if !decision.status.is_terminal() {
        return Err(PolicyError::TransitionRejected {
            id: id.to_string(),
            current: ApprovalStatus::Pending.as_str(),
            next: decision.status.as_str(),
        }
        .into());
    }

    let existing = get_approval_tx(tx, id)
        .await?
        .ok_or_else(|| anyhow!("Approval '{}' not found", id))?;
    if existing.status != ApprovalStatus::Pending {
        return Err(PolicyError::TransitionRejected {
            id: id.to_string(),
            current: existing.status.as_str(),
            next: decision.status.as_str(),
        }
        .into());
    }

    let decided_at = Utc::now().to_rfc3339();
    sqlx::query(
        "UPDATE approvals SET status = ?, decided_by = ?, decision_reason = ?, decided_at = ? WHERE id = ?",
    )
    .bind(decision.status.as_str())
    .bind(decision.decided_by)
    .bind(decision.decision_reason)
    .bind(&decided_at)
    .bind(id)
    .execute(&mut **tx)
    .await?;

    get_approval_tx(tx, id)
        .await?
        .ok_or_else(|| anyhow!("Approval '{}' not found", id))
}

async fn get_approval_tx(
    tx: &mut Transaction<'_, Sqlite>,
    id: &str,
) -> Result<Option<ApprovalRecord>> {
    let row = sqlx::query_as::<_, ApprovalRow>(
        "SELECT id, entity_kind, entity_id, spec, task, operation_kind, status, policy_config_id, evidence_bundle_id, requested_by, decided_by, decision_reason, request_context_json, created_at, decided_at, expires_at \
         FROM approvals WHERE id = ?",
    )
    .bind(id)
    .fetch_optional(&mut **tx)
    .await?;

    row.map(TryInto::try_into).transpose()
}

pub async fn get_approval_state(
    pool: &SqlitePool,
    entity_kind: ApprovalEntityKind,
    entity_id: &str,
    operation_kind: &str,
) -> Result<ApprovalState> {
    let row = sqlx::query_as::<_, ApprovalRow>(
        "SELECT id, entity_kind, entity_id, spec, task, operation_kind, status, policy_config_id, evidence_bundle_id, requested_by, decided_by, decision_reason, request_context_json, created_at, decided_at, expires_at \
         FROM approvals \
         WHERE entity_kind = ? AND entity_id = ? AND operation_kind = ? \
         ORDER BY CASE WHEN status = 'pending' THEN 0 ELSE 1 END, COALESCE(decided_at, created_at) DESC, id DESC \
         LIMIT 1",
    )
    .bind(entity_kind.as_str())
    .bind(entity_id)
    .bind(operation_kind)
    .fetch_optional(pool)
    .await?;

    Ok(ApprovalState::from_latest(
        row.map(TryInto::try_into).transpose()?,
    ))
}

#[derive(sqlx::FromRow)]
struct PolicyConfigRow {
    id: String,
    scope_kind: String,
    scope_ref: String,
    agent: String,
    enabled: bool,
    enforcement_mode: String,
    rules_json: String,
    rationale: Option<String>,
    created_by: Option<String>,
    updated_by: Option<String>,
    created_at: String,
    updated_at: String,
}

impl TryFrom<PolicyConfigRow> for PolicyConfig {
    type Error = anyhow::Error;

    fn try_from(row: PolicyConfigRow) -> Result<Self> {
        Ok(Self {
            id: row.id,
            scope_kind: PolicyScopeKind::from_str(&row.scope_kind)
                .ok_or_else(|| anyhow!("Unknown policy scope kind '{}'", row.scope_kind))?,
            scope_ref: row.scope_ref,
            agent: empty_to_none(row.agent),
            enabled: row.enabled,
            enforcement_mode: EnforcementMode::from_str(&row.enforcement_mode)
                .ok_or_else(|| anyhow!("Unknown enforcement mode '{}'", row.enforcement_mode))?,
            rules_json: row.rules_json,
            rationale: row.rationale,
            created_by: row.created_by,
            updated_by: row.updated_by,
            created_at: row.created_at,
            updated_at: row.updated_at,
        })
    }
}

#[derive(sqlx::FromRow)]
struct ApprovalRow {
    id: String,
    entity_kind: String,
    entity_id: String,
    spec: Option<String>,
    task: Option<String>,
    operation_kind: String,
    status: String,
    policy_config_id: Option<String>,
    evidence_bundle_id: Option<String>,
    requested_by: String,
    decided_by: Option<String>,
    decision_reason: Option<String>,
    request_context_json: String,
    created_at: String,
    decided_at: Option<String>,
    expires_at: Option<String>,
}

impl TryFrom<ApprovalRow> for ApprovalRecord {
    type Error = anyhow::Error;

    fn try_from(row: ApprovalRow) -> Result<Self> {
        Ok(Self {
            id: row.id,
            entity_kind: ApprovalEntityKind::from_str(&row.entity_kind)
                .ok_or_else(|| anyhow!("Unknown approval entity kind '{}'", row.entity_kind))?,
            entity_id: row.entity_id,
            spec: row.spec,
            task: row.task,
            operation_kind: row.operation_kind,
            status: ApprovalStatus::from_str(&row.status)
                .ok_or_else(|| anyhow!("Unknown approval status '{}'", row.status))?,
            policy_config_id: row.policy_config_id,
            evidence_bundle_id: row.evidence_bundle_id,
            requested_by: row.requested_by,
            decided_by: row.decided_by,
            decision_reason: row.decision_reason,
            request_context_json: row.request_context_json,
            created_at: row.created_at,
            decided_at: row.decided_at,
            expires_at: row.expires_at,
        })
    }
}

fn normalize_optional_scope_ref(
    scope_kind: Option<PolicyScopeKind>,
    scope_ref: &str,
) -> Result<String> {
    match scope_kind {
        Some(scope_kind) => normalize_scope_ref(scope_kind, scope_ref.to_string()),
        None => Ok(scope_ref.trim().to_string()),
    }
}

fn normalize_scope_ref(scope_kind: PolicyScopeKind, scope_ref: String) -> Result<String> {
    let trimmed = scope_ref.trim().to_string();
    match scope_kind {
        PolicyScopeKind::Project if trimmed == PROJECT_SCOPE_REF => Ok(trimmed),
        PolicyScopeKind::Project => Err(PolicyError::BadScopeRef {
            scope_kind: scope_kind.as_str(),
            scope_ref,
        }
        .into()),
        _ if trimmed.is_empty() => Err(PolicyError::BadScopeRef {
            scope_kind: scope_kind.as_str(),
            scope_ref,
        }
        .into()),
        _ => Ok(trimmed),
    }
}

fn normalize_agent(agent: Option<&str>) -> Option<String> {
    agent.and_then(|value| {
        let trimmed = value.trim();
        (!trimmed.is_empty()).then(|| trimmed.to_string())
    })
}

impl CompletionPolicy {
    fn strict_default() -> Self {
        Self {
            require_evidence_bundle: true,
            require_rationale: true,
            require_validation: Some(ValidationRequirementLevel::Primary),
            require_approval: false,
        }
    }

    fn relaxed_default() -> Self {
        Self {
            require_evidence_bundle: false,
            require_rationale: false,
            require_validation: None,
            require_approval: false,
        }
    }

    fn apply_overlay(&mut self, overlay: CompletionOverlay) {
        if let Some(value) = overlay.require_evidence_bundle {
            self.require_evidence_bundle = value;
        }
        if let Some(value) = overlay.require_rationale {
            self.require_rationale = value;
        }
        if let Some(value) = overlay.require_validation {
            self.require_validation = Some(value);
        }
        if let Some(value) = overlay.require_approval {
            self.require_approval = value;
        }
    }
}

impl EffectivePolicy {
    fn defaults(
        spec_ref: Option<&str>,
        task_ref: Option<&str>,
        agent: Option<&str>,
        fail_closed: bool,
    ) -> Self {
        let mut risky_operations = BTreeMap::new();
        if fail_closed {
            risky_operations.insert(
                RiskyOperationKind::DestructiveCommand,
                RiskDisposition::RequireApproval,
            );
            risky_operations.insert(
                RiskyOperationKind::WriteOutsideAllowedScope,
                RiskDisposition::Deny,
            );
            risky_operations.insert(
                RiskyOperationKind::SchemaChange,
                RiskDisposition::RequireApproval,
            );
            risky_operations.insert(
                RiskyOperationKind::GlobalConfigChange,
                RiskDisposition::RequireApproval,
            );
            risky_operations.insert(RiskyOperationKind::CompleteTask, RiskDisposition::Deny);
            risky_operations.insert(RiskyOperationKind::CompleteSpec, RiskDisposition::Deny);
        } else {
            for operation in [
                RiskyOperationKind::DestructiveCommand,
                RiskyOperationKind::WriteOutsideAllowedScope,
                RiskyOperationKind::SchemaChange,
                RiskyOperationKind::GlobalConfigChange,
                RiskyOperationKind::CompleteTask,
                RiskyOperationKind::CompleteSpec,
            ] {
                risky_operations.insert(operation, RiskDisposition::Allow);
            }
        }

        Self {
            spec_ref: spec_ref.map(str::to_string),
            task_ref: task_ref.map(str::to_string),
            agent: normalize_agent(agent),
            enforcement_mode: if fail_closed {
                EnforcementMode::Enforced
            } else {
                EnforcementMode::Advisory
            },
            fail_closed,
            allowed_write_scopes: Vec::new(),
            task_completion: if fail_closed {
                CompletionPolicy::strict_default()
            } else {
                CompletionPolicy::relaxed_default()
            },
            spec_completion: if fail_closed {
                CompletionPolicy {
                    require_validation: Some(ValidationRequirementLevel::Full),
                    ..CompletionPolicy::strict_default()
                }
            } else {
                CompletionPolicy::relaxed_default()
            },
            risky_operations,
            sources: Vec::new(),
        }
    }

    fn apply_overlay(&mut self, overlay: PolicyOverlay) {
        if let Some(allowed_write_scopes) = overlay.allowed_write_scopes {
            self.allowed_write_scopes = allowed_write_scopes;
        }
        self.task_completion
            .apply_overlay(overlay.shared_completion);
        self.spec_completion
            .apply_overlay(overlay.shared_completion);
        self.task_completion.apply_overlay(overlay.task_completion);
        self.spec_completion.apply_overlay(overlay.spec_completion);
        for (operation, disposition) in overlay.risky_operations {
            self.risky_operations.insert(operation, disposition);
        }
    }
}

#[derive(Debug, Clone, Copy, Default)]
struct CompletionOverlay {
    require_evidence_bundle: Option<bool>,
    require_rationale: Option<bool>,
    require_validation: Option<ValidationRequirementLevel>,
    require_approval: Option<bool>,
}

#[derive(Debug, Default)]
struct PolicyOverlay {
    allowed_write_scopes: Option<Vec<String>>,
    shared_completion: CompletionOverlay,
    task_completion: CompletionOverlay,
    spec_completion: CompletionOverlay,
    risky_operations: BTreeMap<RiskyOperationKind, RiskDisposition>,
}

fn parse_policy_overlay(value: &Value) -> Result<PolicyOverlay> {
    let object = value
        .as_object()
        .ok_or_else(|| anyhow!("policy rules_json must be a JSON object"))?;
    let mut overlay = PolicyOverlay::default();

    if let Some(scopes) = object.get("allowed_write_scopes") {
        overlay.allowed_write_scopes = Some(parse_allowed_write_scopes(scopes)?);
    }
    if let Some(required) = object.get("require_evidence_bundle") {
        overlay.shared_completion.require_evidence_bundle =
            Some(parse_bool_rule(required, "require_evidence_bundle")?);
    }
    if let Some(required) = object.get("require_rationale") {
        overlay.shared_completion.require_rationale =
            Some(parse_bool_rule(required, "require_rationale")?);
    }
    if let Some(required) = object.get("require_approval") {
        overlay.shared_completion.require_approval =
            Some(parse_bool_rule(required, "require_approval")?);
    }
    if let Some(validation) = object.get("require_validation") {
        overlay.shared_completion.require_validation = Some(parse_validation_requirement(
            validation,
            "require_validation",
        )?);
    }
    if let Some(task_completion) = object.get("task_completion") {
        overlay.task_completion = parse_completion_overlay(task_completion, "task_completion")?;
    }
    if let Some(spec_completion) = object.get("spec_completion") {
        overlay.spec_completion = parse_completion_overlay(spec_completion, "spec_completion")?;
    }
    if let Some(risky_operations) = object.get("risky_operations") {
        overlay.risky_operations = parse_risky_operations(risky_operations)?;
    }

    Ok(overlay)
}

fn parse_allowed_write_scopes(value: &Value) -> Result<Vec<String>> {
    let scopes = value
        .as_array()
        .ok_or_else(|| anyhow!("allowed_write_scopes must be an array of strings"))?;
    scopes
        .iter()
        .map(|scope| {
            let scope = scope
                .as_str()
                .ok_or_else(|| anyhow!("allowed_write_scopes entries must be strings"))?;
            let normalized = normalize_scope_path(scope)?;
            if normalized.is_empty() {
                return Err(anyhow!("allowed_write_scopes entries cannot be empty"));
            }
            Ok(normalized)
        })
        .collect()
}

fn parse_completion_overlay(value: &Value, field_name: &str) -> Result<CompletionOverlay> {
    let object = value
        .as_object()
        .ok_or_else(|| anyhow!("{} must be a JSON object", field_name))?;
    let mut overlay = CompletionOverlay::default();

    if let Some(required) = object.get("require_evidence_bundle") {
        overlay.require_evidence_bundle = Some(parse_bool_rule(
            required,
            &format!("{}.require_evidence_bundle", field_name),
        )?);
    }
    if let Some(required) = object.get("require_rationale") {
        overlay.require_rationale = Some(parse_bool_rule(
            required,
            &format!("{}.require_rationale", field_name),
        )?);
    }
    if let Some(required) = object.get("require_approval") {
        overlay.require_approval = Some(parse_bool_rule(
            required,
            &format!("{}.require_approval", field_name),
        )?);
    }
    if let Some(validation) = object.get("require_validation") {
        overlay.require_validation = Some(parse_validation_requirement(
            validation,
            &format!("{}.require_validation", field_name),
        )?);
    }

    Ok(overlay)
}

fn parse_risky_operations(value: &Value) -> Result<BTreeMap<RiskyOperationKind, RiskDisposition>> {
    let object = value
        .as_object()
        .ok_or_else(|| anyhow!("risky_operations must be a JSON object"))?;
    let mut rules = BTreeMap::new();

    for (key, value) in object {
        let operation = RiskyOperationKind::from_str(key)
            .ok_or_else(|| anyhow!("unknown risky operation '{}'", key))?;
        let disposition_value = value
            .as_str()
            .ok_or_else(|| anyhow!("risky operation '{}' must map to a string", key))?;
        let disposition = RiskDisposition::from_str(disposition_value).ok_or_else(|| {
            anyhow!(
                "invalid disposition '{}' for risky operation '{}'",
                disposition_value,
                key
            )
        })?;
        rules.insert(operation, disposition);
    }

    Ok(rules)
}

fn parse_bool_rule(value: &Value, field_name: &str) -> Result<bool> {
    value
        .as_bool()
        .ok_or_else(|| anyhow!("{} must be a boolean", field_name))
}

fn parse_validation_requirement(
    value: &Value,
    field_name: &str,
) -> Result<ValidationRequirementLevel> {
    let value = value
        .as_str()
        .ok_or_else(|| anyhow!("{} must be a string", field_name))?;
    match value {
        "fast" => Ok(ValidationRequirementLevel::Fast),
        "primary" => Ok(ValidationRequirementLevel::Primary),
        "full" => Ok(ValidationRequirementLevel::Full),
        "custom" => Ok(ValidationRequirementLevel::Custom),
        _ => Err(anyhow!(
            "invalid validation requirement '{}' for {}",
            value,
            field_name
        )),
    }
}

fn normalize_scope_path(value: &str) -> Result<String> {
    let raw = value.trim();
    if raw.is_empty() {
        return Ok(String::new());
    }
    if Path::new(raw).is_absolute() {
        return Err(anyhow!(
            "policy path '{}' must be relative to the project root",
            raw
        ));
    }

    let mut components = Vec::new();
    for component in Path::new(raw).components() {
        match component {
            std::path::Component::Normal(part) => {
                components.push(part.to_string_lossy().into_owned())
            }
            std::path::Component::CurDir => {}
            std::path::Component::ParentDir => {
                return Err(anyhow!(
                    "policy path '{}' cannot escape the project root",
                    raw
                ))
            }
            _ => return Err(anyhow!("policy path '{}' is invalid", raw)),
        }
    }

    Ok(components.join("/"))
}

fn is_path_within_allowed_scopes(path: &str, scopes: &[String]) -> bool {
    let Ok(normalized_path) = normalize_scope_path(path) else {
        return false;
    };

    !normalized_path.is_empty()
        && scopes.iter().any(|scope| {
            normalized_path == *scope
                || normalized_path
                    .strip_prefix(scope)
                    .is_some_and(|suffix| suffix.starts_with('/'))
        })
}

fn policy_rollout_applies(spec_status: &str) -> bool {
    matches!(spec_status, "approved" | "in_progress" | "paused" | "done")
}

fn validation_rank(level: ValidationRequirementLevel) -> u8 {
    match level {
        ValidationRequirementLevel::Fast => 1,
        ValidationRequirementLevel::Primary => 2,
        ValidationRequirementLevel::Full => 3,
        ValidationRequirementLevel::Custom => 4,
    }
}

fn validation_satisfies(
    actual: Option<ValidationRequirementLevel>,
    required: ValidationRequirementLevel,
) -> bool {
    match (actual, required) {
        (Some(ValidationRequirementLevel::Custom), ValidationRequirementLevel::Custom) => true,
        (Some(ValidationRequirementLevel::Custom), _) => false,
        (Some(actual), ValidationRequirementLevel::Custom) => {
            actual == ValidationRequirementLevel::Custom
        }
        (Some(actual), required) => validation_rank(actual) >= validation_rank(required),
        (None, _) => false,
    }
}

async fn evaluate_approval_gate(
    pool: &SqlitePool,
    request: RiskyOperationRequest<'_>,
    effective_policy: EffectivePolicy,
) -> Result<RiskyOperationEvaluation> {
    let approval_state = get_approval_state(
        pool,
        request.entity_kind,
        request.entity_id,
        request.operation.as_str(),
    )
    .await?;

    let (outcome, reason) = match &approval_state {
        ApprovalState::Approved(_) => (
            RiskyOperationOutcome::Allowed,
            format!(
                "{} has approved policy clearance",
                request.operation.as_str()
            ),
        ),
        ApprovalState::Pending(_) => (
            RiskyOperationOutcome::ApprovalRequired,
            format!(
                "{} is waiting on human approval",
                request.operation.as_str()
            ),
        ),
        ApprovalState::NotRequested => (
            RiskyOperationOutcome::ApprovalRequired,
            format!("{} requires human approval", request.operation.as_str()),
        ),
        ApprovalState::Rejected(_) => (
            RiskyOperationOutcome::Denied,
            format!("{} approval was rejected", request.operation.as_str()),
        ),
        ApprovalState::Cancelled(_) => (
            RiskyOperationOutcome::Denied,
            format!("{} approval was cancelled", request.operation.as_str()),
        ),
        ApprovalState::Expired(_) => (
            RiskyOperationOutcome::Denied,
            format!("{} approval expired", request.operation.as_str()),
        ),
    };

    Ok(RiskyOperationEvaluation {
        outcome,
        reason,
        effective_policy,
        approval_state: Some(approval_state),
    })
}

fn normalize_json_object(value: &Value, error: PolicyError) -> Result<String> {
    if !value.is_object() {
        return Err(error.into());
    }
    Ok(serde_json::to_string(value)?)
}

fn empty_to_none(value: String) -> Option<String> {
    (!value.is_empty()).then_some(value)
}

fn validate_approval_entity(
    entity_kind: ApprovalEntityKind,
    entity_id: &str,
    spec: Option<&str>,
    task: Option<&str>,
) -> Result<()> {
    match entity_kind {
        ApprovalEntityKind::Task if task == Some(entity_id) => Ok(()),
        ApprovalEntityKind::Spec if spec == Some(entity_id) && task.is_none() => Ok(()),
        ApprovalEntityKind::Operation => Ok(()),
        _ => Err(PolicyError::BadApprovalEntity(format!(
            "entity_kind={}, entity_id={}, spec={:?}, task={:?}",
            entity_kind.as_str(),
            entity_id,
            spec,
            task,
        ))
        .into()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::sdd::spec::create_spec;
    use crate::sdd::task::create_task;
    use crate::sdd::test_helpers::make_pool;
    use serde_json::json;

    async fn setup_task_fixture(pool: &SqlitePool) {
        create_spec(pool, "SPEC-003", "Policy Spec", "P0", &[])
            .await
            .unwrap();
        create_task(
            pool,
            "T022",
            "SPEC-003",
            "Policy task",
            "sdd-builder",
            &[],
            None,
        )
        .await
        .unwrap();
    }

    #[tokio::test]
    async fn create_and_query_policy_config_by_scope_key() {
        let pool = make_pool().await;
        let policy = create_policy_config(
            &pool,
            CreatePolicyConfig {
                id: "pcfg-1",
                scope_kind: PolicyScopeKind::Spec,
                scope_ref: "SPEC-003",
                agent: Some("sdd-builder"),
                enabled: true,
                enforcement_mode: EnforcementMode::Enforced,
                rules_json: &json!({"require_validation": "primary"}),
                rationale: Some("Gate completions"),
                created_by: Some("architect"),
            },
        )
        .await
        .unwrap();

        assert_eq!(policy.scope_kind, PolicyScopeKind::Spec);
        assert_eq!(policy.agent.as_deref(), Some("sdd-builder"));
        assert_eq!(policy.rules().unwrap()["require_validation"], "primary");

        let fetched = get_policy_config_by_scope_key(
            &pool,
            &PolicyScopeKey::new(PolicyScopeKind::Spec, "SPEC-003", Some("sdd-builder")).unwrap(),
        )
        .await
        .unwrap()
        .unwrap();
        assert_eq!(fetched.id, "pcfg-1");
    }

    #[tokio::test]
    async fn update_policy_config_persists_new_rules_and_metadata() {
        let pool = make_pool().await;
        create_policy_config(
            &pool,
            CreatePolicyConfig {
                id: "pcfg-2",
                scope_kind: PolicyScopeKind::Project,
                scope_ref: PROJECT_SCOPE_REF,
                agent: None,
                enabled: true,
                enforcement_mode: EnforcementMode::Advisory,
                rules_json: &json!({"tools": ["bash"]}),
                rationale: None,
                created_by: Some("architect"),
            },
        )
        .await
        .unwrap();

        let updated = update_policy_config(
            &pool,
            "pcfg-2",
            UpdatePolicyConfig {
                enabled: false,
                enforcement_mode: EnforcementMode::Enforced,
                rules_json: &json!({"tools": ["bash"], "approval_required": true}),
                rationale: Some("Rollout hardened"),
                updated_by: Some("architect"),
            },
        )
        .await
        .unwrap();

        assert!(!updated.enabled);
        assert_eq!(updated.enforcement_mode, EnforcementMode::Enforced);
        assert_eq!(updated.rules().unwrap()["approval_required"], true);
        assert_eq!(updated.updated_by.as_deref(), Some("architect"));
    }

    #[tokio::test]
    async fn list_policy_configs_for_resolution_orders_scope_then_agent_specificity() {
        let pool = make_pool().await;
        let configs = [
            (
                "project-base",
                PolicyScopeKind::Project,
                PROJECT_SCOPE_REF,
                None,
            ),
            (
                "project-agent",
                PolicyScopeKind::Project,
                PROJECT_SCOPE_REF,
                Some("sdd-builder"),
            ),
            ("spec-base", PolicyScopeKind::Spec, "SPEC-003", None),
            (
                "task-agent",
                PolicyScopeKind::Task,
                "T022",
                Some("sdd-builder"),
            ),
        ];

        for (id, scope_kind, scope_ref, agent) in configs {
            create_policy_config(
                &pool,
                CreatePolicyConfig {
                    id,
                    scope_kind,
                    scope_ref,
                    agent,
                    enabled: true,
                    enforcement_mode: EnforcementMode::Enforced,
                    rules_json: &json!({"id": id}),
                    rationale: None,
                    created_by: Some("architect"),
                },
            )
            .await
            .unwrap();
        }

        let resolved = list_policy_configs_for_resolution(
            &pool,
            Some("SPEC-003"),
            Some("T022"),
            Some("sdd-builder"),
        )
        .await
        .unwrap();
        let ids: Vec<&str> = resolved.iter().map(|config| config.id.as_str()).collect();
        assert_eq!(
            ids,
            vec!["project-base", "project-agent", "spec-base", "task-agent"]
        );
    }

    #[tokio::test]
    async fn resolve_effective_policy_applies_precedence_and_overrides() {
        let pool = make_pool().await;

        create_policy_config(
            &pool,
            CreatePolicyConfig {
                id: "project-base",
                scope_kind: PolicyScopeKind::Project,
                scope_ref: PROJECT_SCOPE_REF,
                agent: None,
                enabled: true,
                enforcement_mode: EnforcementMode::Enforced,
                rules_json: &json!({
                    "allowed_write_scopes": ["src"],
                    "require_validation": "primary",
                    "risky_operations": {
                        "global_config_change": "require_approval"
                    }
                }),
                rationale: None,
                created_by: Some("architect"),
            },
        )
        .await
        .unwrap();

        create_policy_config(
            &pool,
            CreatePolicyConfig {
                id: "spec-override",
                scope_kind: PolicyScopeKind::Spec,
                scope_ref: "SPEC-003",
                agent: None,
                enabled: true,
                enforcement_mode: EnforcementMode::Enforced,
                rules_json: &json!({
                    "task_completion": {
                        "require_validation": "full"
                    }
                }),
                rationale: None,
                created_by: Some("architect"),
            },
        )
        .await
        .unwrap();

        create_policy_config(
            &pool,
            CreatePolicyConfig {
                id: "task-agent-override",
                scope_kind: PolicyScopeKind::Task,
                scope_ref: "T024",
                agent: Some("sdd-builder"),
                enabled: true,
                enforcement_mode: EnforcementMode::Enforced,
                rules_json: &json!({
                    "allowed_write_scopes": ["src/sdd"],
                    "task_completion": {
                        "require_approval": true
                    },
                    "risky_operations": {
                        "destructive_command": "deny"
                    }
                }),
                rationale: None,
                created_by: Some("architect"),
            },
        )
        .await
        .unwrap();

        let effective = resolve_effective_policy(
            &pool,
            Some("SPEC-003"),
            Some("T024"),
            Some("sdd-builder"),
            "approved",
        )
        .await
        .unwrap();

        assert_eq!(effective.allowed_write_scopes, vec!["src/sdd"]);
        assert_eq!(
            effective.task_completion.require_validation,
            Some(ValidationRequirementLevel::Full)
        );
        assert!(effective.task_completion.require_approval);
        assert_eq!(
            effective
                .risky_operations
                .get(&RiskyOperationKind::DestructiveCommand),
            Some(&RiskDisposition::Deny)
        );
        let source_ids: Vec<&str> = effective
            .sources
            .iter()
            .map(|source| source.id.as_str())
            .collect();
        assert_eq!(
            source_ids,
            vec!["project-base", "spec-override", "task-agent-override"]
        );
    }

    #[tokio::test]
    async fn evaluate_risky_operation_fails_closed_for_write_scope_on_approved_specs() {
        let pool = make_pool().await;

        let evaluation = evaluate_risky_operation(
            &pool,
            RiskyOperationRequest {
                spec_status: "approved",
                spec_ref: Some("SPEC-003"),
                task_ref: Some("T024"),
                agent: Some("sdd-builder"),
                entity_kind: ApprovalEntityKind::Operation,
                entity_id: "op-write-1",
                operation: RiskyOperationKind::WriteOutsideAllowedScope,
                write_path: Some("README.md"),
                completion_evidence: None,
            },
        )
        .await
        .unwrap();

        assert_eq!(evaluation.outcome, RiskyOperationOutcome::Denied);
        assert!(evaluation.reason.contains("outside allowed policy scope"));
        assert!(evaluation.effective_policy.fail_closed);
    }

    #[tokio::test]
    async fn evaluate_risky_operation_fails_closed_on_invalid_policy_input() {
        let pool = make_pool().await;
        create_policy_config(
            &pool,
            CreatePolicyConfig {
                id: "bad-policy",
                scope_kind: PolicyScopeKind::Spec,
                scope_ref: "SPEC-003",
                agent: None,
                enabled: true,
                enforcement_mode: EnforcementMode::Enforced,
                rules_json: &json!({
                    "require_validation": "bogus"
                }),
                rationale: None,
                created_by: Some("architect"),
            },
        )
        .await
        .unwrap();

        let evaluation = evaluate_risky_operation(
            &pool,
            RiskyOperationRequest {
                spec_status: "approved",
                spec_ref: Some("SPEC-003"),
                task_ref: Some("T024"),
                agent: Some("sdd-builder"),
                entity_kind: ApprovalEntityKind::Operation,
                entity_id: "op-destroy-1",
                operation: RiskyOperationKind::DestructiveCommand,
                write_path: None,
                completion_evidence: None,
            },
        )
        .await
        .unwrap();

        assert_eq!(evaluation.outcome, RiskyOperationOutcome::Denied);
        assert!(evaluation.reason.contains("invalid policy input"));
        assert!(evaluation.reason.contains("failing closed"));
    }

    #[tokio::test]
    async fn evaluate_task_completion_requires_evidence_before_allowing_close() {
        let pool = make_pool().await;

        let denied = evaluate_risky_operation(
            &pool,
            RiskyOperationRequest {
                spec_status: "approved",
                spec_ref: Some("SPEC-003"),
                task_ref: Some("T024"),
                agent: Some("sdd-builder"),
                entity_kind: ApprovalEntityKind::Task,
                entity_id: "T024",
                operation: RiskyOperationKind::CompleteTask,
                write_path: None,
                completion_evidence: Some(CompletionEvidence {
                    has_evidence_bundle: false,
                    has_rationale: false,
                    satisfied_validation: None,
                }),
            },
        )
        .await
        .unwrap();
        assert_eq!(denied.outcome, RiskyOperationOutcome::Denied);
        assert!(denied.reason.contains("evidence bundle"));

        let allowed = evaluate_risky_operation(
            &pool,
            RiskyOperationRequest {
                spec_status: "approved",
                spec_ref: Some("SPEC-003"),
                task_ref: Some("T024"),
                agent: Some("sdd-builder"),
                entity_kind: ApprovalEntityKind::Task,
                entity_id: "T024",
                operation: RiskyOperationKind::CompleteTask,
                write_path: None,
                completion_evidence: Some(CompletionEvidence {
                    has_evidence_bundle: true,
                    has_rationale: true,
                    satisfied_validation: Some(ValidationRequirementLevel::Primary),
                }),
            },
        )
        .await
        .unwrap();
        assert_eq!(allowed.outcome, RiskyOperationOutcome::Allowed);
        assert!(allowed
            .reason
            .contains("satisfied effective evidence requirements"));
    }

    #[tokio::test]
    async fn create_policy_config_rejects_invalid_project_scope_ref() {
        let pool = make_pool().await;
        let error = create_policy_config(
            &pool,
            CreatePolicyConfig {
                id: "pcfg-bad",
                scope_kind: PolicyScopeKind::Project,
                scope_ref: "SPEC-003",
                agent: None,
                enabled: true,
                enforcement_mode: EnforcementMode::Enforced,
                rules_json: &json!({}),
                rationale: None,
                created_by: None,
            },
        )
        .await
        .unwrap_err();

        assert!(error.to_string().contains("invalid scope reference"));
    }

    #[tokio::test]
    async fn create_and_decide_approval_updates_state() {
        let pool = make_pool().await;
        setup_task_fixture(&pool).await;

        let approval = create_approval(
            &pool,
            CreateApproval {
                id: "approval-1",
                entity_kind: ApprovalEntityKind::Task,
                entity_id: "T022",
                spec: Some("SPEC-003"),
                task: Some("T022"),
                operation_kind: "complete_task",
                policy_config_id: None,
                evidence_bundle_id: None,
                requested_by: "sdd-builder",
                request_context_json: &json!({"reason": "behavior change"}),
                expires_at: None,
            },
        )
        .await
        .unwrap();

        assert_eq!(approval.status, ApprovalStatus::Pending);
        assert_eq!(
            approval.request_context().unwrap()["reason"],
            "behavior change"
        );
        assert!(matches!(
            get_approval_state(&pool, ApprovalEntityKind::Task, "T022", "complete_task")
                .await
                .unwrap(),
            ApprovalState::Pending(_)
        ));

        let decided = decide_approval(
            &pool,
            "approval-1",
            ApprovalDecision {
                status: ApprovalStatus::Approved,
                decided_by: "human-reviewer",
                decision_reason: Some("Looks good"),
            },
        )
        .await
        .unwrap();

        assert_eq!(decided.status, ApprovalStatus::Approved);
        assert_eq!(decided.decided_by.as_deref(), Some("human-reviewer"));
        assert!(matches!(
            get_approval_state(&pool, ApprovalEntityKind::Task, "T022", "complete_task")
                .await
                .unwrap(),
            ApprovalState::Approved(_)
        ));
    }

    #[tokio::test]
    async fn decide_approval_rejects_non_pending_transition() {
        let pool = make_pool().await;
        setup_task_fixture(&pool).await;
        create_approval(
            &pool,
            CreateApproval {
                id: "approval-2",
                entity_kind: ApprovalEntityKind::Task,
                entity_id: "T022",
                spec: Some("SPEC-003"),
                task: Some("T022"),
                operation_kind: "complete_task_v2",
                policy_config_id: None,
                evidence_bundle_id: None,
                requested_by: "sdd-builder",
                request_context_json: &json!({"reason": "behavior change"}),
                expires_at: None,
            },
        )
        .await
        .unwrap();

        decide_approval(
            &pool,
            "approval-2",
            ApprovalDecision {
                status: ApprovalStatus::Rejected,
                decided_by: "human-reviewer",
                decision_reason: Some("Need more tests"),
            },
        )
        .await
        .unwrap();

        let error = decide_approval(
            &pool,
            "approval-2",
            ApprovalDecision {
                status: ApprovalStatus::Approved,
                decided_by: "human-reviewer",
                decision_reason: Some("changed mind"),
            },
        )
        .await
        .unwrap_err();

        assert!(error.to_string().contains("cannot transition"));
    }

    #[tokio::test]
    async fn create_approval_rejects_invalid_entity_linkage() {
        let pool = make_pool().await;
        let error = create_approval(
            &pool,
            CreateApproval {
                id: "approval-bad",
                entity_kind: ApprovalEntityKind::Spec,
                entity_id: "SPEC-003",
                spec: None,
                task: Some("T022"),
                operation_kind: "complete_spec",
                policy_config_id: None,
                evidence_bundle_id: None,
                requested_by: "sdd-builder",
                request_context_json: &json!({}),
                expires_at: None,
            },
        )
        .await
        .unwrap_err();

        assert!(error
            .to_string()
            .contains("invalid approval entity linkage"));
    }
}
