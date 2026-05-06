mod cli;
mod config;
mod doctor;
mod host;
mod mcp;
mod scaffold;
mod sdd;
mod skills_mgr;
pub mod webhooks;

use anyhow::Result;
use clap::{CommandFactory, Parser, Subcommand};
use clap_complete::{generate, Shell};

use cli::{
    brief::cmd_brief,
    doctor::cmd_doctor,
    eval::{
        cmd_eval_compare, cmd_eval_create, cmd_eval_list, cmd_eval_show, EvalCompareOptions,
        EvalCreateOptions, EvalListOptions,
    },
    mcp_cmd::{cmd_mcp_serve, cmd_mcp_setup},
    memory_cmd::{
        cmd_memory_gc, cmd_memory_list, cmd_memory_search, cmd_memory_set, cmd_memory_show,
        MemorySetOpts,
    },
    plan::{cmd_plan_build, cmd_plan_show},
    policy::{
        cmd_policy_approval_approve, cmd_policy_approval_list, cmd_policy_approval_reject,
        cmd_policy_config_list, cmd_policy_config_set, cmd_policy_config_show,
        cmd_policy_evidence_record_validation, cmd_policy_evidence_show,
        cmd_policy_evidence_submit,
    },
    pulse::cmd_pulse,
    readiness::{
        cmd_readiness_add_requirement, cmd_readiness_approve, cmd_readiness_enter_review,
        cmd_readiness_operator, cmd_readiness_phase, cmd_readiness_satisfy_requirement,
        cmd_readiness_spec,
    },
    session::{
        cmd_session_checkpoint, cmd_session_checkpoints, cmd_session_end, cmd_session_list,
        cmd_session_restore, cmd_session_start,
    },
    skill_cmd::{cmd_setup, cmd_skill_install, cmd_skill_list},
    spec::{
        cmd_spec_add, cmd_spec_approve, cmd_spec_done, cmd_spec_list, cmd_spec_show, cmd_spec_start,
    },
    task::{cmd_task_add, cmd_task_done, cmd_task_fail, cmd_task_list, cmd_task_start},
    trace::cmd_trace,
    update::cmd_update,
    workspace::cmd_workspace_status,
};
use sdd::db::open_project_db;

// ─── CLI Structures ──────────────────────────────────────────────────────────

#[derive(Parser)]
#[command(
    name = "spex",
    about = "Spec-Driven Development CLI — manage AI-assisted development workflows",
    version,
    propagate_version = true
)]
pub struct Cli {
    #[command(subcommand)]
    pub command: Commands,
}

#[derive(Subcommand)]
pub enum Commands {
    /// Initialise spex in the current directory (existing project)
    Init,

    /// One-time global setup: install bundled agents and write MCP config
    Setup {
        /// Target host: opencode, copilot, vscode, or pi (interactive picker if omitted)
        #[arg(long)]
        host: Option<String>,
    },

    /// Bootstrap a new spex project
    New {
        /// Project name
        name: String,
        /// Skip confirmation prompts
        #[arg(short, long)]
        yes: bool,
    },

    /// Manage specs (feature slices)
    Spec {
        #[command(subcommand)]
        cmd: SpecCmd,
    },

    /// Manage implementation plans
    Plan {
        #[command(subcommand)]
        cmd: PlanCmd,
    },

    /// Manage tasks
    Task {
        #[command(subcommand)]
        cmd: TaskCmd,
    },

    /// Manage policy configs, evidence bundles, and approvals
    Policy {
        #[command(subcommand)]
        cmd: PolicyCmd,
    },

    /// Manage eval runs and scorecards
    Eval {
        #[command(subcommand)]
        cmd: EvalCmd,
    },

    /// Show project status dashboard
    Pulse {
        /// Show events since this timestamp or duration (e.g. 2026-01-01 or 1h)
        #[arg(long)]
        since: Option<String>,
        /// Show events until this timestamp
        #[arg(long)]
        until: Option<String>,
    },

    /// Print a compact project brief for AI session kickoff
    Brief {
        /// Output as JSON instead of markdown
        #[arg(long)]
        json: bool,
    },

    /// Show domain event log
    Trace {
        #[arg(long)]
        spec: Option<String>,
        #[arg(long)]
        agent: Option<String>,
        #[arg(long)]
        task: Option<String>,
        #[arg(long)]
        full: bool,
        #[arg(long)]
        limit: Option<i64>,
        #[arg(long)]
        offset: Option<i64>,
    },

    /// MCP server commands
    Mcp {
        #[command(subcommand)]
        cmd: McpCmd,
    },

    #[command(
        about = "Bundled agent management",
        long_about = "Manage bundled agents installed under the host agents directory.\n\nFor OpenCode: ~/.config/opencode/agents/\nFor GitHub Copilot CLI: ~/.copilot/agents/\nFor VS Code: no per-agent files (MCP config only)\nFor Pi / pi-subagents: ~/.pi/agent/agents/\n\nThis command group does not manage generated custom skills. Custom skills remain separate `SKILL.md` files under ~/.agents/skills/<slug>/SKILL.md."
    )]
    Skill {
        #[command(subcommand)]
        cmd: SkillCmd,
    },

    /// Run health checks
    Doctor {
        /// Attempt automatic fixes
        #[arg(long)]
        fix: bool,
    },

    /// Generate shell completion scripts
    Completions {
        /// Shell to generate completions for
        #[arg(value_enum)]
        shell: Shell,
    },

    /// Update spex to the latest release
    Update {
        /// Only check for a newer version without installing
        #[arg(long)]
        check: bool,
    },

    /// Manage agent memory entries
    Memory {
        #[command(subcommand)]
        cmd: MemoryCmd,
    },

    /// Query status across multiple spex workspaces (read-only)
    Workspace {
        #[command(subcommand)]
        cmd: WorkspaceCmd,
    },

    /// Manage agent/human work sessions
    Session {
        #[command(subcommand)]
        cmd: SessionCmd,
    },

    /// Manage workflow phases, review requirements, and readiness reports
    Readiness {
        #[command(subcommand)]
        cmd: ReadinessCmd,
    },
}

// ─── Spec Subcommands ─────────────────────────────────────────────────────────

#[derive(Subcommand)]
pub enum SpecCmd {
    /// Add a new spec
    Add {
        id: String,
        title: String,
        #[arg(short, long, default_value = "P1")]
        priority: String,
    },
    /// Approve a spec (human gate)
    Approve { id: String },
    /// Start working on a spec
    Start { id: String },
    /// Mark spec as done
    Done { id: String },
    /// List all specs
    List {
        #[arg(long)]
        json: bool,
        #[arg(long)]
        limit: Option<i64>,
        #[arg(long)]
        offset: Option<i64>,
    },
    /// Show spec details
    Show { id: String },
}

// ─── Plan Subcommands ─────────────────────────────────────────────────────────

#[derive(Subcommand)]
pub enum PlanCmd {
    /// Decompose a spec into tasks (adds tasks via prompts)
    Build { spec_id: String },
    /// Show the plan for a spec
    Show { spec_id: String },
}

// ─── Task Subcommands ─────────────────────────────────────────────────────────

#[derive(Subcommand)]
pub enum TaskCmd {
    /// Add a task to a spec
    Add {
        spec_id: String,
        task_id: String,
        title: String,
        #[arg(long)]
        agent: String,
        #[arg(long)]
        inputs: Vec<String>,
        #[arg(long)]
        output_artifact: Option<String>,
    },
    /// Start a task
    Start {
        id: String,
        #[arg(long)]
        updated_by: String,
    },
    /// Mark task as done
    Done {
        id: String,
        #[arg(long)]
        updated_by: String,
    },
    /// Mark task as failed
    Fail { id: String },
    /// List tasks
    List {
        spec_id: Option<String>,
        #[arg(long)]
        json: bool,
        #[arg(long)]
        limit: Option<i64>,
        #[arg(long)]
        offset: Option<i64>,
    },
}

// ─── Policy Subcommands ───────────────────────────────────────────────────────

#[derive(Subcommand)]
pub enum PolicyCmd {
    /// Manage persisted policy configs
    Config {
        #[command(subcommand)]
        cmd: PolicyConfigCmd,
    },
    /// Submit or inspect evidence bundles
    Evidence {
        #[command(subcommand)]
        cmd: PolicyEvidenceCmd,
    },
    /// Inspect or decide pending approvals
    Approval {
        #[command(subcommand)]
        cmd: PolicyApprovalCmd,
    },
}

#[derive(Subcommand)]
pub enum EvalCmd {
    /// Create a structured eval run
    Create {
        #[arg(long)]
        id: Option<String>,
        #[arg(long)]
        evaluator: String,
        #[arg(long, value_parser = ["spec", "task", "artifact", "scope"])]
        target_kind: String,
        #[arg(long)]
        target_ref: String,
        #[arg(long)]
        spec: Option<String>,
        #[arg(long)]
        task: Option<String>,
        #[arg(long)]
        artifact_id: Option<String>,
        #[arg(long)]
        summary: Option<String>,
        #[arg(long)]
        rationale: Option<String>,
        #[arg(long, value_parser = ["pass", "warn", "fail", "mixed", "unknown"])]
        outcome: String,
        #[arg(long)]
        overall_score: Option<f64>,
        #[arg(long, default_value = "cli", value_parser = ["recorded", "cli", "mcp"])]
        source: String,
        #[arg(long)]
        metadata_json: Option<String>,
        #[arg(long)]
        dimensions_json: Option<String>,
        #[arg(long)]
        dimensions_file: Option<String>,
        #[arg(long)]
        links_json: Option<String>,
        #[arg(long)]
        links_file: Option<String>,
        #[arg(long)]
        json: bool,
    },
    /// List eval runs with filters
    List {
        #[arg(long)]
        spec: Option<String>,
        #[arg(long)]
        task: Option<String>,
        #[arg(long)]
        artifact_id: Option<String>,
        #[arg(long)]
        outcome: Option<String>,
        #[arg(long)]
        evaluator: Option<String>,
        #[arg(long)]
        target_kind: Option<String>,
        #[arg(long)]
        target_ref: Option<String>,
        #[arg(long)]
        source: Option<String>,
        #[arg(long)]
        created_after: Option<String>,
        #[arg(long)]
        created_before: Option<String>,
        #[arg(long)]
        limit: Option<i64>,
        #[arg(long)]
        offset: Option<i64>,
        #[arg(long)]
        json: bool,
    },
    /// Show a single eval run with scorecard detail
    Show {
        id: String,
        #[arg(long)]
        json: bool,
    },
    /// Compare an eval run against an explicit or inferred baseline
    Compare {
        #[arg(long)]
        baseline_id: Option<String>,
        #[arg(long)]
        current_id: String,
        #[arg(long)]
        latest_baseline: bool,
        #[arg(long)]
        json: bool,
    },
}

#[derive(Subcommand)]
pub enum PolicyConfigCmd {
    /// Create or update a policy config by ID
    Set {
        id: String,
        #[arg(long, value_parser = ["project", "spec", "task"])]
        scope: String,
        /// Scope reference; omit for project scope
        #[arg(long)]
        scope_ref: Option<String>,
        #[arg(long)]
        agent: Option<String>,
        #[arg(long, default_value = "enforced", value_parser = ["advisory", "enforced"])]
        mode: String,
        #[arg(long)]
        disabled: bool,
        /// Inline policy rules JSON object
        #[arg(long)]
        rules_json: Option<String>,
        /// Path to a file containing the policy rules JSON object
        #[arg(long)]
        rules_file: Option<String>,
        #[arg(long)]
        rationale: Option<String>,
        #[arg(long, default_value = "human")]
        by: String,
    },
    /// List policy configs
    List {
        #[arg(long, value_parser = ["project", "spec", "task"])]
        scope: Option<String>,
        #[arg(long)]
        scope_ref: Option<String>,
        #[arg(long)]
        agent: Option<String>,
    },
    /// Show one policy config
    Show { id: String },
}

#[derive(Subcommand)]
pub enum PolicyEvidenceCmd {
    /// Create or update a submitted evidence bundle
    Submit {
        id: String,
        #[arg(long)]
        spec: String,
        #[arg(long)]
        task: Option<String>,
        #[arg(long)]
        summary: Option<String>,
        #[arg(long)]
        behavior_change: bool,
        /// Inline evidence metadata JSON object
        #[arg(long)]
        metadata_json: Option<String>,
        /// Repeat as --artifact <ARTIFACT_ID[:supporting|primary_output|test_evidence]>
        #[arg(long)]
        artifact: Vec<String>,
        /// Repeat as --validation <RUN_ID[:fast|primary|full|custom]>
        #[arg(long)]
        validation: Vec<String>,
        #[arg(long, default_value = "human")]
        by: String,
    },
    /// Show a submitted evidence bundle and linked refs
    Show { id: String },
    /// Record a validation run and attach it to an evidence bundle
    RecordValidation {
        /// Unique ID for this validation run
        id: String,
        /// Evidence bundle to attach to
        #[arg(long)]
        bundle: String,
        /// Validation alias: fast|primary|full|custom
        #[arg(long, default_value = "full")]
        alias: String,
        /// Shell command that was run
        #[arg(long)]
        command: Option<String>,
        /// Whether the run passed
        #[arg(long, default_value_t = true)]
        passed: bool,
        /// Optional exit code
        #[arg(long)]
        exit_code: Option<i64>,
        /// Optional output summary
        #[arg(long)]
        output: Option<String>,
    },
}

#[derive(Subcommand)]
pub enum PolicyApprovalCmd {
    /// List approvals, pending first
    List {
        #[arg(long, value_parser = ["task", "spec", "operation"])]
        entity_kind: Option<String>,
        #[arg(long)]
        entity_id: Option<String>,
        #[arg(long)]
        operation: Option<String>,
        #[arg(long, value_parser = ["pending", "approved", "rejected", "cancelled", "expired"])]
        status: Option<String>,
    },
    /// Approve a pending approval request
    Approve {
        id: String,
        #[arg(long, default_value = "human")]
        by: String,
        #[arg(long)]
        reason: Option<String>,
    },
    /// Reject a pending approval request
    Reject {
        id: String,
        #[arg(long, default_value = "human")]
        by: String,
        #[arg(long)]
        reason: Option<String>,
    },
}

/// MCP server commands
#[derive(Subcommand)]
pub enum McpCmd {
    /// Start MCP stdio server
    Serve,
    /// Write MCP config for the target host
    Setup {
        #[arg(long)]
        global: bool,
        /// Target host: opencode (default), copilot, or vscode
        #[arg(long)]
        host: Option<String>,
    },
}

// ─── Skill Subcommands ────────────────────────────────────────────────────────

#[derive(Subcommand)]
pub enum SkillCmd {
    #[command(
        about = "Install bundled agents to the host agents directory",
        long_about = "Install bundled agents to the host agents directory.\n\nFor OpenCode (default): ~/.config/opencode/agents/\nFor GitHub Copilot CLI: ~/.copilot/agents/ (with .agent.md extension)\nFor VS Code: no per-agent files — skipped with an informative message.\nFor Pi / pi-subagents: ~/.pi/agent/agents/\n\nGenerated custom skills are not installed by this command; they remain separate `SKILL.md` files under ~/.agents/skills/<slug>/SKILL.md."
    )]
    Install {
        #[arg(long)]
        all: bool,
        /// Target host: opencode (default), copilot, vscode, or pi
        #[arg(long)]
        host: Option<String>,
    },
    /// List installed bundled agents
    List {
        /// Target host: opencode (default), copilot, vscode, or pi
        #[arg(long)]
        host: Option<String>,
    },
}

// ─── Memory Subcommands ───────────────────────────────────────────────────────

#[derive(Subcommand)]
pub enum MemoryCmd {
    /// List memory entries for an agent
    List {
        #[arg(long)]
        agent: String,
        #[arg(long)]
        spec: Option<String>,
        #[arg(long, name = "type")]
        mem_type: Option<String>,
        #[arg(long, default_value = "100")]
        limit: Option<i64>,
        /// Output as JSON
        #[arg(long)]
        json: bool,
    },
    /// Show a single memory entry
    Show {
        agent: String,
        key: String,
        #[arg(long)]
        spec: Option<String>,
        /// Output as JSON
        #[arg(long)]
        json: bool,
    },
    /// Full-text search across memory entries
    Search {
        query: String,
        #[arg(long)]
        agent: String,
        #[arg(long)]
        spec: Option<String>,
        #[arg(long, name = "type")]
        mem_type: Option<String>,
        /// Output as JSON
        #[arg(long)]
        json: bool,
    },
    /// Set (create or update) a memory entry
    Set {
        /// Agent name
        #[arg(long)]
        agent: String,
        /// Memory key
        #[arg(long)]
        key: String,
        /// Value — raw JSON object/array or plain string
        #[arg(long)]
        value: String,
        /// Scope to a spec ID
        #[arg(long)]
        spec: Option<String>,
        /// Memory type (decision, architecture, bugfix, pattern, config, discovery, learning)
        #[arg(long, name = "type")]
        mem_type: Option<String>,
        /// TTL in seconds
        #[arg(long)]
        ttl: Option<i64>,
        /// Related memory keys as JSON array (e.g. '["agent/key"]')
        #[arg(long)]
        related_to: Option<String>,
        /// Output as JSON
        #[arg(long)]
        json: bool,
    },
    /// Garbage-collect soft-deleted and expired entries
    Gc {
        #[arg(long)]
        dry_run: bool,
    },
}

// ─── Workspace Subcommands ────────────────────────────────────────────────────

#[derive(Subcommand)]
pub enum WorkspaceCmd {
    /// Show status summary for one or more spex project paths
    Status {
        /// Paths to spex project roots (each must contain .spex/state.db)
        #[arg(required = true)]
        paths: Vec<String>,
    },
    /// (placeholder — workspace commands are read-only in v1)
    #[command(hide = true)]
    Other,
}

// ─── Session Subcommands ──────────────────────────────────────────────────────

#[derive(Subcommand)]
pub enum SessionCmd {
    /// Start a new session
    Start {
        /// Agent identifier
        #[arg(long)]
        agent: String,
        /// Scope to a spec ID
        #[arg(long)]
        spec: Option<String>,
        /// Scope to a task ID
        #[arg(long)]
        task: Option<String>,
        /// Host/environment identifier
        #[arg(long)]
        host: Option<String>,
        /// Free-form notes
        #[arg(long)]
        notes: Option<String>,
    },
    /// End an active session
    End {
        /// Session ID to end
        session_id: String,
    },
    /// List sessions
    List {
        /// Filter by spec ID
        #[arg(long)]
        spec: Option<String>,
        /// Filter by agent
        #[arg(long)]
        agent: Option<String>,
        /// Show only active (not yet ended) sessions
        #[arg(long)]
        active: bool,
    },
    /// Save a checkpoint for a session
    Checkpoint {
        /// Session ID
        session_id: String,
        /// Agent identifier
        #[arg(long)]
        agent: String,
        /// Scope to a spec ID
        #[arg(long)]
        spec: Option<String>,
        /// Scope to a task ID
        #[arg(long)]
        task: Option<String>,
        /// Human-readable label for this checkpoint
        #[arg(long)]
        label: Option<String>,
        /// Checkpoint data as a JSON string
        #[arg(long)]
        data: String,
    },
    /// Restore a session checkpoint (latest if --checkpoint not given)
    Restore {
        /// Session ID
        session_id: String,
        /// Checkpoint ID to restore (defaults to latest)
        #[arg(long)]
        checkpoint: Option<String>,
    },
    /// List all checkpoints for a session
    Checkpoints {
        /// Session ID
        session_id: String,
    },
}

// ─── Readiness Subcommands ────────────────────────────────────────────────────

#[derive(Subcommand)]
pub enum ReadinessCmd {
    /// Show readiness report for a single spec
    Spec {
        /// Spec ID
        spec_id: String,
    },
    /// Show operator-level readiness across all specs
    Operator,
    /// Transition a spec to a new workflow phase
    Phase {
        /// Spec ID
        spec_id: String,
        /// Phase: planning | in_progress | review | done
        phase: String,
        /// Who is making the transition
        #[arg(long)]
        by: Option<String>,
        /// Optional notes
        #[arg(long)]
        notes: Option<String>,
    },
    /// Enter review phase and seed default requirements
    EnterReview {
        /// Spec ID
        spec_id: String,
        /// Agent entering review
        #[arg(long)]
        by: Option<String>,
    },
    /// Approve review for a spec
    Approve {
        /// Spec ID
        spec_id: String,
        /// Approver identity
        #[arg(long, required = true)]
        by: String,
    },
    /// Add a review requirement to a spec
    AddRequirement {
        /// Spec ID
        spec_id: String,
        /// Requirement kind: test_pass | lint_pass | review_approved | custom
        #[arg(long, required = true)]
        kind: String,
        /// Human-readable description
        #[arg(long, required = true)]
        description: String,
    },
    /// Satisfy a review requirement
    Satisfy {
        /// Requirement ID
        req_id: String,
        /// Who satisfied the requirement
        #[arg(long, required = true)]
        by: String,
    },
}

// ─── Entry Point ──────────────────────────────────────────────────────────────

#[tokio::main]
async fn main() -> Result<()> {
    let cli = Cli::parse();

    match cli.command {
        Commands::Init => {
            let cwd = std::env::current_dir()?;
            scaffold::init_project(&cwd).await?;
        }

        Commands::Setup { host } => {
            cmd_setup(host.as_deref()).await?;
        }

        Commands::New { name, yes } => {
            let cwd = std::env::current_dir()?;
            let project_dir = cwd.join(&name);
            scaffold::scaffold_project(&name, &project_dir, yes).await?;
        }

        Commands::Spec { cmd } => {
            let pool = open_project_db().await?;
            match cmd {
                SpecCmd::Add {
                    id,
                    title,
                    priority,
                } => cmd_spec_add(&pool, &id, &title, &priority).await?,
                SpecCmd::Approve { id } => cmd_spec_approve(&pool, &id).await?,
                SpecCmd::Start { id } => cmd_spec_start(&pool, &id).await?,
                SpecCmd::Done { id } => cmd_spec_done(&pool, &id).await?,
                SpecCmd::List {
                    json,
                    limit,
                    offset,
                } => cmd_spec_list(&pool, json, limit, offset).await?,
                SpecCmd::Show { id } => cmd_spec_show(&pool, &id).await?,
            }
        }

        Commands::Plan { cmd } => {
            let pool = open_project_db().await?;
            match cmd {
                PlanCmd::Build { spec_id } => cmd_plan_build(&pool, &spec_id).await?,
                PlanCmd::Show { spec_id } => cmd_plan_show(&pool, &spec_id).await?,
            }
        }

        Commands::Task { cmd } => {
            let pool = open_project_db().await?;
            match cmd {
                TaskCmd::Add {
                    spec_id,
                    task_id,
                    title,
                    agent,
                    inputs,
                    output_artifact,
                } => {
                    cmd_task_add(
                        &pool,
                        &spec_id,
                        &task_id,
                        &title,
                        &agent,
                        &inputs,
                        output_artifact,
                    )
                    .await?
                }
                TaskCmd::Start { id, updated_by } => {
                    cmd_task_start(&pool, &id, &updated_by).await?
                }
                TaskCmd::Done { id, updated_by } => cmd_task_done(&pool, &id, &updated_by).await?,
                TaskCmd::Fail { id } => cmd_task_fail(&pool, &id).await?,
                TaskCmd::List {
                    spec_id,
                    json,
                    limit,
                    offset,
                } => cmd_task_list(&pool, spec_id.as_deref(), json, limit, offset).await?,
            }
        }

        Commands::Policy { cmd } => {
            let pool = open_project_db().await?;
            match cmd {
                PolicyCmd::Config { cmd } => match cmd {
                    PolicyConfigCmd::Set {
                        id,
                        scope,
                        scope_ref,
                        agent,
                        mode,
                        disabled,
                        rules_json,
                        rules_file,
                        rationale,
                        by,
                    } => {
                        cmd_policy_config_set(
                            &pool,
                            &id,
                            &scope,
                            scope_ref.as_deref(),
                            agent.as_deref(),
                            &mode,
                            !disabled,
                            rules_json.as_deref(),
                            rules_file.as_deref(),
                            rationale.as_deref(),
                            &by,
                        )
                        .await?
                    }
                    PolicyConfigCmd::List {
                        scope,
                        scope_ref,
                        agent,
                    } => {
                        cmd_policy_config_list(
                            &pool,
                            scope.as_deref(),
                            scope_ref.as_deref(),
                            agent.as_deref(),
                        )
                        .await?
                    }
                    PolicyConfigCmd::Show { id } => cmd_policy_config_show(&pool, &id).await?,
                },
                PolicyCmd::Evidence { cmd } => match cmd {
                    PolicyEvidenceCmd::Submit {
                        id,
                        spec,
                        task,
                        summary,
                        behavior_change,
                        metadata_json,
                        artifact,
                        validation,
                        by,
                    } => {
                        cmd_policy_evidence_submit(
                            &pool,
                            &id,
                            &spec,
                            task.as_deref(),
                            summary.as_deref(),
                            behavior_change,
                            metadata_json.as_deref(),
                            &artifact,
                            &validation,
                            &by,
                        )
                        .await?
                    }
                    PolicyEvidenceCmd::Show { id } => cmd_policy_evidence_show(&pool, &id).await?,
                    PolicyEvidenceCmd::RecordValidation {
                        id,
                        bundle,
                        alias,
                        command,
                        passed,
                        exit_code,
                        output,
                    } => {
                        cmd_policy_evidence_record_validation(
                            &pool,
                            &id,
                            &bundle,
                            &alias,
                            command.as_deref(),
                            passed,
                            exit_code,
                            output.as_deref(),
                        )
                        .await?
                    }
                },
                PolicyCmd::Approval { cmd } => match cmd {
                    PolicyApprovalCmd::List {
                        entity_kind,
                        entity_id,
                        operation,
                        status,
                    } => {
                        cmd_policy_approval_list(
                            &pool,
                            entity_kind.as_deref(),
                            entity_id.as_deref(),
                            operation.as_deref(),
                            status.as_deref(),
                        )
                        .await?
                    }
                    PolicyApprovalCmd::Approve { id, by, reason } => {
                        cmd_policy_approval_approve(&pool, &id, &by, reason.as_deref()).await?
                    }
                    PolicyApprovalCmd::Reject { id, by, reason } => {
                        cmd_policy_approval_reject(&pool, &id, &by, reason.as_deref()).await?
                    }
                },
            }
        }

        Commands::Eval { cmd } => {
            let pool = open_project_db().await?;
            match cmd {
                EvalCmd::Create {
                    id,
                    evaluator,
                    target_kind,
                    target_ref,
                    spec,
                    task,
                    artifact_id,
                    summary,
                    rationale,
                    outcome,
                    overall_score,
                    source,
                    metadata_json,
                    dimensions_json,
                    dimensions_file,
                    links_json,
                    links_file,
                    json,
                } => {
                    cmd_eval_create(
                        &pool,
                        EvalCreateOptions {
                            id: id.as_deref(),
                            evaluator: &evaluator,
                            target_kind: &target_kind,
                            target_ref: &target_ref,
                            spec: spec.as_deref(),
                            task: task.as_deref(),
                            artifact_id: artifact_id.as_deref(),
                            summary: summary.as_deref(),
                            rationale: rationale.as_deref(),
                            outcome: &outcome,
                            overall_score,
                            source: &source,
                            metadata_json: metadata_json.as_deref(),
                            dimensions_json: dimensions_json.as_deref(),
                            dimensions_file: dimensions_file.as_deref(),
                            links_json: links_json.as_deref(),
                            links_file: links_file.as_deref(),
                            json,
                        },
                    )
                    .await?
                }
                EvalCmd::List {
                    spec,
                    task,
                    artifact_id,
                    outcome,
                    evaluator,
                    target_kind,
                    target_ref,
                    source,
                    created_after,
                    created_before,
                    limit,
                    offset,
                    json,
                } => {
                    cmd_eval_list(
                        &pool,
                        EvalListOptions {
                            spec: spec.as_deref(),
                            task: task.as_deref(),
                            artifact_id: artifact_id.as_deref(),
                            outcome: outcome.as_deref(),
                            evaluator: evaluator.as_deref(),
                            target_kind: target_kind.as_deref(),
                            target_ref: target_ref.as_deref(),
                            source: source.as_deref(),
                            created_after: created_after.as_deref(),
                            created_before: created_before.as_deref(),
                            limit,
                            offset,
                            json,
                        },
                    )
                    .await?
                }
                EvalCmd::Show { id, json } => cmd_eval_show(&pool, &id, json).await?,
                EvalCmd::Compare {
                    baseline_id,
                    current_id,
                    latest_baseline,
                    json,
                } => {
                    cmd_eval_compare(
                        &pool,
                        EvalCompareOptions {
                            baseline_id: baseline_id.as_deref(),
                            current_id: &current_id,
                            latest_baseline,
                            json,
                        },
                    )
                    .await?
                }
            }
        }

        Commands::Pulse { since, until } => {
            let pool = open_project_db().await?;
            cmd_pulse(&pool, since.as_deref(), until.as_deref()).await?;
        }

        Commands::Brief { json } => {
            let pool = open_project_db().await?;
            cmd_brief(&pool, json).await?;
        }

        Commands::Trace {
            spec,
            agent,
            task,
            full,
            limit,
            offset,
        } => {
            let pool = open_project_db().await?;
            cmd_trace(
                &pool,
                spec.as_deref(),
                agent.as_deref(),
                task.as_deref(),
                full,
                limit,
                offset,
            )
            .await?;
        }

        Commands::Mcp { cmd } => match cmd {
            McpCmd::Serve => {
                let pool = open_project_db().await?;
                cmd_mcp_serve(pool).await?;
            }
            McpCmd::Setup { global, host } => {
                cmd_mcp_setup(global, host.as_deref())?;
            }
        },

        Commands::Skill { cmd } => match cmd {
            SkillCmd::Install { all, host } => cmd_skill_install(all, host.as_deref()).await?,
            SkillCmd::List { host } => cmd_skill_list(host.as_deref())?,
        },

        Commands::Doctor { fix } => {
            cmd_doctor(fix).await?;
        }

        Commands::Completions { shell } => {
            generate(shell, &mut Cli::command(), "spex", &mut std::io::stdout());
        }

        Commands::Update { check } => {
            cmd_update(check).await?;
        }

        Commands::Memory { cmd } => {
            let pool = open_project_db().await?;
            match cmd {
                MemoryCmd::List {
                    agent,
                    spec,
                    mem_type,
                    limit,
                    json,
                } => {
                    cmd_memory_list(
                        &pool,
                        &agent,
                        spec.as_deref(),
                        mem_type.as_deref(),
                        limit,
                        json,
                    )
                    .await?
                }
                MemoryCmd::Show {
                    agent,
                    key,
                    spec,
                    json,
                } => cmd_memory_show(&pool, &agent, &key, spec.as_deref(), json).await?,
                MemoryCmd::Search {
                    query,
                    agent,
                    spec,
                    mem_type,
                    json,
                } => {
                    cmd_memory_search(
                        &pool,
                        &query,
                        &agent,
                        spec.as_deref(),
                        mem_type.as_deref(),
                        json,
                    )
                    .await?
                }
                MemoryCmd::Set {
                    agent,
                    key,
                    value,
                    spec,
                    mem_type,
                    ttl,
                    related_to,
                    json,
                } => {
                    cmd_memory_set(
                        &pool,
                        &agent,
                        &key,
                        &value,
                        MemorySetOpts {
                            spec: spec.as_deref(),
                            mem_type: mem_type.as_deref(),
                            ttl,
                            related_to: related_to.as_deref(),
                            json,
                        },
                    )
                    .await?
                }
                MemoryCmd::Gc { dry_run } => cmd_memory_gc(&pool, dry_run).await?,
            }
        }

        Commands::Workspace { cmd } => match cmd {
            WorkspaceCmd::Status { paths } => cmd_workspace_status(&paths).await?,
            WorkspaceCmd::Other => {
                eprintln!("workspace commands are read-only in v1");
                std::process::exit(1);
            }
        },

        Commands::Session { cmd } => {
            let pool = open_project_db().await?;
            match cmd {
                SessionCmd::Start {
                    agent,
                    spec,
                    task,
                    host,
                    notes,
                } => {
                    cmd_session_start(
                        &pool,
                        &agent,
                        spec.as_deref(),
                        task.as_deref(),
                        host.as_deref(),
                        notes.as_deref(),
                    )
                    .await?
                }
                SessionCmd::End { session_id } => cmd_session_end(&pool, &session_id).await?,
                SessionCmd::List {
                    spec,
                    agent,
                    active,
                } => cmd_session_list(&pool, spec.as_deref(), agent.as_deref(), active).await?,
                SessionCmd::Checkpoint {
                    session_id,
                    agent,
                    spec,
                    task,
                    label,
                    data,
                } => {
                    cmd_session_checkpoint(
                        &pool,
                        &session_id,
                        &agent,
                        spec.as_deref(),
                        task.as_deref(),
                        label.as_deref(),
                        &data,
                    )
                    .await?
                }
                SessionCmd::Restore {
                    session_id,
                    checkpoint,
                } => cmd_session_restore(&pool, &session_id, checkpoint.as_deref()).await?,
                SessionCmd::Checkpoints { session_id } => {
                    cmd_session_checkpoints(&pool, &session_id).await?
                }
            }
        }

        Commands::Readiness { cmd } => {
            let pool = open_project_db().await?;
            match cmd {
                ReadinessCmd::Spec { spec_id } => cmd_readiness_spec(&pool, &spec_id).await?,
                ReadinessCmd::Operator => cmd_readiness_operator(&pool).await?,
                ReadinessCmd::Phase {
                    spec_id,
                    phase,
                    by,
                    notes,
                } => {
                    cmd_readiness_phase(&pool, &spec_id, &phase, by.as_deref(), notes.as_deref())
                        .await?
                }
                ReadinessCmd::EnterReview { spec_id, by } => {
                    cmd_readiness_enter_review(&pool, &spec_id, by.as_deref()).await?
                }
                ReadinessCmd::Approve { spec_id, by } => {
                    cmd_readiness_approve(&pool, &spec_id, &by).await?
                }
                ReadinessCmd::AddRequirement {
                    spec_id,
                    kind,
                    description,
                } => cmd_readiness_add_requirement(&pool, &spec_id, &kind, &description).await?,
                ReadinessCmd::Satisfy { req_id, by } => {
                    cmd_readiness_satisfy_requirement(&pool, &req_id, &by).await?
                }
            }
        }
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use clap::CommandFactory;

    fn render_help(mut command: clap::Command) -> String {
        let mut output = Vec::new();
        command
            .write_long_help(&mut output)
            .expect("help output must render");
        String::from_utf8(output).expect("help output must be valid UTF-8")
    }

    fn render_subcommand_help(path: &[&str]) -> String {
        let mut command = Cli::command();

        for name in path {
            command = command
                .find_subcommand_mut(name)
                .unwrap_or_else(|| panic!("missing subcommand: {name}"))
                .clone();
        }

        render_help(command)
    }

    #[test]
    fn root_help_lists_skill_as_bundled_agent_management() {
        let help = render_help(Cli::command());

        assert!(
            help.contains("skill") && help.contains("Bundled agent management"),
            "root help must describe the skill command as bundled agent management"
        );
    }

    #[test]
    fn skill_help_distinguishes_bundled_agents_from_custom_skills() {
        let help = render_subcommand_help(&["skill"]);

        assert!(
            help.contains("Manage bundled agents installed under the host agents directory."),
            "skill help must describe host agents directory"
        );
        assert!(
            help.contains(
                "Custom skills remain separate `SKILL.md` files under ~/.agents/skills/<slug>/SKILL.md."
            ),
            "skill help must keep custom skills on the shared skills path"
        );
        assert!(
            !help.contains("bundled agents installed under ~/.config/opencode/skills/"),
            "skill help must not point bundled agents to the old skills directory"
        );
    }

    #[test]
    fn skill_install_help_uses_same_distinct_paths() {
        let help = render_subcommand_help(&["skill", "install"]);

        assert!(
            help.contains("Install bundled agents to the host agents directory"),
            "skill install help must describe host agents directory"
        );
        assert!(
            help.contains(
                "Generated custom skills are not installed by this command; they remain separate `SKILL.md` files under ~/.agents/skills/<slug>/SKILL.md."
            ),
            "skill install help must keep generated custom skills on the shared skills path"
        );
        assert!(
            !help.contains("Install bundled agents to ~/.config/opencode/skills/"),
            "skill install help must not point bundled agents to the old skills directory"
        );
    }

    #[test]
    fn policy_help_exposes_config_evidence_and_approval_groups() {
        let help = render_subcommand_help(&["policy"]);

        assert!(
            help.contains("config"),
            "policy help must list config subcommands"
        );
        assert!(
            help.contains("evidence"),
            "policy help must list evidence subcommands"
        );
        assert!(
            help.contains("approval"),
            "policy help must list approval subcommands"
        );
    }

    #[test]
    fn policy_evidence_submit_help_documents_attachment_syntax() {
        let help = render_subcommand_help(&["policy", "evidence", "submit"]);

        assert!(
            help.contains("ARTIFACT_ID[:supporting|primary_output|test_evidence]"),
            "policy evidence submit help must document artifact attachment syntax"
        );
        assert!(
            help.contains("RUN_ID[:fast|primary|full|custom]"),
            "policy evidence submit help must document validation attachment syntax"
        );
    }

    #[test]
    fn eval_help_exposes_create_list_and_show() {
        let help = render_subcommand_help(&["eval"]);

        assert!(help.contains("create"), "eval help must list create");
        assert!(help.contains("list"), "eval help must list list");
        assert!(help.contains("show"), "eval help must list show");
        assert!(help.contains("compare"), "eval help must list compare");
    }

    #[test]
    fn eval_create_help_documents_json_inputs() {
        let help = render_subcommand_help(&["eval", "create"]);

        assert!(help.contains("--dimensions-json"));
        assert!(help.contains("--dimensions-file"));
        assert!(help.contains("--links-json"));
        assert!(help.contains("--links-file"));
    }

    #[test]
    fn eval_compare_help_documents_baseline_options() {
        let help = render_subcommand_help(&["eval", "compare"]);

        assert!(help.contains("--baseline-id"));
        assert!(help.contains("--current-id"));
        assert!(help.contains("--latest-baseline"));
    }
}
