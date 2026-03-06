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
    let mut results = vec![];

    // 1. .spex/state.db exists and is readable
    results.push(check_state_db());

    // 2. PRD.md exists and is not the default template
    results.push(check_prd());

    // 3. ~/.config/opencode/skills/ exists
    results.push(check_skills_dir());

    // 4. At least one skill is installed
    results.push(check_skills_installed());

    // 5. opencode.json exists in current dir with spex-state MCP entry
    results.push(check_opencode_json());

    // 6. Git repo detected
    results.push(check_git_repo());

    // 7. No specs stuck in in_progress
    results.push(check_stuck_specs().await);

    results
}

fn check_state_db() -> CheckResult {
    let cwd = std::env::current_dir().unwrap_or_default();
    // Walk up looking for .spex/state.db
    let mut current = cwd.clone();
    loop {
        let db_path = current.join(".spex").join("state.db");
        if db_path.exists() {
            // Try to open it
            match std::fs::metadata(&db_path) {
                Ok(_) => {
                    return CheckResult {
                        name: "State DB".to_string(),
                        status: CheckStatus::Pass,
                        message: format!("Found at {}", db_path.display()),
                    };
                }
                Err(e) => {
                    return CheckResult {
                        name: "State DB".to_string(),
                        status: CheckStatus::Fail,
                        message: format!("Cannot read {}: {}", db_path.display(), e),
                    };
                }
            }
        }
        match current.parent() {
            Some(p) => current = p.to_path_buf(),
            None => break,
        }
    }

    CheckResult {
        name: "State DB".to_string(),
        status: CheckStatus::Fail,
        message: "No .spex/state.db found. Run `spex new <name>` to create a project.".to_string(),
    }
}

fn check_prd() -> CheckResult {
    let cwd = std::env::current_dir().unwrap_or_default();
    let prd_path = cwd.join("docs").join("PRD.md");
    if !prd_path.exists() {
        return CheckResult {
            name: "docs/PRD.md".to_string(),
            status: CheckStatus::Warn,
            message: "docs/PRD.md not found. Run `spex init` or create docs/PRD.md manually.".to_string(),
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
    let config_dir = dirs::config_dir().unwrap_or_default();
    let skills_dir = config_dir.join("opencode").join("skills");
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
    let config_dir = dirs::config_dir().unwrap_or_default();
    let skills_dir = config_dir.join("opencode").join("skills");
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

fn check_opencode_json() -> CheckResult {
    let cwd = std::env::current_dir().unwrap_or_default();
    let path = cwd.join("opencode.json");
    if !path.exists() {
        return CheckResult {
            name: "opencode.json".to_string(),
            status: CheckStatus::Warn,
            message: "opencode.json not found. Run `spex mcp setup`.".to_string(),
        };
    }

    match std::fs::read_to_string(&path) {
        Err(e) => CheckResult {
            name: "opencode.json".to_string(),
            status: CheckStatus::Fail,
            message: format!("Cannot read opencode.json: {}", e),
        },
        Ok(content) => {
            let has_spex = content.contains("spex-state");
            if has_spex {
                CheckResult {
                    name: "opencode.json".to_string(),
                    status: CheckStatus::Pass,
                    message: "MCP entry found (spex-state).".to_string(),
                }
            } else {
                CheckResult {
                    name: "opencode.json".to_string(),
                    status: CheckStatus::Warn,
                    message: "opencode.json exists but missing spex-state entry. Run `spex mcp setup`.".to_string(),
                }
            }
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
    match crate::sdd::db::open_project_db().await {
        Err(_) => CheckResult {
            name: "Stuck specs".to_string(),
            status: CheckStatus::Warn,
            message: "Cannot open project DB to check.".to_string(),
        },
        Ok(pool) => match crate::sdd::spec::list_specs(&pool).await {
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

    // Fix 1: Create .spex/ directory if missing
    let cwd = std::env::current_dir().unwrap_or_default();
    let spex_dir = cwd.join(".spex");
    if !spex_dir.exists() {
        match std::fs::create_dir_all(&spex_dir) {
            Ok(_) => results.push(("State DB".to_string(), format!("Created {}", spex_dir.display()))),
            Err(e) => results.push(("State DB".to_string(), format!("Could not create .spex/: {}", e))),
        }
    }

    // Fix 2: Create docs/PRD.md if missing
    let docs_dir = cwd.join("docs");
    let prd_path = docs_dir.join("PRD.md");
    if !prd_path.exists() {
        let name = cwd.file_name().and_then(|n| n.to_str()).unwrap_or("project");
        let template = crate::scaffold::prd_template(name);
        let _ = std::fs::create_dir_all(&docs_dir);
        match std::fs::write(&prd_path, template) {
            Ok(_) => results.push(("docs/PRD.md".to_string(), "Created docs/PRD.md with default template".to_string())),
            Err(e) => results.push(("docs/PRD.md".to_string(), format!("Could not create docs/PRD.md: {}", e))),
        }
    }

    // Fix 3: Install skills if missing
    let config_dir = dirs::config_dir().unwrap_or_default();
    let skills_dir = config_dir.join("opencode").join("skills");
    if !skills_dir.exists() || crate::skills_mgr::list_installed_skills(&skills_dir).map(|s| s.is_empty()).unwrap_or(true) {
        match crate::skills_mgr::install_bundled_skills(&skills_dir) {
            Ok(_) => results.push(("Skills installed".to_string(), format!("Installed all bundled skills to {}", skills_dir.display()))),
            Err(e) => results.push(("Skills installed".to_string(), format!("Could not install skills: {}", e))),
        }
    }

    // Fix 4: Create opencode.json with MCP entry if missing
    let opencode_path = cwd.join("opencode.json");
    if !opencode_path.exists() {
        let config = serde_json::json!({
            "mcp": {
                "spex-state": crate::scaffold::mcp_entry_json()
            }
        });
        match serde_json::to_string_pretty(&config) {
            Ok(json_str) => match std::fs::write(&opencode_path, json_str) {
                Ok(_) => results.push(("opencode.json".to_string(), "Created opencode.json with spex-state MCP entry".to_string())),
                Err(e) => results.push(("opencode.json".to_string(), format!("Could not create opencode.json: {}", e))),
            },
            Err(e) => results.push(("opencode.json".to_string(), format!("Serialization error: {}", e))),
        }
    }

    // Fix 5: Init git repo if missing
    let has_git = {
        let mut found = false;
        let mut cur = cwd.clone();
        loop {
            if cur.join(".git").exists() { found = true; break; }
            match cur.parent() { Some(p) => cur = p.to_path_buf(), None => break }
        }
        found
    };
    if !has_git {
        results.push(("Git repo".to_string(), "Cannot auto-fix: run `git init` manually to create a Git repository.".to_string()));
    }

    results
}
