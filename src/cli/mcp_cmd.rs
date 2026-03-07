use anyhow::{Context, Result};
use colored::Colorize;
use serde_json::json;
use std::path::PathBuf;
use std::sync::Arc;

use crate::mcp::server::run_mcp_server;
use crate::sdd;
use crate::tool_target::ToolTarget;

/// Resolve the project directory from `SPEX_PROJECT_DIR` env var, or fall back
/// to the current working directory.  Returns the raw (non-canonicalized) path;
/// callers that need an absolute path should call `.canonicalize()` on the result.
pub fn resolve_project_dir() -> Result<PathBuf> {
    if let Ok(dir) = std::env::var("SPEX_PROJECT_DIR") {
        let path = PathBuf::from(&dir);
        if !path.is_dir() {
            anyhow::bail!("SPEX_PROJECT_DIR={dir} is not a valid directory");
        }
        Ok(path)
    } else {
        Ok(std::env::current_dir()?)
    }
}

/// Start the MCP stdio server against the global database.
///
/// The active project context is determined once at startup from
/// `SPEX_PROJECT_DIR` (or the current working directory) and threaded through
/// every tool call so that all reads/writes are scoped to that project.
///
/// The pool is opened here against `~/.local/share/spex/global-state.db`;
/// `main.rs` no longer needs to pre-open a pool for this command.
pub async fn cmd_mcp_serve() -> Result<()> {
    // Determine the active project directory for this server session.
    let project_dir = resolve_project_dir()?
        .canonicalize()
        .context("Could not canonicalize project dir")?
        .to_string_lossy()
        .to_string();

    // Inform the host (e.g. OpenCode) which project is being served.
    eprintln!("spex-state: serving project_dir={project_dir}");

    // Open the global DB (shared across all projects on this machine).
    let pool = Arc::new(sdd::db::open_global_db().await?);

    run_mcp_server(pool, project_dir).await
}

/// Write or merge the spex-state MCP entry into the appropriate config file.
///
/// - `tool` – which AI tool to configure
/// - `local` – if `true`, write to the per-project config file (OpenCode only);
///   if `false` (default), write to the global config file.
pub fn cmd_mcp_setup(tool: &ToolTarget, local: bool) -> Result<()> {
    if local {
        // CopilotCli has no concept of per-project config
        if *tool == ToolTarget::CopilotCli {
            println!(
                "{} Copilot CLI does not support per-project MCP config. \
                 Config is always global at ~/.copilot/mcp-config.json",
                "!".yellow()
            );
            return Ok(());
        }

        // OpenCode: write to ./opencode.json
        let path = tool
            .local_mcp_config_path()
            .ok_or_else(|| anyhow::anyhow!("Could not determine local MCP config path"))?;

        write_mcp_config(tool, &path)?;
    } else {
        // Global config
        let path = tool
            .global_mcp_config_path()
            .ok_or_else(|| anyhow::anyhow!("Could not find home directory"))?;

        // Ensure the parent directory exists
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }

        write_mcp_config(tool, &path)?;
    }

    Ok(())
}

/// Load `path`, merge the spex-state entry via `tool.merge_mcp_config`, write back,
/// and print the appropriate status line.
fn write_mcp_config(tool: &ToolTarget, path: &std::path::Path) -> Result<()> {
    let existing = if path.exists() {
        let raw = std::fs::read_to_string(path)?;
        serde_json::from_str(&raw).unwrap_or_else(|_| json!({}))
    } else {
        json!({})
    };

    let (config, changed) = tool.merge_mcp_config(existing);
    std::fs::write(path, serde_json::to_string_pretty(&config)?)?;

    if changed {
        println!("{} Updated {}", "✓".green(), path.display());
    } else {
        println!(
            "{} {} already contains MCP entry",
            "•".dimmed(),
            path.display()
        );
    }

    Ok(())
}
