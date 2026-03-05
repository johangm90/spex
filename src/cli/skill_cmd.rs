use anyhow::Result;
use colored::Colorize;

use crate::skills_mgr::{install_bundled_agents, install_bundled_skills, list_installed_skills};

pub async fn cmd_skill_install(all: bool) -> Result<()> {
    if !all {
        println!(
            "{}",
            "Use `spex skill install --all` to install all bundled skills.".dimmed()
        );
        return Ok(());
    }

    let config_dir =
        dirs::config_dir().ok_or_else(|| anyhow::anyhow!("Could not find config directory"))?;
    let skills_dir = config_dir.join("opencode").join("skills");
    let agents_dir = config_dir.join("opencode").join("agents");

    std::fs::create_dir_all(&skills_dir)?;
    std::fs::create_dir_all(&agents_dir)?;

    let skill_count = install_bundled_skills(&skills_dir)?;
    println!(
        "{} Installed {} skill files to {}",
        "✓".green(),
        skill_count,
        skills_dir.display()
    );

    let agent_count = install_bundled_agents(&agents_dir)?;
    println!(
        "{} Installed {} agent files to {}",
        "✓".green(),
        agent_count,
        agents_dir.display()
    );

    Ok(())
}

pub fn cmd_skill_list() -> Result<()> {
    let config_dir =
        dirs::config_dir().ok_or_else(|| anyhow::anyhow!("Could not find config directory"))?;
    let skills_dir = config_dir.join("opencode").join("skills");

    if !skills_dir.exists() {
        println!(
            "{} No skills installed. Run `spex skill install --all`.",
            "ℹ".blue()
        );
        return Ok(());
    }

    let skills = list_installed_skills(&skills_dir)?;
    if skills.is_empty() {
        println!(
            "{} No spex skills found in {}.",
            "ℹ".blue(),
            skills_dir.display()
        );
    } else {
        println!("{}", "Installed skills:".bold());
        for skill in &skills {
            println!("  {} {}", "•".cyan(), skill);
        }
    }
    Ok(())
}

/// One-time global setup: install skills + agents, then write MCP config.
pub async fn cmd_setup(global: bool) -> Result<()> {
    println!("{}", "Running one-time spex setup…".bold());
    println!();

    // Step 1: Install bundled skills and agents
    let config_dir =
        dirs::config_dir().ok_or_else(|| anyhow::anyhow!("Could not find config directory"))?;
    let skills_dir = config_dir.join("opencode").join("skills");
    let agents_dir = config_dir.join("opencode").join("agents");

    std::fs::create_dir_all(&skills_dir)?;
    std::fs::create_dir_all(&agents_dir)?;

    let skill_count = install_bundled_skills(&skills_dir)?;
    println!(
        "  {} Installed {} skill files → {}",
        "✓".green(),
        skill_count,
        skills_dir.display()
    );

    let agent_count = install_bundled_agents(&agents_dir)?;
    println!(
        "  {} Installed {} agent files → {}",
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
    println!("You can now open OpenCode in any project where you have run {} or {}.",
        "spex init".cyan(), "spex new".cyan());

    Ok(())
}
