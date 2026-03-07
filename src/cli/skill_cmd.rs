use anyhow::Result;
use colored::Colorize;

use crate::skills_mgr::{install_bundled_agents, install_bundled_skills, list_installed_skills};
use crate::tool_target::ToolTarget;

pub async fn cmd_skill_install(all: bool, tool: &ToolTarget) -> Result<()> {
    if !all {
        println!(
            "{}",
            "Use `spex skill install --all` to install all bundled skills.".dimmed()
        );
        return Ok(());
    }

    let skills_dir = tool
        .skills_dir()
        .ok_or_else(|| anyhow::anyhow!("Could not determine skills directory"))?;
    let agents_dir = tool
        .agents_dir()
        .ok_or_else(|| anyhow::anyhow!("Could not determine agents directory"))?;

    std::fs::create_dir_all(&skills_dir)?;
    std::fs::create_dir_all(&agents_dir)?;

    let tool_name = tool.display_name();

    let skill_count = install_bundled_skills(&skills_dir)?;
    println!(
        "{} Installed {} skill files to {} [{}]",
        "✓".green(),
        skill_count,
        skills_dir.display(),
        tool_name,
    );

    let agent_count = install_bundled_agents(&agents_dir, tool)?;
    println!(
        "{} Installed {} agent files to {} [{}]",
        "✓".green(),
        agent_count,
        agents_dir.display(),
        tool_name,
    );

    Ok(())
}

pub fn cmd_skill_list() -> Result<()> {
    let tools = ToolTarget::detect_installed();

    if tools.is_empty() {
        // Fall back to OpenCode skills dir regardless
        let opencode_dir = crate::cli::util::opencode_config_dir()
            .ok_or_else(|| anyhow::anyhow!("Could not find home directory"))?;
        let skills_dir = opencode_dir.join("skills");

        if !skills_dir.exists() {
            println!(
                "{} No skills installed. Run `spex skill install --all`.",
                "ℹ".blue()
            );
            return Ok(());
        }

        let skills = list_installed_skills(&skills_dir)?;
        println!("{} [OpenCode]:", "Installed skills".bold());
        if skills.is_empty() {
            println!(
                "  {} No spex skills found in {}.",
                "ℹ".blue(),
                skills_dir.display()
            );
        } else {
            for skill in &skills {
                println!("  {} {}", "•".cyan(), skill);
            }
        }
        return Ok(());
    }

    for tool in &tools {
        let skills_dir = tool.skills_dir().unwrap_or_default();
        println!("{} [{}]:", "Installed skills".bold(), tool.display_name());

        if !skills_dir.exists() {
            println!(
                "  {} No skills installed. Run `spex skill install --all`.",
                "ℹ".blue()
            );
            continue;
        }

        let skills = list_installed_skills(&skills_dir)?;
        if skills.is_empty() {
            println!(
                "  {} No spex skills found in {}.",
                "ℹ".blue(),
                skills_dir.display()
            );
        } else {
            for skill in &skills {
                println!("  {} {}", "•".cyan(), skill);
            }
        }
    }

    Ok(())
}

/// One-time setup: install skills + agents, then write MCP config.
pub async fn cmd_setup(tool: &ToolTarget, local: bool) -> Result<()> {
    println!(
        "{}",
        format!("Running one-time spex setup [{}]…", tool.display_name()).bold()
    );
    println!();

    // Step 1: Install bundled skills and agents
    let skills_dir = tool
        .skills_dir()
        .ok_or_else(|| anyhow::anyhow!("Could not determine skills directory"))?;
    let agents_dir = tool
        .agents_dir()
        .ok_or_else(|| anyhow::anyhow!("Could not determine agents directory"))?;

    std::fs::create_dir_all(&skills_dir)?;
    std::fs::create_dir_all(&agents_dir)?;

    let skill_count = install_bundled_skills(&skills_dir)?;
    println!(
        "  {} Installed {} skill files → {}",
        "✓".green(),
        skill_count,
        skills_dir.display()
    );

    let agent_count = install_bundled_agents(&agents_dir, tool)?;
    println!(
        "  {} Installed {} agent files → {}",
        "✓".green(),
        agent_count,
        agents_dir.display()
    );

    // Step 2: Write MCP config
    println!();
    crate::cli::mcp_cmd::cmd_mcp_setup(tool, local)?;

    println!();
    println!("{} Setup complete!", "✓".green().bold());
    println!();
    println!(
        "You can now open {} in any project where you have run {} or {}.",
        tool.display_name(),
        "spex init".cyan(),
        "spex new".cyan()
    );

    Ok(())
}
