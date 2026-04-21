use anyhow::{anyhow, Result};
use colored::Colorize;

use crate::host::{Host, HostProfile};
use crate::skills_mgr::install_bundled_agents;

/// Resolve a host name string to a `HostProfile`, defaulting to OpenCode.
fn resolve_host_profile(host: Option<&str>) -> Result<HostProfile> {
    let h = match host {
        None => Host::OpenCode,
        Some(s) => Host::from_str(s).ok_or_else(|| {
            anyhow!(
                "Unknown host '{}'. Valid values: opencode, copilot, vscode",
                s
            )
        })?,
    };
    HostProfile::for_host(h).ok_or_else(|| anyhow!("Could not determine home directory"))
}

/// Prompt the user to pick one or more hosts interactively.
/// Returns the selected hosts, or an error if the prompt is cancelled.
fn prompt_host_selection() -> Result<Vec<Host>> {
    let detected = crate::host::detect_installed_hosts();

    let options = vec!["opencode", "copilot", "vscode"];
    let defaults: Vec<bool> = options
        .iter()
        .map(|name| detected.iter().any(|h| h.name() == *name))
        .collect();

    let ans = inquire::MultiSelect::new("Select host(s) to configure spex for:", options.clone())
        .with_default(
            &defaults
                .iter()
                .enumerate()
                .filter_map(|(i, &v)| if v { Some(i) } else { None })
                .collect::<Vec<_>>(),
        )
        .prompt()?;

    if ans.is_empty() {
        return Err(anyhow!("No host selected. Aborting setup."));
    }

    let hosts = ans.into_iter().filter_map(Host::from_str).collect();
    Ok(hosts)
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

    if let (Some(agents_dir), Some(extension)) = (&profile.agents_dir, profile.agent_extension) {
        std::fs::create_dir_all(agents_dir)?;
        let agent_count = install_bundled_agents(agents_dir, extension)?;
        println!(
            "{} Installed {} agent file(s) to {}",
            "✓".green(),
            agent_count,
            agents_dir.display()
        );
    } else {
        println!(
            "{} {} does not use per-agent files — skipping agent install.",
            "•".dimmed(),
            profile.host.name()
        );
    }

    Ok(())
}

pub fn cmd_skill_list(host: Option<&str>) -> Result<()> {
    let profile = resolve_host_profile(host)?;

    let Some(agents_dir) = &profile.agents_dir else {
        println!(
            "{} {} does not use per-agent files.",
            "ℹ".blue(),
            profile.host.name()
        );
        return Ok(());
    };

    if !agents_dir.exists() {
        println!("{} No agents installed. Run `spex setup`.", "ℹ".blue());
        return Ok(());
    }

    let extension = format!(".{}", profile.agent_extension.unwrap_or("md"));
    let mut agents: Vec<String> = std::fs::read_dir(agents_dir)?
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

/// One-time global setup: install agents (if applicable), then write MCP config.
pub async fn cmd_setup(host: Option<&str>) -> Result<()> {
    println!("{}", "Running one-time spex setup…".bold());
    println!();

    // If no host was specified, show the interactive TUI picker
    let hosts: Vec<Host> = if host.is_none() {
        prompt_host_selection()?
    } else {
        let profile = resolve_host_profile(host)?;
        vec![profile.host]
    };

    for h in &hosts {
        let profile = HostProfile::for_host(h.clone())
            .ok_or_else(|| anyhow!("Could not determine home directory"))?;
        let host_name = h.name();

        println!("{}", format!("── {} ──", host_name).bold());

        // Step 1: Install bundled agents (skip for hosts without agent files)
        if let (Some(agents_dir), Some(extension)) = (&profile.agents_dir, profile.agent_extension)
        {
            std::fs::create_dir_all(agents_dir)?;
            let agent_count = install_bundled_agents(agents_dir, extension)?;
            println!(
                "  {} Installed {} agent file(s) → {}",
                "✓".green(),
                agent_count,
                agents_dir.display()
            );
        } else {
            println!(
                "  {} {} does not use per-agent files — skipping agent install.",
                "•".dimmed(),
                host_name
            );
        }

        // Step 2: Write MCP config — always global for spex setup
        println!();
        crate::cli::mcp_cmd::cmd_mcp_setup(true, Some(host_name))?;
        println!();
    }

    println!("{} Setup complete!", "✓".green().bold());
    println!();

    for h in &hosts {
        let hint = match h {
            Host::OpenCode => format!(
                "You can now open OpenCode in any project where you have run {} or {}.",
                "spex init".cyan(),
                "spex new".cyan()
            ),
            Host::Copilot => format!(
                "You can now use GitHub Copilot CLI in any project where you have run {} or {}.",
                "spex init".cyan(),
                "spex new".cyan()
            ),
            Host::VSCode => format!(
                "You can now use VS Code with the spex MCP server. Open any project where you have run {} or {}.",
                "spex init".cyan(),
                "spex new".cyan()
            ),
        };
        println!("{}", hint);
    }

    Ok(())
}
