use anyhow::{Context, Result};
use colored::Colorize;
use serde_json::json;
use sqlx::SqlitePool;
use std::path::PathBuf;

use crate::mcp::server::run_mcp_server;
use crate::tool_target::ToolTarget;
use std::sync::Arc;

/// Resolve the project directory from `SPEX_PROJECT_DIR` env var, or fall back
/// to the current working directory.  Called by `main.rs` (T04-07) so that the
/// DB walk-up happens in the right directory.
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

/// Start the MCP stdio server.
///
/// If `SPEX_PROJECT_DIR` is set the working directory is changed before the
/// server loop starts so that any relative paths resolve correctly.
///
/// NOTE: the `pool` is already opened by `main.rs` at this point.
/// TODO(T04-07): main.rs needs to pass SPEX_PROJECT_DIR through before opening
/// the DB so that `open_project_db()` walks up from the right directory.
pub async fn cmd_mcp_serve(pool: SqlitePool) -> Result<()> {
    if let Ok(project_dir) = std::env::var("SPEX_PROJECT_DIR") {
        // Change working directory so that open_project_db() walk-up finds the right .spex/state.db
        std::env::set_current_dir(&project_dir)
            .with_context(|| format!("SPEX_PROJECT_DIR={project_dir} is not a valid directory"))?;
    }

    let pool = Arc::new(pool);
    run_mcp_server(pool).await
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
