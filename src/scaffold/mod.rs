use anyhow::Result;
use colored::Colorize;
use std::path::Path;

use crate::sdd::db::ensure_spex_dir;
use crate::tool_target::ToolTarget;

/// Write the spex-state MCP entry into each detected tool's global config file.
/// Returns the list of tools that were successfully written (or already had the entry).
fn write_global_mcp_configs(tools: &[ToolTarget]) -> Vec<(ToolTarget, std::path::PathBuf, bool)> {
    let mut results = Vec::new();
    for tool in tools {
        if let Some(config_path) = tool.global_mcp_config_path() {
            // Load existing JSON if the file exists
            let existing = if config_path.exists() {
                std::fs::read_to_string(&config_path)
                    .ok()
                    .and_then(|s| serde_json::from_str(&s).ok())
                    .unwrap_or(serde_json::json!({}))
            } else {
                serde_json::json!({})
            };

            let (updated, changed) = tool.merge_mcp_config(existing);

            // Ensure parent directory exists
            if let Some(parent) = config_path.parent() {
                let _ = std::fs::create_dir_all(parent);
            }

            if let Ok(json_str) = serde_json::to_string_pretty(&updated) {
                let _ = std::fs::write(&config_path, json_str);
            }

            results.push((tool.clone(), config_path, changed));
        }
    }
    results
}

pub async fn scaffold_project(name: &str, dir: &Path, yes: bool) -> Result<()> {
    if !yes {
        use inquire::Confirm;
        let confirmed = Confirm::new(&format!("Create new spex project '{}'?", name))
            .with_default(true)
            .prompt()?;
        if !confirmed {
            println!("{} Cancelled.", "ℹ".blue());
            return Ok(());
        }
    }

    println!("{}", format!("Creating spex project: {}", name).bold());
    println!();

    // 1. Create project directory
    std::fs::create_dir_all(dir)?;

    // 2. Create .spex/ directory
    ensure_spex_dir(dir)?;

    // 3. Create .gitignore
    let gitignore_path = dir.join(".gitignore");
    if !gitignore_path.exists() {
        std::fs::write(
            &gitignore_path,
            "target/\nnode_modules/\n",
        )?;
        println!("  {} .gitignore", "created".green());
    }

    // 4. Create README.md
    let readme_path = dir.join("README.md");
    if !readme_path.exists() {
        std::fs::write(
            &readme_path,
            format!(
                "# {}\n\nA spex-driven project.\n\n## Getting Started\n\n```sh\nspex pulse        # project dashboard\nspex spec add SPEC-001 \"First feature\"\n```\n",
                name
            ),
        )?;
        println!("  {} README.md", "created".green());
    }

    // 5. Create docs/ directory and PRD.md
    let docs_dir = dir.join("docs");
    if !docs_dir.exists() {
        std::fs::create_dir_all(&docs_dir)?;
        println!("  {} docs/", "created".green());
    }
    let prd_path = docs_dir.join("PRD.md");
    if !prd_path.exists() {
        std::fs::write(&prd_path, prd_template(name))?;
        println!("  {} docs/PRD.md", "created".green());
    }

    // 6. Write global MCP config for all detected tools (graceful fallback to OpenCode)
    let detected = ToolTarget::detect_installed();
    let tools: Vec<ToolTarget> = if detected.is_empty() {
        vec![ToolTarget::OpenCode]
    } else {
        detected
    };

    let written = write_global_mcp_configs(&tools);

    // 7. Write per-project opencode.json (OpenCode only — backward compat convenience copy)
    let opencode_path = dir.join("opencode.json");
    if !opencode_path.exists() {
        let config = serde_json::json!({
            "mcp": {
                "spex-state": mcp_entry_json()
            }
        });
        std::fs::write(&opencode_path, serde_json::to_string_pretty(&config)?)?;

        // Find global path to display in the message
        let global_path = ToolTarget::OpenCode
            .global_mcp_config_path()
            .map(|p| p.display().to_string())
            .unwrap_or_else(|| "~/.config/opencode/config.json".to_string());

        println!(
            "  {} opencode.json (local copy; global config at {} is the primary)",
            "created".green(),
            global_path.cyan()
        );
    }

    println!();
    println!("{} Project {} created!", "✓".green().bold(), name.cyan());
    println!();

    // Print global DB note
    println!(
        "{}  Global DB will be created at {} on first {}.",
        "ℹ".blue(),
        "~/.local/share/spex/global-state.db".cyan(),
        "spex mcp serve".cyan()
    );
    println!();

    println!("Next steps:");
    println!(
        "  {} Fill in the PRD:          {}",
        "1.".dimmed(),
        format!("cd {} && $EDITOR docs/PRD.md", name).cyan()
    );
    println!(
        "  {} Add your first spec:      {}",
        "2.".dimmed(),
        "spex spec add SPEC-001 \"My first feature\"".cyan()
    );
    println!(
        "  {} View project status:      {}",
        "3.".dimmed(),
        "spex pulse".cyan()
    );
    println!();

    // Build "MCP config written for" summary line
    let tool_labels: Vec<String> = written
        .iter()
        .map(|(tool, _, _)| format!("{} (global)", tool.display_name()))
        .collect();
    if !tool_labels.is_empty() {
        println!(
            "  MCP config written for: {}",
            tool_labels.join(", ").cyan()
        );
    }
    println!(
        "  {}",
        "Tip: run `spex setup` once to install agent skills globally.".dimmed()
    );

    Ok(())
}

pub async fn init_project(dir: &Path) -> Result<()> {
    let name = dir
        .file_name()
        .and_then(|n| n.to_str())
        .unwrap_or("project");

    println!("{}", "Initialising spex in existing project…".bold());
    println!();

    // 1. Create .spex/ directory
    ensure_spex_dir(dir)?;
    println!("  {} .spex/", "created".green());

    // 2. Append to .gitignore (never overwrite)
    let gitignore_path = dir.join(".gitignore");
    let spex_entry = "# spex marker\n.spex/\n";
    if gitignore_path.exists() {
        let existing = std::fs::read_to_string(&gitignore_path)?;
        if !existing.contains(".spex/") {
            let mut f = std::fs::OpenOptions::new()
                .append(true)
                .open(&gitignore_path)?;
            use std::io::Write;
            write!(f, "\n# spex\n{}", spex_entry)?;
            println!(
                "  {} .gitignore (appended spex entries)",
                "updated".yellow()
            );
        } else {
            println!(
                "  {} .gitignore already contains spex entries",
                "skipped".dimmed()
            );
        }
    } else {
        std::fs::write(&gitignore_path, spex_entry)?;
        println!("  {} .gitignore", "created".green());
    }

    // 3. Create docs/ directory and PRD.md if missing
    let docs_dir = dir.join("docs");
    if !docs_dir.exists() {
        std::fs::create_dir_all(&docs_dir)?;
        println!("  {} docs/", "created".green());
    }
    let prd_path = docs_dir.join("PRD.md");
    if !prd_path.exists() {
        std::fs::write(&prd_path, prd_template(name))?;
        println!("  {} docs/PRD.md", "created".green());
    } else {
        println!("  {} docs/PRD.md already exists", "skipped".dimmed());
    }

    // 4. Write global MCP config for all detected tools (graceful fallback to OpenCode)
    let detected = ToolTarget::detect_installed();
    let tools: Vec<ToolTarget> = if detected.is_empty() {
        vec![ToolTarget::OpenCode]
    } else {
        detected
    };

    let written = write_global_mcp_configs(&tools);

    // 5. Write per-project opencode.json (OpenCode only — backward compat convenience copy)
    let opencode_path = dir.join("opencode.json");
    if opencode_path.exists() {
        let raw = std::fs::read_to_string(&opencode_path)?;
        let mut json: serde_json::Value =
            serde_json::from_str(&raw).unwrap_or(serde_json::json!({}));
        let has_spex = json.get("mcp").and_then(|m| m.get("spex-state")).is_some();

        if !has_spex {
            json["mcp"]["spex-state"] = mcp_entry_json();
            std::fs::write(&opencode_path, serde_json::to_string_pretty(&json)?)?;
            println!(
                "  {} opencode.json (merged spex-state MCP entry)",
                "updated".yellow()
            );
        } else {
            println!(
                "  {} opencode.json already has spex-state entry",
                "skipped".dimmed()
            );
        }
    } else {
        let config = serde_json::json!({
            "mcp": {
                "spex-state": mcp_entry_json()
            }
        });
        std::fs::write(&opencode_path, serde_json::to_string_pretty(&config)?)?;

        let global_path = ToolTarget::OpenCode
            .global_mcp_config_path()
            .map(|p| p.display().to_string())
            .unwrap_or_else(|| "~/.config/opencode/config.json".to_string());

        println!(
            "  {} opencode.json (local copy; global config at {} is the primary)",
            "created".green(),
            global_path.cyan()
        );
    }

    println!();
    println!(
        "{} spex initialised in {}",
        "✓".green().bold(),
        dir.display().to_string().cyan()
    );
    println!();

    // Print global DB note
    println!(
        "{}  Global DB will be created at {} on first {}.",
        "ℹ".blue(),
        "~/.local/share/spex/global-state.db".cyan(),
        "spex mcp serve".cyan()
    );
    println!();

    println!("Next steps:");
    println!(
        "  {}  Fill in the PRD:        {}",
        "1.".dimmed(),
        "$EDITOR docs/PRD.md".cyan()
    );
    println!(
        "  {}  Add your first spec:    {}",
        "2.".dimmed(),
        "spex spec add SPEC-001 \"My first feature\"".cyan()
    );
    println!();

    // Build "MCP config written for" summary line
    let tool_labels: Vec<String> = written
        .iter()
        .map(|(tool, _, _)| format!("{} (global)", tool.display_name()))
        .collect();
    if !tool_labels.is_empty() {
        println!(
            "  MCP config written for: {}",
            tool_labels.join(", ").cyan()
        );
    }
    println!(
        "  {}",
        "Tip: run `spex setup` once to install agent skills globally.".dimmed()
    );

    Ok(())
}

/// Canonical OpenCode MCP server entry.
/// Uses the array-command format with type and enabled fields per OpenCode docs.
/// Delegates to `ToolTarget::OpenCode.mcp_entry_json()` for consistency.
pub fn mcp_entry_json() -> serde_json::Value {
    ToolTarget::OpenCode.mcp_entry_json()
}

/// Default PRD template written to docs/PRD.md on project creation.
pub fn prd_template(project_name: &str) -> String {
    format!(
        r#"# {project_name} — Product Requirements Document

> **Status:** draft
> _Fill in each section. The orchestrator agent will guide you through this on first startup._

## Vision

<!-- What is this project? What problem does it solve? Who benefits? -->

## Goals

<!-- Top 3 measurable goals -->

1.
2.
3.

## Non-Goals

<!-- What is explicitly out of scope? -->

## Users

<!-- Who are the target users / personas? -->

## Tech Stack

<!-- Languages, frameworks, databases, infrastructure -->

## Architecture Principles

<!-- Key constraints and decisions that every spec must honour -->

## Acceptance Standards

<!-- What defines "done" for any spec in this project? -->

## Open Questions

<!-- Unresolved decisions that must be answered before or during development -->
"#
    )
}
