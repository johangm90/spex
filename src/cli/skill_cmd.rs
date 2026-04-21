use anyhow::{anyhow, Result};
use colored::Colorize;

use crate::host::{Host, HostProfile};
use crate::skills_mgr::install_bundled_agents;

/// Resolve a host name string to a `HostProfile`, defaulting to OpenCode.
fn resolve_host_profile(host: Option<&str>) -> Result<HostProfile> {
    let h = match host {
        None => Host::OpenCode,
        Some(s) => Host::from_str(s)
            .ok_or_else(|| anyhow!("Unknown host '{}'. Valid values: opencode, copilot", s))?,
    };
    HostProfile::for_host(h).ok_or_else(|| anyhow!("Could not determine home directory"))
}

pub async fn cmd_skill_install(all: bool, host: Option<&str>) -> Result<()> {
    if !all {
        println!(
            "{}",
            "Use `spex skill install --all` to install all bundled agents.".dimmed()
        );
        return Ok(());
    }

    let profile = resolve_host_profile(host)?;
    std::fs::create_dir_all(&profile.agents_dir)?;

    let agent_count = install_bundled_agents(&profile.agents_dir, profile.agent_extension)?;
    println!(
        "{} Installed {} agent file(s) to {}",
        "✓".green(),
        agent_count,
        profile.agents_dir.display()
    );

    Ok(())
}

pub fn cmd_skill_list(host: Option<&str>) -> Result<()> {
    let profile = resolve_host_profile(host)?;

    if !profile.agents_dir.exists() {
        println!("{} No agents installed. Run `spex setup`.", "ℹ".blue());
        return Ok(());
    }

    let extension = format!(".{}", profile.agent_extension);
    let mut agents: Vec<String> = std::fs::read_dir(&profile.agents_dir)?
        .filter_map(|e| e.ok())
        .filter(|e| {
            e.path()
                .file_name()
                .and_then(|s| s.to_str())
                .map(|name| name.ends_with(&extension))
                .unwrap_or(false)
        })
        .map(|e| e.file_name().to_string_lossy().to_string())
        .collect();
    agents.sort();

    if agents.is_empty() {
        println!(
            "{} No agent files found in {}.",
            "ℹ".blue(),
            profile.agents_dir.display()
        );
    } else {
        println!("{}", "Installed agents:".bold());
        for agent in &agents {
            println!("  {} {}", "•".cyan(), agent);
        }
    }
    Ok(())
}

/// One-time global setup: install agents, then write MCP config.
pub async fn cmd_setup(global: bool, host: Option<&str>) -> Result<()> {
    println!("{}", "Running one-time spex setup…".bold());
    println!();

    let profile = resolve_host_profile(host)?;

    // Step 1: Install bundled agents
    std::fs::create_dir_all(&profile.agents_dir)?;
    let agent_count = install_bundled_agents(&profile.agents_dir, profile.agent_extension)?;
    println!(
        "  {} Installed {} agent file(s) → {}",
        "✓".green(),
        agent_count,
        profile.agents_dir.display()
    );

    // Step 2: Write MCP config
    println!();
    crate::cli::mcp_cmd::cmd_mcp_setup(global, host)?;

    println!();
    println!("{} Setup complete!", "✓".green().bold());
    println!();

    let init_hint = match profile.host {
        crate::host::Host::OpenCode => format!(
            "You can now open OpenCode in any project where you have run {} or {}.",
            "spex init".cyan(),
            "spex new".cyan()
        ),
        crate::host::Host::Copilot => format!(
            "You can now use GitHub Copilot CLI in any project where you have run {} or {}.",
            "spex init".cyan(),
            "spex new".cyan()
        ),
    };
    println!("{}", init_hint);

    Ok(())
}
