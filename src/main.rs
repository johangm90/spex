#![recursion_limit = "512"]
mod cli;
mod doctor;
mod mcp;
mod scaffold;
mod sdd;
mod skills_mgr;
pub mod tool_target;
pub use tool_target::ToolTarget;

use anyhow::Result;
use clap::{Parser, Subcommand};

use cli::{
    doctor::cmd_doctor,
    mcp_cmd::{cmd_mcp_serve, cmd_mcp_setup},
    ops::{
        cmd_gap_add, cmd_gap_list, cmd_gap_show, cmd_gap_update, cmd_handoff_add, cmd_handoff_list,
        cmd_handoff_show, cmd_incident_add, cmd_incident_list, cmd_incident_show,
        cmd_incident_update, cmd_interrupt_add, cmd_interrupt_list, cmd_interrupt_show,
        cmd_interrupt_update, cmd_verify_add, cmd_verify_list, cmd_verify_show,
    },
    orchestrate::{
        cmd_orchestrate_claim, cmd_orchestrate_expire, cmd_orchestrate_heartbeat,
        cmd_orchestrate_lock, cmd_orchestrate_locks, cmd_orchestrate_next,
        cmd_orchestrate_plan_version, cmd_orchestrate_plan_versions, cmd_orchestrate_release,
        cmd_orchestrate_replan, cmd_orchestrate_replan_update, cmd_orchestrate_replans,
        cmd_orchestrate_task_metadata,
    },
    plan::{cmd_plan_build, cmd_plan_dag, cmd_plan_show},
    pulse::cmd_pulse,
    skill_cmd::{cmd_setup, cmd_skill_install, cmd_skill_list},
    spec::{
        cmd_spec_add, cmd_spec_approve, cmd_spec_done, cmd_spec_list, cmd_spec_show,
        cmd_spec_stabilize, cmd_spec_start,
    },
    task::{
        cmd_task_add, cmd_task_block, cmd_task_cancel, cmd_task_done, cmd_task_list,
        cmd_task_review, cmd_task_start, cmd_task_verify,
    },
    trace::cmd_trace,
};
use sdd::db::open_project_db;

fn parse_tool_target(s: &str) -> ToolTarget {
    match s.to_ascii_lowercase().as_str() {
        "copilot" | "copilot-cli" | "gh-copilot" => ToolTarget::CopilotCli,
        _ => ToolTarget::OpenCode,
    }
}

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

    /// One-time global setup: install agent skills and write MCP config
    Setup {
        /// Target AI tool: opencode (default) or copilot
        #[arg(long, default_value = "opencode")]
        tool: String,
        /// Write to local per-project config instead of global
        #[arg(long)]
        local: bool,
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

    /// Manage incidents
    Incident {
        #[command(subcommand)]
        cmd: IncidentCmd,
    },

    /// Manage context gaps
    Gap {
        #[command(subcommand)]
        cmd: GapCmd,
    },

    /// Manage verification runs
    Verify {
        #[command(subcommand)]
        cmd: VerifyCmd,
    },

    /// Manage interrupts
    Interrupt {
        #[command(subcommand)]
        cmd: InterruptCmd,
    },

    /// Manage handoff snapshots
    Handoff {
        #[command(subcommand)]
        cmd: HandoffCmd,
    },

    /// Scheduler/runtime orchestration helpers
    Orchestrate {
        #[command(subcommand)]
        cmd: OrchestrateCmd,
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

    /// Show domain event log
    Trace {
        #[arg(long)]
        spec: Option<String>,
        #[arg(long)]
        agent: Option<String>,
        #[arg(long)]
        limit: Option<i64>,
    },

    /// MCP server commands
    Mcp {
        #[command(subcommand)]
        cmd: McpCmd,
    },

    /// Skill management
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
    /// Move a spec into stabilizing
    Stabilize { id: String },
    /// Mark spec as done
    Done { id: String },
    /// List all specs
    List {
        #[arg(long)]
        json: bool,
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
    /// Show task dependency DAG for a spec
    Dag { spec_id: String },
}

// ─── Task Subcommands ─────────────────────────────────────────────────────────

#[allow(clippy::large_enum_variant)]
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
        depends_on: Vec<String>,
        #[arg(long)]
        conflicts_with: Vec<String>,
        #[arg(long)]
        lock_set: Vec<String>,
        #[arg(long, default_value_t = 100)]
        priority: i64,
        #[arg(long, default_value = "medium")]
        risk_level: String,
        #[arg(long, default_value = "coordinated_parallel")]
        execution_bucket: String,
        #[arg(long, default_value_t = 3)]
        estimate_points: i64,
        #[arg(long, default_value_t = 0)]
        unblock_value: i64,
        #[arg(long)]
        plan_version: Option<String>,
        #[arg(long)]
        output_artifact: Option<String>,
    },
    /// Start a task
    Start { id: String },
    /// Mark task as blocked
    Block { id: String },
    /// Mark task as in review
    Review { id: String },
    /// Mark task as verified
    Verify { id: String },
    /// Mark task as done
    Done { id: String },
    /// Cancel a task
    Cancel { id: String },
    /// List tasks
    List {
        spec_id: Option<String>,
        #[arg(long)]
        json: bool,
    },
}

#[derive(Subcommand)]
pub enum IncidentCmd {
    Add {
        id: String,
        spec: String,
        title: String,
        #[arg(long)]
        severity: String,
        #[arg(long)]
        source: String,
        #[arg(long)]
        task: Option<String>,
        #[arg(long, default_value_t = false)]
        blocking: bool,
        #[arg(long)]
        repro_steps: Option<String>,
    },
    Show {
        id: String,
    },
    Update {
        id: String,
        #[arg(long)]
        status: Option<String>,
        #[arg(long)]
        blocking: Option<bool>,
        #[arg(long)]
        root_cause: Option<String>,
        #[arg(long)]
        fix_strategy: Option<String>,
    },
    List {
        #[arg(long)]
        spec: Option<String>,
        #[arg(long)]
        status: Option<String>,
        #[arg(long)]
        json: bool,
    },
}

#[derive(Subcommand)]
pub enum GapCmd {
    Add {
        id: String,
        spec: String,
        question: String,
        #[arg(long)]
        kind: String,
        #[arg(long)]
        criticality: String,
        #[arg(long)]
        task: Option<String>,
        #[arg(long, default_value_t = false)]
        blocking: bool,
        #[arg(long)]
        assumption: Option<String>,
    },
    Show {
        id: String,
    },
    Update {
        id: String,
        #[arg(long)]
        status: Option<String>,
        #[arg(long)]
        blocking: Option<bool>,
        #[arg(long)]
        assumption: Option<String>,
        #[arg(long)]
        resolution: Option<String>,
    },
    List {
        #[arg(long)]
        spec: Option<String>,
        #[arg(long)]
        status: Option<String>,
        #[arg(long)]
        json: bool,
    },
}

#[derive(Subcommand)]
pub enum VerifyCmd {
    Add {
        id: String,
        spec: String,
        summary: String,
        #[arg(long)]
        kind: String,
        #[arg(long)]
        status: String,
        #[arg(long)]
        task: Option<String>,
        #[arg(long)]
        slice: Option<String>,
        #[arg(long)]
        command: Option<String>,
        #[arg(long)]
        evidence: Option<String>,
    },
    Show {
        id: String,
    },
    List {
        #[arg(long)]
        spec: Option<String>,
        #[arg(long)]
        task: Option<String>,
        #[arg(long)]
        status: Option<String>,
        #[arg(long)]
        json: bool,
    },
}

#[derive(Subcommand)]
pub enum InterruptCmd {
    Add {
        id: String,
        spec: String,
        #[arg(long)]
        reason_type: String,
        #[arg(long)]
        preempted_tasks: Vec<String>,
        #[arg(long)]
        resume_hint: Option<String>,
    },
    Show {
        id: String,
    },
    Update {
        id: String,
        #[arg(long)]
        status: Option<String>,
        #[arg(long)]
        resume_hint: Option<String>,
    },
    List {
        #[arg(long)]
        spec: Option<String>,
        #[arg(long)]
        status: Option<String>,
        #[arg(long)]
        json: bool,
    },
}

#[derive(Subcommand)]
pub enum HandoffCmd {
    Add {
        id: String,
        spec: String,
        #[arg(long)]
        interrupt: Option<String>,
        #[arg(long)]
        last_wave: Option<i64>,
        #[arg(long)]
        last_task: Option<String>,
        #[arg(long)]
        files_touched: Vec<String>,
        #[arg(long)]
        decisions: Vec<String>,
        #[arg(long)]
        open_risks: Vec<String>,
        #[arg(long)]
        next_steps: Vec<String>,
    },
    Show {
        id: String,
    },
    List {
        #[arg(long)]
        spec: Option<String>,
        #[arg(long)]
        json: bool,
    },
}

#[derive(Subcommand)]
pub enum OrchestrateCmd {
    /// Show next schedulable task for an agent
    Next {
        spec: String,
        #[arg(long)]
        agent: String,
    },
    /// Claim a task lease
    Claim {
        task: String,
        #[arg(long)]
        spec: Option<String>,
        #[arg(long)]
        agent: String,
        #[arg(long, default_value_t = false)]
        auto_lock: bool,
        #[arg(long, default_value_t = 1800)]
        ttl: i64,
    },
    /// Heartbeat an active task lease
    Heartbeat {
        task: String,
        #[arg(long)]
        spec: Option<String>,
        #[arg(long)]
        progress: Option<String>,
        #[arg(long, default_value_t = 1800)]
        ttl: i64,
    },
    /// Release a task lease
    Release {
        task: String,
        #[arg(long)]
        spec: Option<String>,
        #[arg(long)]
        final_status: Option<String>,
    },
    /// Expire stale leases
    Expire,
    /// Acquire locks for a task
    Lock {
        task: String,
        spec: String,
        #[arg(long)]
        module: Vec<String>,
        #[arg(long)]
        semantic: Vec<String>,
        #[arg(long)]
        file: Vec<String>,
    },
    /// List active locks
    Locks {
        #[arg(long)]
        spec: Option<String>,
        #[arg(long)]
        task: Option<String>,
    },
    /// Update stored task scheduling metadata
    TaskMetadata {
        task: String,
        #[arg(long)]
        depends_on: Vec<String>,
        #[arg(long)]
        conflicts_with: Vec<String>,
        #[arg(long)]
        lock_set: Vec<String>,
        #[arg(long)]
        priority: Option<i64>,
        #[arg(long)]
        risk_level: Option<String>,
        #[arg(long)]
        execution_bucket: Option<String>,
        #[arg(long)]
        estimate_points: Option<i64>,
        #[arg(long)]
        unblock_value: Option<i64>,
        #[arg(long)]
        plan_version: Option<String>,
    },

    /// Create a replan request
    Replan {
        id: String,
        spec: String,
        #[arg(long)]
        task: Option<String>,
        #[arg(long)]
        agent: String,
        #[arg(long)]
        reason: String,
        #[arg(long)]
        impact: Vec<String>,
        #[arg(long)]
        proposed_action: Option<String>,
    },
    /// List replan requests
    Replans {
        #[arg(long)]
        spec: Option<String>,
        #[arg(long)]
        status: Option<String>,
    },
    /// Update a replan request
    ReplanUpdate {
        id: String,
        #[arg(long)]
        status: String,
    },
    /// Register a plan version
    PlanVersion {
        id: String,
        spec: String,
        #[arg(long)]
        version: i64,
        #[arg(long)]
        reason: Option<String>,
        #[arg(long)]
        plan_json: String,
        #[arg(long, default_value_t = false)]
        supersede: bool,
    },
    /// List plan versions
    PlanVersions {
        #[arg(long)]
        spec: Option<String>,
    },
}

// ─── MCP Subcommands ──────────────────────────────────────────────────────────

#[derive(Subcommand)]
pub enum McpCmd {
    /// Start MCP stdio server
    Serve,
    /// Write opencode.json MCP config
    Setup {
        /// Target AI tool: opencode (default) or copilot
        #[arg(long, default_value = "opencode")]
        tool: String,
        /// Write to local per-project config instead of global
        #[arg(long)]
        local: bool,
    },
}

// ─── Skill Subcommands ────────────────────────────────────────────────────────

#[derive(Subcommand)]
pub enum SkillCmd {
    /// Install bundled skills to ~/.config/opencode/skills/
    Install {
        #[arg(long)]
        all: bool,
        /// Target AI tool: opencode (default) or copilot
        #[arg(long, default_value = "opencode")]
        tool: String,
    },
    /// List installed skills
    List,
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

        Commands::Setup { tool, local } => {
            cmd_setup(&parse_tool_target(&tool), local).await?;
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
                SpecCmd::Stabilize { id } => cmd_spec_stabilize(&pool, &id).await?,
                SpecCmd::Done { id } => cmd_spec_done(&pool, &id).await?,
                SpecCmd::List { json } => cmd_spec_list(&pool, json).await?,
                SpecCmd::Show { id } => cmd_spec_show(&pool, &id).await?,
            }
        }

        Commands::Plan { cmd } => {
            let pool = open_project_db().await?;
            match cmd {
                PlanCmd::Build { spec_id } => cmd_plan_build(&pool, &spec_id).await?,
                PlanCmd::Show { spec_id } => cmd_plan_show(&pool, &spec_id).await?,
                PlanCmd::Dag { spec_id } => cmd_plan_dag(&pool, &spec_id).await?,
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
                    depends_on,
                    conflicts_with,
                    lock_set,
                    priority,
                    risk_level,
                    execution_bucket,
                    estimate_points,
                    unblock_value,
                    plan_version,
                    output_artifact,
                } => {
                    cmd_task_add(
                        &pool,
                        &spec_id,
                        &task_id,
                        &title,
                        &agent,
                        &inputs,
                        &depends_on,
                        &conflicts_with,
                        &lock_set,
                        priority,
                        &risk_level,
                        &execution_bucket,
                        estimate_points,
                        unblock_value,
                        plan_version,
                        output_artifact,
                    )
                    .await?
                }
                TaskCmd::Start { id } => cmd_task_start(&pool, &id).await?,
                TaskCmd::Block { id } => cmd_task_block(&pool, &id).await?,
                TaskCmd::Review { id } => cmd_task_review(&pool, &id).await?,
                TaskCmd::Verify { id } => cmd_task_verify(&pool, &id).await?,
                TaskCmd::Done { id } => cmd_task_done(&pool, &id).await?,
                TaskCmd::Cancel { id } => cmd_task_cancel(&pool, &id).await?,
                TaskCmd::List { spec_id, json } => {
                    cmd_task_list(&pool, spec_id.as_deref(), json).await?
                }
            }
        }

        Commands::Incident { cmd } => {
            let pool = open_project_db().await?;
            match cmd {
                IncidentCmd::Add {
                    id,
                    spec,
                    title,
                    severity,
                    source,
                    task,
                    blocking,
                    repro_steps,
                } => {
                    cmd_incident_add(
                        &pool,
                        &id,
                        &spec,
                        task.as_deref(),
                        &title,
                        &severity,
                        &source,
                        blocking,
                        repro_steps.as_deref(),
                    )
                    .await?
                }
                IncidentCmd::Show { id } => cmd_incident_show(&pool, &id).await?,
                IncidentCmd::Update {
                    id,
                    status,
                    blocking,
                    root_cause,
                    fix_strategy,
                } => {
                    cmd_incident_update(
                        &pool,
                        &id,
                        status.as_deref(),
                        blocking,
                        root_cause.as_deref(),
                        fix_strategy.as_deref(),
                    )
                    .await?
                }
                IncidentCmd::List { spec, status, json } => {
                    cmd_incident_list(&pool, spec.as_deref(), status.as_deref(), json).await?
                }
            }
        }

        Commands::Gap { cmd } => {
            let pool = open_project_db().await?;
            match cmd {
                GapCmd::Add {
                    id,
                    spec,
                    question,
                    kind,
                    criticality,
                    task,
                    blocking,
                    assumption,
                } => {
                    cmd_gap_add(
                        &pool,
                        &id,
                        &spec,
                        task.as_deref(),
                        &kind,
                        &criticality,
                        blocking,
                        &question,
                        assumption.as_deref(),
                    )
                    .await?
                }
                GapCmd::Show { id } => cmd_gap_show(&pool, &id).await?,
                GapCmd::Update {
                    id,
                    status,
                    blocking,
                    assumption,
                    resolution,
                } => {
                    cmd_gap_update(
                        &pool,
                        &id,
                        status.as_deref(),
                        blocking,
                        assumption.as_deref(),
                        resolution.as_deref(),
                    )
                    .await?
                }
                GapCmd::List { spec, status, json } => {
                    cmd_gap_list(&pool, spec.as_deref(), status.as_deref(), json).await?
                }
            }
        }

        Commands::Verify { cmd } => {
            let pool = open_project_db().await?;
            match cmd {
                VerifyCmd::Add {
                    id,
                    spec,
                    summary,
                    kind,
                    status,
                    task,
                    slice,
                    command,
                    evidence,
                } => {
                    cmd_verify_add(
                        &pool,
                        &id,
                        &spec,
                        task.as_deref(),
                        slice.as_deref(),
                        &kind,
                        &status,
                        command.as_deref(),
                        &summary,
                        evidence.as_deref(),
                    )
                    .await?
                }
                VerifyCmd::Show { id } => cmd_verify_show(&pool, &id).await?,
                VerifyCmd::List {
                    spec,
                    task,
                    status,
                    json,
                } => {
                    cmd_verify_list(
                        &pool,
                        spec.as_deref(),
                        task.as_deref(),
                        status.as_deref(),
                        json,
                    )
                    .await?
                }
            }
        }

        Commands::Interrupt { cmd } => {
            let pool = open_project_db().await?;
            match cmd {
                InterruptCmd::Add {
                    id,
                    spec,
                    reason_type,
                    preempted_tasks,
                    resume_hint,
                } => {
                    cmd_interrupt_add(
                        &pool,
                        &id,
                        &spec,
                        &reason_type,
                        &preempted_tasks,
                        resume_hint.as_deref(),
                    )
                    .await?
                }
                InterruptCmd::Show { id } => cmd_interrupt_show(&pool, &id).await?,
                InterruptCmd::Update {
                    id,
                    status,
                    resume_hint,
                } => {
                    cmd_interrupt_update(&pool, &id, status.as_deref(), resume_hint.as_deref())
                        .await?
                }
                InterruptCmd::List { spec, status, json } => {
                    cmd_interrupt_list(&pool, spec.as_deref(), status.as_deref(), json).await?
                }
            }
        }

        Commands::Handoff { cmd } => {
            let pool = open_project_db().await?;
            match cmd {
                HandoffCmd::Add {
                    id,
                    spec,
                    interrupt,
                    last_wave,
                    last_task,
                    files_touched,
                    decisions,
                    open_risks,
                    next_steps,
                } => {
                    cmd_handoff_add(
                        &pool,
                        &id,
                        &spec,
                        interrupt.as_deref(),
                        last_wave,
                        last_task.as_deref(),
                        &files_touched,
                        &decisions,
                        &open_risks,
                        &next_steps,
                    )
                    .await?
                }
                HandoffCmd::Show { id } => cmd_handoff_show(&pool, &id).await?,
                HandoffCmd::List { spec, json } => {
                    cmd_handoff_list(&pool, spec.as_deref(), json).await?
                }
            }
        }

        Commands::Orchestrate { cmd } => {
            let pool = open_project_db().await?;
            match cmd {
                OrchestrateCmd::Next { spec, agent } => {
                    cmd_orchestrate_next(&pool, &spec, &agent).await?
                }
                OrchestrateCmd::Claim {
                    task,
                    spec,
                    agent,
                    auto_lock,
                    ttl,
                } => {
                    cmd_orchestrate_claim(&pool, spec.as_deref(), &task, &agent, auto_lock, ttl)
                        .await?
                }
                OrchestrateCmd::Heartbeat {
                    task,
                    spec,
                    progress,
                    ttl,
                } => {
                    cmd_orchestrate_heartbeat(
                        &pool,
                        spec.as_deref(),
                        &task,
                        ttl,
                        progress.as_deref(),
                    )
                    .await?
                }
                OrchestrateCmd::Release {
                    task,
                    spec,
                    final_status,
                } => {
                    cmd_orchestrate_release(&pool, spec.as_deref(), &task, final_status.as_deref())
                        .await?
                }
                OrchestrateCmd::Expire => cmd_orchestrate_expire(&pool).await?,
                OrchestrateCmd::Lock {
                    task,
                    spec,
                    module,
                    semantic,
                    file,
                } => cmd_orchestrate_lock(&pool, &task, &spec, &module, &semantic, &file).await?,
                OrchestrateCmd::Locks { spec, task } => {
                    cmd_orchestrate_locks(&pool, spec.as_deref(), task.as_deref()).await?
                }
                OrchestrateCmd::TaskMetadata {
                    task,
                    depends_on,
                    conflicts_with,
                    lock_set,
                    priority,
                    risk_level,
                    execution_bucket,
                    estimate_points,
                    unblock_value,
                    plan_version,
                } => {
                    cmd_orchestrate_task_metadata(
                        &pool,
                        &task,
                        &depends_on,
                        &conflicts_with,
                        &lock_set,
                        priority,
                        risk_level.as_deref(),
                        execution_bucket.as_deref(),
                        estimate_points,
                        unblock_value,
                        plan_version.as_deref(),
                    )
                    .await?
                }
                OrchestrateCmd::Replan {
                    id,
                    spec,
                    task,
                    agent,
                    reason,
                    impact,
                    proposed_action,
                } => {
                    cmd_orchestrate_replan(
                        &pool,
                        &id,
                        &spec,
                        task.as_deref(),
                        &agent,
                        &reason,
                        &impact,
                        proposed_action.as_deref(),
                    )
                    .await?
                }
                OrchestrateCmd::Replans { spec, status } => {
                    cmd_orchestrate_replans(&pool, spec.as_deref(), status.as_deref()).await?
                }
                OrchestrateCmd::ReplanUpdate { id, status } => {
                    cmd_orchestrate_replan_update(&pool, &id, &status).await?
                }
                OrchestrateCmd::PlanVersion {
                    id,
                    spec,
                    version,
                    reason,
                    plan_json,
                    supersede,
                } => {
                    cmd_orchestrate_plan_version(
                        &pool,
                        &id,
                        &spec,
                        version,
                        reason.as_deref(),
                        &plan_json,
                        supersede,
                    )
                    .await?
                }
                OrchestrateCmd::PlanVersions { spec } => {
                    cmd_orchestrate_plan_versions(&pool, spec.as_deref()).await?
                }
            }
        }

        Commands::Pulse { since, until } => {
            let pool = open_project_db().await?;
            cmd_pulse(&pool, since.as_deref(), until.as_deref()).await?;
        }

        Commands::Trace { spec, agent, limit } => {
            let pool = open_project_db().await?;
            cmd_trace(&pool, spec.as_deref(), agent.as_deref(), limit).await?;
        }

        Commands::Mcp { cmd } => match cmd {
            McpCmd::Serve => {
                use cli::mcp_cmd::resolve_project_dir;
                let project_dir = resolve_project_dir()?;
                std::env::set_current_dir(&project_dir)?;
                let pool = open_project_db().await?;
                cmd_mcp_serve(pool).await?;
            }
            McpCmd::Setup { tool, local } => {
                cmd_mcp_setup(&parse_tool_target(&tool), local)?;
            }
        },

        Commands::Skill { cmd } => match cmd {
            SkillCmd::Install { all, tool } => {
                cmd_skill_install(all, &parse_tool_target(&tool)).await?
            }
            SkillCmd::List => cmd_skill_list()?,
        },

        Commands::Doctor { fix } => {
            cmd_doctor(fix).await?;
        }
    }

    Ok(())
}
