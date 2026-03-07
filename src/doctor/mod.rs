use crate::tool_target::ToolTarget;

pub enum CheckStatus {
    Pass,
    Warn,
    Fail,
}

pub struct CheckResult {
    pub name: String,
    pub status: CheckStatus,
    pub message: String,
}

pub async fn run_checks() -> Vec<CheckResult> {
    // 1. Global DB exists and is readable (FAIL if not; auto-fixable)
    // 2. Stale per-project .spex/state.db (WARN if present)
    // 3. SPEX_PROJECT_DIR env var (WARN if not set)
    // 4. PRD.md exists and is not the default template
    // 5. ~/.config/opencode/skills/ exists
    // 6. At least one skill is installed
    // 7. OpenCode MCP entry in global or local config
    let mut results = vec![
        check_global_db().await,
        check_stale_per_project_db(),
        check_spex_project_dir(),
        check_prd(),
        check_skills_dir(),
        check_skills_installed(),
        check_opencode_mcp(),
    ];

    // 6. Copilot CLI checks (only if ~/.copilot exists)
    if ToolTarget::CopilotCli
        .config_dir()
        .map(|d| d.exists())
        .unwrap_or(false)
    {
        results.push(check_copilot_cli_mcp());
        results.push(check_copilot_cli_skills());
    }

    // 7. Git repo detected
    results.push(check_git_repo());

    // 8. No specs stuck in in_progress
    results.push(check_stuck_specs().await);

    results
}

/// AC-14: Check that the global DB exists and is readable; auto-fixable via `--fix`.
async fn check_global_db() -> CheckResult {
    let path = match crate::sdd::db::global_db_path() {
        Ok(p) => p,
        Err(e) => {
            return CheckResult {
                name: "Global DB".to_string(),
                status: CheckStatus::Fail,
                message: format!("[FAIL] could not determine global DB path: {}", e),
            };
        }
    };

    match std::fs::metadata(&path) {
        Ok(_) => CheckResult {
            name: "Global DB".to_string(),
            status: CheckStatus::Pass,
            message: format!("[OK] global DB: {}", path.display()),
        },
        Err(_) => CheckResult {
            name: "Global DB".to_string(),
            status: CheckStatus::Fail,
            message: format!(
                "[FAIL] global DB not found at {} — run `spex doctor --fix` to create it",
                path.display()
            ),
        },
    }
}

/// AC-13: Warn if a stale per-project `.spex/state.db` exists in or above CWD.
fn check_stale_per_project_db() -> CheckResult {
    let cwd = std::env::current_dir().unwrap_or_default();
    let mut current = cwd.clone();
    loop {
        let db_path = current.join(".spex").join("state.db");
        if db_path.exists() {
            return CheckResult {
                name: "Stale per-project DB".to_string(),
                status: CheckStatus::Warn,
                message: format!(
                    "[WARN] stale per-project DB detected at {} — run 'spex db migrate-to-global' to migrate, then remove it",
                    db_path.display()
                ),
            };
        }
        match current.parent() {
            Some(p) => current = p.to_path_buf(),
            None => break,
        }
    }

    CheckResult {
        name: "Stale per-project DB".to_string(),
        status: CheckStatus::Pass,
        message: "[OK] no stale per-project DB found".to_string(),
    }
}

/// AC-15: Warn if `SPEX_PROJECT_DIR` is not set in the environment.
fn check_spex_project_dir() -> CheckResult {
    match std::env::var("SPEX_PROJECT_DIR") {
        Ok(val) => CheckResult {
            name: "SPEX_PROJECT_DIR".to_string(),
            status: CheckStatus::Pass,
            message: format!("[OK] SPEX_PROJECT_DIR: {}", val),
        },
        Err(_) => CheckResult {
            name: "SPEX_PROJECT_DIR".to_string(),
            status: CheckStatus::Warn,
            message: "[WARN] SPEX_PROJECT_DIR not set — project context will fall back to CWD which may be ambiguous in some editors".to_string(),
        },
    }
}

fn check_prd() -> CheckResult {
    let cwd = std::env::current_dir().unwrap_or_default();
    let prd_path = cwd.join("docs").join("PRD.md");
    if !prd_path.exists() {
        return CheckResult {
            name: "docs/PRD.md".to_string(),
            status: CheckStatus::Warn,
            message: "docs/PRD.md not found. Run `spex init` or create docs/PRD.md manually."
                .to_string(),
        };
    }
    let content = match std::fs::read_to_string(&prd_path) {
        Err(e) => {
            return CheckResult {
                name: "docs/PRD.md".to_string(),
                status: CheckStatus::Fail,
                message: format!("Cannot read docs/PRD.md: {}", e),
            }
        }
        Ok(c) => c,
    };
    let is_template = content.contains("<!-- What is this project?")
        || content.contains("<!-- Top 3 measurable goals")
        || content.contains("<!-- What is explicitly out of scope?");
    if is_template {
        CheckResult {
            name: "docs/PRD.md".to_string(),
            status: CheckStatus::Warn,
            message: "docs/PRD.md contains only the default template. Fill it in with your project details.".to_string(),
        }
    } else {
        CheckResult {
            name: "docs/PRD.md".to_string(),
            status: CheckStatus::Pass,
            message: format!("Found at {}", prd_path.display()),
        }
    }
}

fn check_skills_dir() -> CheckResult {
    let skills_dir = crate::cli::util::opencode_config_dir()
        .unwrap_or_default()
        .join("skills");
    if skills_dir.exists() {
        CheckResult {
            name: "Skills dir".to_string(),
            status: CheckStatus::Pass,
            message: format!("{}", skills_dir.display()),
        }
    } else {
        CheckResult {
            name: "Skills dir".to_string(),
            status: CheckStatus::Warn,
            message: format!(
                "{} not found. Run `spex skill install --all`.",
                skills_dir.display()
            ),
        }
    }
}

fn check_skills_installed() -> CheckResult {
    let skills_dir = crate::cli::util::opencode_config_dir()
        .unwrap_or_default()
        .join("skills");
    match crate::skills_mgr::list_installed_skills(&skills_dir) {
        Err(e) => CheckResult {
            name: "Skills installed".to_string(),
            status: CheckStatus::Fail,
            message: format!("Error: {}", e),
        },
        Ok(skills) if skills.is_empty() => CheckResult {
            name: "Skills installed".to_string(),
            status: CheckStatus::Warn,
            message: "No spex-* skills found. Run `spex skill install --all`.".to_string(),
        },
        Ok(skills) => CheckResult {
            name: "Skills installed".to_string(),
            status: CheckStatus::Pass,
            message: format!("{} skill(s): {}", skills.len(), skills.join(", ")),
        },
    }
}

/// Check whether the spex-state MCP entry exists in either:
///   - Global OpenCode config: ~/.config/opencode/config.json
///   - Local OpenCode config:  ./opencode.json
fn check_opencode_mcp() -> CheckResult {
    let global_path = ToolTarget::OpenCode.global_mcp_config_path();
    let local_path = ToolTarget::OpenCode.local_mcp_config_path();

    // Helper: does this file contain "spex-state"?
    let has_entry = |path: &std::path::Path| -> bool {
        std::fs::read_to_string(path)
            .map(|c| c.contains("spex-state"))
            .unwrap_or(false)
    };

    if let Some(ref p) = global_path {
        if p.exists() && has_entry(p) {
            return CheckResult {
                name: "OpenCode MCP".to_string(),
                status: CheckStatus::Pass,
                message: format!("MCP entry found in {}", p.display()),
            };
        }
    }

    if let Some(ref p) = local_path {
        if p.exists() && has_entry(p) {
            return CheckResult {
                name: "OpenCode MCP".to_string(),
                status: CheckStatus::Pass,
                message: format!("MCP entry found in {}", p.display()),
            };
        }
    }

    CheckResult {
        name: "OpenCode MCP".to_string(),
        status: CheckStatus::Warn,
        message: "Neither ~/.config/opencode/config.json nor ./opencode.json has spex-state. Run `spex mcp setup`.".to_string(),
    }
}

/// Check whether ~/.copilot/mcp-config.json contains the spex-state MCP entry.
/// Only call this when ~/.copilot/ exists.
fn check_copilot_cli_mcp() -> CheckResult {
    let mcp_path = match ToolTarget::CopilotCli.global_mcp_config_path() {
        Some(p) => p,
        None => {
            return CheckResult {
                name: "Copilot CLI MCP".to_string(),
                status: CheckStatus::Warn,
                message: "Cannot determine Copilot CLI config path.".to_string(),
            };
        }
    };

    if !mcp_path.exists() {
        return CheckResult {
            name: "Copilot CLI MCP".to_string(),
            status: CheckStatus::Warn,
            message: "Copilot CLI detected but mcp-config.json not found. Run `spex mcp setup --tool copilot-cli`.".to_string(),
        };
    }

    match std::fs::read_to_string(&mcp_path) {
        Err(e) => CheckResult {
            name: "Copilot CLI MCP".to_string(),
            status: CheckStatus::Fail,
            message: format!("Cannot read mcp-config.json: {}", e),
        },
        Ok(content) => {
            if content.contains("spex-state") {
                CheckResult {
                    name: "Copilot CLI MCP".to_string(),
                    status: CheckStatus::Pass,
                    message: "Copilot CLI MCP entry found.".to_string(),
                }
            } else {
                CheckResult {
                    name: "Copilot CLI MCP".to_string(),
                    status: CheckStatus::Warn,
                    message: "mcp-config.json missing spex-state entry. Run `spex mcp setup --tool copilot-cli`.".to_string(),
                }
            }
        }
    }
}

/// Check whether ~/.copilot/skills/ contains spex-* skill directories.
/// Only call this when ~/.copilot/ exists.
fn check_copilot_cli_skills() -> CheckResult {
    let skills_dir = match ToolTarget::CopilotCli.skills_dir() {
        Some(d) => d,
        None => {
            return CheckResult {
                name: "Copilot CLI skills".to_string(),
                status: CheckStatus::Warn,
                message: "Cannot determine Copilot CLI skills path.".to_string(),
            };
        }
    };

    if !skills_dir.exists() {
        return CheckResult {
            name: "Copilot CLI skills".to_string(),
            status: CheckStatus::Warn,
            message: "Copilot CLI skills not installed. Run `spex skill install --all --tool copilot-cli`.".to_string(),
        };
    }

    let spex_skills: Vec<_> = match std::fs::read_dir(&skills_dir) {
        Err(_) => {
            return CheckResult {
                name: "Copilot CLI skills".to_string(),
                status: CheckStatus::Warn,
                message: "Copilot CLI skills not installed. Run `spex skill install --all --tool copilot-cli`.".to_string(),
            };
        }
        Ok(rd) => rd
            .filter_map(|e| e.ok())
            .filter(|e| {
                e.file_name()
                    .to_str()
                    .map(|n| n.starts_with("spex-"))
                    .unwrap_or(false)
                    && e.path().is_dir()
            })
            .collect(),
    };

    if spex_skills.is_empty() {
        CheckResult {
            name: "Copilot CLI skills".to_string(),
            status: CheckStatus::Warn,
            message: "Copilot CLI skills not installed. Run `spex skill install --all --tool copilot-cli`.".to_string(),
        }
    } else {
        CheckResult {
            name: "Copilot CLI skills".to_string(),
            status: CheckStatus::Pass,
            message: format!(
                "{} spex skill(s) found in ~/.copilot/skills/",
                spex_skills.len()
            ),
        }
    }
}

fn check_git_repo() -> CheckResult {
    let cwd = std::env::current_dir().unwrap_or_default();
    let mut current = cwd.clone();
    loop {
        if current.join(".git").exists() {
            return CheckResult {
                name: "Git repo".to_string(),
                status: CheckStatus::Pass,
                message: format!("Found at {}", current.display()),
            };
        }
        match current.parent() {
            Some(p) => current = p.to_path_buf(),
            None => break,
        }
    }

    CheckResult {
        name: "Git repo".to_string(),
        status: CheckStatus::Warn,
        message: "No .git directory found. Consider `git init`.".to_string(),
    }
}

async fn check_stuck_specs() -> CheckResult {
    // Resolve the project directory the same way mcp_cmd does — SPEX_PROJECT_DIR or CWD.
    let project_dir = std::env::var("SPEX_PROJECT_DIR")
        .ok()
        .or_else(|| {
            std::env::current_dir()
                .ok()
                .and_then(|p| p.canonicalize().ok())
                .map(|p| p.to_string_lossy().into_owned())
        })
        .unwrap_or_default();

    match crate::sdd::db::open_global_db().await {
        Err(_) => CheckResult {
            name: "Stuck specs".to_string(),
            status: CheckStatus::Warn,
            message: "Cannot open global DB to check.".to_string(),
        },
        Ok(pool) => match crate::sdd::spec::list_specs(&pool, &project_dir).await {
            Err(e) => CheckResult {
                name: "Stuck specs".to_string(),
                status: CheckStatus::Fail,
                message: format!("Error: {}", e),
            },
            Ok(specs) => {
                let in_progress: Vec<_> =
                    specs.iter().filter(|s| s.status == "in_progress").collect();
                if in_progress.is_empty() {
                    CheckResult {
                        name: "Stuck specs".to_string(),
                        status: CheckStatus::Pass,
                        message: "No specs stuck in_progress.".to_string(),
                    }
                } else {
                    let ids: Vec<&str> = in_progress.iter().map(|s| s.id.as_str()).collect();
                    CheckResult {
                        name: "Stuck specs".to_string(),
                        status: CheckStatus::Warn,
                        message: format!(
                            "{} spec(s) in_progress: {}",
                            in_progress.len(),
                            ids.join(", ")
                        ),
                    }
                }
            }
        },
    }
}

/// Attempt automatic fixes for failed/warned checks.
/// Returns a vec of (check_name, fix_message) describing what was done or why it was skipped.
pub async fn fix_issues() -> Vec<(String, String)> {
    let mut results = vec![];

    // Fix 1: Create global DB if missing (AC-14)
    let cwd = std::env::current_dir().unwrap_or_default();
    match crate::sdd::db::global_db_path() {
        Err(e) => results.push((
            "Global DB".to_string(),
            format!("Could not determine global DB path: {}", e),
        )),
        Ok(ref path) if !path.exists() => {
            match crate::sdd::db::open_global_db().await {
                Ok(_) => results.push((
                    "Global DB".to_string(),
                    format!("Created global DB at {}", path.display()),
                )),
                Err(e) => results.push((
                    "Global DB".to_string(),
                    format!("Could not create global DB: {}", e),
                )),
            }
        }
        Ok(_) => {} // already exists, nothing to fix
    }

    // Fix 2: Create docs/PRD.md if missing
    let docs_dir = cwd.join("docs");
    let prd_path = docs_dir.join("PRD.md");
    if !prd_path.exists() {
        let name = cwd
            .file_name()
            .and_then(|n| n.to_str())
            .unwrap_or("project");
        let template = crate::scaffold::prd_template(name);
        let _ = std::fs::create_dir_all(&docs_dir);
        match std::fs::write(&prd_path, template) {
            Ok(_) => results.push((
                "docs/PRD.md".to_string(),
                "Created docs/PRD.md with default template".to_string(),
            )),
            Err(e) => results.push((
                "docs/PRD.md".to_string(),
                format!("Could not create docs/PRD.md: {}", e),
            )),
        }
    }

    // Fix 3: Install skills if missing
    let skills_dir = crate::cli::util::opencode_config_dir()
        .unwrap_or_default()
        .join("skills");
    if !skills_dir.exists()
        || crate::skills_mgr::list_installed_skills(&skills_dir)
            .map(|s| s.is_empty())
            .unwrap_or(true)
    {
        match crate::skills_mgr::install_bundled_skills(&skills_dir) {
            Ok(_) => results.push((
                "Skills installed".to_string(),
                format!("Installed all bundled skills to {}", skills_dir.display()),
            )),
            Err(e) => results.push((
                "Skills installed".to_string(),
                format!("Could not install skills: {}", e),
            )),
        }
    }

    // Fix 4: Write global OpenCode config (~/.config/opencode/config.json) and local ./opencode.json
    // with the spex-state MCP entry if either is missing.
    let mcp_entry = ToolTarget::OpenCode.mcp_entry_json();

    // 4a: Global config
    if let Some(global_config_path) = ToolTarget::OpenCode.global_mcp_config_path() {
        let needs_fix = if global_config_path.exists() {
            std::fs::read_to_string(&global_config_path)
                .map(|c| !c.contains("spex-state"))
                .unwrap_or(true)
        } else {
            true
        };

        if needs_fix {
            // Load existing or start fresh
            let existing: serde_json::Value = global_config_path
                .exists()
                .then(|| {
                    std::fs::read_to_string(&global_config_path)
                        .ok()
                        .and_then(|s| serde_json::from_str(&s).ok())
                })
                .flatten()
                .unwrap_or(serde_json::json!({}));

            let (updated, _) = ToolTarget::OpenCode.merge_mcp_config(existing);

            // Ensure parent directory exists
            if let Some(parent) = global_config_path.parent() {
                let _ = std::fs::create_dir_all(parent);
            }

            match serde_json::to_string_pretty(&updated) {
                Ok(json_str) => match std::fs::write(&global_config_path, json_str) {
                    Ok(_) => results.push((
                        "OpenCode MCP".to_string(),
                        format!(
                            "Wrote spex-state MCP entry to {}",
                            global_config_path.display()
                        ),
                    )),
                    Err(e) => results.push((
                        "OpenCode MCP".to_string(),
                        format!(
                            "Could not write {}: {}",
                            global_config_path.display(),
                            e
                        ),
                    )),
                },
                Err(e) => results.push((
                    "OpenCode MCP".to_string(),
                    format!("Serialization error: {}", e),
                )),
            }
        }
    }

    // 4b: Local config (./opencode.json) — always keep in sync as a convenience copy
    let local_config_path = cwd.join("opencode.json");
    let local_needs_fix = if local_config_path.exists() {
        std::fs::read_to_string(&local_config_path)
            .map(|c| !c.contains("spex-state"))
            .unwrap_or(true)
    } else {
        true
    };

    if local_needs_fix {
        let existing: serde_json::Value = local_config_path
            .exists()
            .then(|| {
                std::fs::read_to_string(&local_config_path)
                    .ok()
                    .and_then(|s| serde_json::from_str(&s).ok())
            })
            .flatten()
            .unwrap_or(serde_json::json!({
                "mcp": {
                    "spex-state": mcp_entry
                }
            }));

        let (updated, _) = ToolTarget::OpenCode.merge_mcp_config(existing);

        match serde_json::to_string_pretty(&updated) {
            Ok(json_str) => match std::fs::write(&local_config_path, json_str) {
                Ok(_) => results.push((
                    "opencode.json".to_string(),
                    "Created/updated opencode.json with spex-state MCP entry".to_string(),
                )),
                Err(e) => results.push((
                    "opencode.json".to_string(),
                    format!("Could not write opencode.json: {}", e),
                )),
            },
            Err(e) => results.push((
                "opencode.json".to_string(),
                format!("Serialization error: {}", e),
            )),
        }
    }

    // Fix 5: Copilot CLI — write mcp-config.json if ~/.copilot/ exists but file is missing
    let copilot_config_dir = ToolTarget::CopilotCli.config_dir();
    if let Some(ref copilot_dir) = copilot_config_dir {
        if copilot_dir.exists() {
            if let Some(mcp_config_path) = ToolTarget::CopilotCli.global_mcp_config_path() {
                let needs_fix = if mcp_config_path.exists() {
                    std::fs::read_to_string(&mcp_config_path)
                        .map(|c| !c.contains("spex-state"))
                        .unwrap_or(true)
                } else {
                    true
                };

                if needs_fix {
                    let existing: serde_json::Value = mcp_config_path
                        .exists()
                        .then(|| {
                            std::fs::read_to_string(&mcp_config_path)
                                .ok()
                                .and_then(|s| serde_json::from_str(&s).ok())
                        })
                        .flatten()
                        .unwrap_or(serde_json::json!({}));

                    let (updated, _) = ToolTarget::CopilotCli.merge_mcp_config(existing);

                    match serde_json::to_string_pretty(&updated) {
                        Ok(json_str) => match std::fs::write(&mcp_config_path, json_str) {
                            Ok(_) => results.push((
                                "Copilot CLI MCP".to_string(),
                                format!(
                                    "Wrote spex-state MCP entry to {}",
                                    mcp_config_path.display()
                                ),
                            )),
                            Err(e) => results.push((
                                "Copilot CLI MCP".to_string(),
                                format!(
                                    "Could not write {}: {}",
                                    mcp_config_path.display(),
                                    e
                                ),
                            )),
                        },
                        Err(e) => results.push((
                            "Copilot CLI MCP".to_string(),
                            format!("Serialization error: {}", e),
                        )),
                    }
                }
            }
        }
    }

    // Fix 6: Init git repo if missing
    let has_git = {
        let mut found = false;
        let mut cur = cwd.clone();
        loop {
            if cur.join(".git").exists() {
                found = true;
                break;
            }
            match cur.parent() {
                Some(p) => cur = p.to_path_buf(),
                None => break,
            }
        }
        found
    };
    if !has_git {
        results.push((
            "Git repo".to_string(),
            "Cannot auto-fix: run `git init` manually to create a Git repository.".to_string(),
        ));
    }

    results
}
