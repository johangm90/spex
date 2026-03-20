mod cli;
mod doctor;
mod mcp;
mod scaffold;
mod sdd;
mod skills_mgr;

use anyhow::Result;
use clap::{Parser, Subcommand};

use cli::{
    doctor::cmd_doctor,
    mcp_cmd::{cmd_mcp_serve, cmd_mcp_setup},
    memory_cmd::{cmd_memory_gc, cmd_memory_list, cmd_memory_search, cmd_memory_show},
    plan::{cmd_plan_build, cmd_plan_show},
    pulse::cmd_pulse,
    skill_cmd::{cmd_setup, cmd_skill_install, cmd_skill_list},
    spec::{
        cmd_spec_add, cmd_spec_approve, cmd_spec_done, cmd_spec_list, cmd_spec_show, cmd_spec_start,
    },
    task::{cmd_task_add, cmd_task_done, cmd_task_fail, cmd_task_list, cmd_task_start},
    trace::cmd_trace,
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

    /// One-time global setup: install agent skills and write MCP config
    Setup {
        /// Write MCP config to global ~/.config/opencode/config.json instead of ./opencode.json
        #[arg(long)]
        global: bool,
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

    /// Manage agent memory entries
    Memory {
        #[command(subcommand)]
        cmd: MemoryCmd,
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
    Start { id: String },
    /// Mark task as done
    Done { id: String },
    /// Mark task as failed
    Fail { id: String },
    /// List tasks
    List {
        spec_id: Option<String>,
        #[arg(long)]
        json: bool,
    },
}

// ─── MCP Subcommands ──────────────────────────────────────────────────────────

#[derive(Subcommand)]
pub enum McpCmd {
    /// Start MCP stdio server
    Serve,
    /// Write opencode.json MCP config
    Setup {
        #[arg(long)]
        global: bool,
    },
}

// ─── Skill Subcommands ────────────────────────────────────────────────────────

#[derive(Subcommand)]
pub enum SkillCmd {
    /// Install bundled skills to ~/.config/opencode/skills/
    Install {
        #[arg(long)]
        all: bool,
    },
    /// List installed skills
    List,
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
    },
    /// Show a single memory entry
    Show {
        agent: String,
        key: String,
        #[arg(long)]
        spec: Option<String>,
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
    },
    /// Garbage-collect soft-deleted and expired entries
    Gc {
        #[arg(long)]
        dry_run: bool,
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

        Commands::Setup { global } => {
            cmd_setup(global).await?;
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
                SpecCmd::List { json } => cmd_spec_list(&pool, json).await?,
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
                TaskCmd::Start { id } => cmd_task_start(&pool, &id).await?,
                TaskCmd::Done { id } => cmd_task_done(&pool, &id).await?,
                TaskCmd::Fail { id } => cmd_task_fail(&pool, &id).await?,
                TaskCmd::List { spec_id, json } => {
                    cmd_task_list(&pool, spec_id.as_deref(), json).await?
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
                let pool = open_project_db().await?;
                cmd_mcp_serve(pool).await?;
            }
            McpCmd::Setup { global } => {
                cmd_mcp_setup(global)?;
            }
        },

        Commands::Skill { cmd } => match cmd {
            SkillCmd::Install { all } => cmd_skill_install(all).await?,
            SkillCmd::List => cmd_skill_list()?,
        },

        Commands::Doctor { fix } => {
            cmd_doctor(fix).await?;
        }

        Commands::Memory { cmd } => {
            let pool = open_project_db().await?;
            match cmd {
                MemoryCmd::List {
                    agent,
                    spec,
                    mem_type,
                    limit,
                } => {
                    cmd_memory_list(&pool, &agent, spec.as_deref(), mem_type.as_deref(), limit)
                        .await?
                }
                MemoryCmd::Show { agent, key, spec } => {
                    cmd_memory_show(&pool, &agent, &key, spec.as_deref()).await?
                }
                MemoryCmd::Search {
                    query,
                    agent,
                    spec,
                    mem_type,
                } => {
                    cmd_memory_search(&pool, &query, &agent, spec.as_deref(), mem_type.as_deref())
                        .await?
                }
                MemoryCmd::Gc { dry_run } => cmd_memory_gc(&pool, dry_run).await?,
            }
        }
    }

    Ok(())
}
