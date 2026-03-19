use anyhow::Result;
use colored::Colorize;

use crate::skills_mgr::install_bundled_agents;

pub async fn cmd_skill_install(all: bool) -> Result<()> {
    if !all {
        println!(
            "{}",
            "Use `spex skill install --all` to install all bundled agents.".dimmed()
        );
        return Ok(());
    }

    let opencode_dir = crate::cli::util::opencode_config_dir()
        .ok_or_else(|| anyhow::anyhow!("Could not find home directory"))?;
    let agents_dir = opencode_dir.join("agents");

    std::fs::create_dir_all(&agents_dir)?;

    let agent_count = install_bundled_agents(&agents_dir)?;
    println!(
        "{} Installed {} agent file(s) to {}",
        "✓".green(),
        agent_count,
        agents_dir.display()
    );

    Ok(())
}

pub fn cmd_skill_list() -> Result<()> {
    let opencode_dir = crate::cli::util::opencode_config_dir()
        .ok_or_else(|| anyhow::anyhow!("Could not find home directory"))?;
    let agents_dir = opencode_dir.join("agents");

    if !agents_dir.exists() {
        println!(
            "{} No agents installed. Run `spex setup`.",
            "ℹ".blue()
        );
        return Ok(());
    }

    let mut agents: Vec<String> = std::fs::read_dir(&agents_dir)?
        .filter_map(|e| e.ok())
        .filter(|e| e.path().extension().and_then(|s| s.to_str()) == Some("md"))
        .map(|e| e.file_name().to_string_lossy().to_string())
        .collect();
    agents.sort();

    if agents.is_empty() {
        println!(
            "{} No agent .md files found in {}.",
            "ℹ".blue(),
            agents_dir.display()
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
pub async fn cmd_setup(global: bool) -> Result<()> {
    println!("{}", "Running one-time spex setup…".bold());
    println!();

    // Step 1: Install bundled agents
    let opencode_dir = crate::cli::util::opencode_config_dir()
        .ok_or_else(|| anyhow::anyhow!("Could not find home directory"))?;
    let agents_dir = opencode_dir.join("agents");

    std::fs::create_dir_all(&agents_dir)?;

    let agent_count = install_bundled_agents(&agents_dir)?;
    println!(
        "  {} Installed {} agent file(s) → {}",
        "✓".green(),
        agent_count,
        agents_dir.display()
    );

    // Step 2: Write MCP config
    println!();
    crate::cli::mcp_cmd::cmd_mcp_setup(global)?;

    println!();
    println!("{} Setup complete!", "✓".green().bold());
    println!();
    println!(
        "You can now open OpenCode in any project where you have run {} or {}.",
        "spex init".cyan(),
        "spex new".cyan()
    );

    Ok(())
}
