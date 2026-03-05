use anyhow::Result;
use colored::Colorize;
use std::path::Path;

use crate::sdd::db::{ensure_spex_dir, get_db_path, open_db};

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
            ".spex/state.db\n.spex/*.db\ntarget/\nnode_modules/\n",
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

    // 5. Create PRD.md in project root
    let prd_path = dir.join("PRD.md");
    if !prd_path.exists() {
        std::fs::write(&prd_path, prd_template(name))?;
        println!("  {} PRD.md", "created".green());
    }

    // 6. Create opencode.json with MCP config (correct OpenCode format)
    let opencode_path = dir.join("opencode.json");
    if !opencode_path.exists() {
        let config = serde_json::json!({
            "mcp": {
                "spex-state": mcp_entry_json()
            }
        });
        std::fs::write(&opencode_path, serde_json::to_string_pretty(&config)?)?;
        println!("  {} opencode.json", "created".green());
    }

    // 7. Initialize the SQLite database (runs migrations)
    let db_path = get_db_path(dir);
    let _pool = open_db(&db_path).await?;
    println!("  {} .spex/state.db", "created".green());

    println!();
    println!("{} Project {} created!", "✓".green().bold(), name.cyan());
    println!();
    println!("Next steps:");
    println!(
        "  {} Fill in the PRD:          {}",
        "1.".dimmed(),
        format!("cd {} && $EDITOR PRD.md", name).cyan()
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
    println!(
        "  MCP config written to {}. Open OpenCode — the orchestrator will help you fill PRD.md.",
        "opencode.json".cyan()
    );
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
    let spex_entry = ".spex/state.db\n.spex/*.db\n";
    if gitignore_path.exists() {
        let existing = std::fs::read_to_string(&gitignore_path)?;
        if !existing.contains(".spex/state.db") {
            let mut f = std::fs::OpenOptions::new()
                .append(true)
                .open(&gitignore_path)?;
            use std::io::Write;
            write!(f, "\n# spex state\n{}", spex_entry)?;
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

    // 3. Create PRD.md if missing
    let prd_path = dir.join("PRD.md");
    if !prd_path.exists() {
        std::fs::write(&prd_path, prd_template(name))?;
        println!("  {} PRD.md", "created".green());
    } else {
        println!("  {} PRD.md already exists", "skipped".dimmed());
    }

    // 4. Merge spex-state into opencode.json (correct OpenCode format; never overwrite existing keys)
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
        println!("  {} opencode.json", "created".green());
    }

    // 5. Initialise the SQLite database (runs migrations)
    let db_path = get_db_path(dir);
    let _pool = open_db(&db_path).await?;
    println!("  {} .spex/state.db", "created".green());

    println!();
    println!(
        "{} spex initialised in {}",
        "✓".green().bold(),
        dir.display().to_string().cyan()
    );
    println!();
    println!("Next steps:");
    println!(
        "  {}  Fill in the PRD:        {}",
        "1.".dimmed(),
        "$EDITOR PRD.md".cyan()
    );
    println!(
        "  {}  Add your first spec:    {}",
        "2.".dimmed(),
        "spex spec add SPEC-001 \"My first feature\"".cyan()
    );
    println!();
    println!(
        "  MCP entry written to {}. Open OpenCode — the orchestrator will help you fill PRD.md.",
        "opencode.json".cyan()
    );
    println!(
        "  {}",
        "Tip: run `spex setup` once to install agent skills globally.".dimmed()
    );

    Ok(())
}

/// Canonical OpenCode MCP server entry.
/// Uses the array-command format with type and enabled fields per OpenCode docs.
pub fn mcp_entry_json() -> serde_json::Value {
    serde_json::json!({
        "type": "local",
        "enabled": true,
        "command": ["spex", "mcp", "serve"]
    })
}

/// Default PRD template written to PRD.md on project creation.
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
