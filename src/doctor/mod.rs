use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};

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

    // 8. Bundled or installed agent prompts reference real tools and agents
    results.push(check_prompt_runtime_consistency());

    // 9. Hard-coded documentation counts match runtime reality
    results.push(check_documentation_count_drift());

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
    let agents_dir = crate::cli::util::opencode_config_dir()
        .unwrap_or_default()
        .join("agents");
    if agents_dir.exists() {
        CheckResult {
            name: "Agents dir".to_string(),
            status: CheckStatus::Pass,
            message: format!("{}", agents_dir.display()),
        }
    } else {
        CheckResult {
            name: "Agents dir".to_string(),
            status: CheckStatus::Warn,
            message: format!("{} not found. Run `spex setup`.", agents_dir.display()),
        }
    }
}

fn check_skills_installed() -> CheckResult {
    let agents_dir = crate::cli::util::opencode_config_dir()
        .unwrap_or_default()
        .join("agents");

    if !agents_dir.exists() {
        return CheckResult {
            name: "Agents installed".to_string(),
            status: CheckStatus::Warn,
            message: "No agents directory found. Run `spex setup`.".to_string(),
        };
    }

    let agents: Vec<String> = match std::fs::read_dir(&agents_dir) {
        Err(e) => {
            return CheckResult {
                name: "Agents installed".to_string(),
                status: CheckStatus::Fail,
                message: format!("Error reading agents dir: {}", e),
            }
        }
        Ok(entries) => entries
            .filter_map(|e| e.ok())
            .filter(|e| e.path().extension().and_then(|s| s.to_str()) == Some("md"))
            .map(|e| e.file_name().to_string_lossy().to_string())
            .collect(),
    };

    if agents.is_empty() {
        CheckResult {
            name: "Agents installed".to_string(),
            status: CheckStatus::Warn,
            message: "No agent .md files found. Run `spex setup`.".to_string(),
        }
    } else {
        CheckResult {
            name: "Agents installed".to_string(),
            status: CheckStatus::Pass,
            message: format!("{} agent(s): {}", agents.len(), agents.join(", ")),
        }
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
                    message:
                        "opencode.json exists but missing spex-state entry. Run `spex mcp setup`."
                            .to_string(),
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
        Ok(pool) => match crate::sdd::spec::list_specs(&pool, None, None).await {
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

fn check_prompt_runtime_consistency() -> CheckResult {
    let Some(agent_files) = discover_agent_prompt_files() else {
        return CheckResult {
            name: "Prompt/runtime consistency".to_string(),
            status: CheckStatus::Warn,
            message: "No bundled or installed agent prompt files found to validate.".to_string(),
        };
    };

    let tool_count = crate::mcp::server::canonical_tool_names().len();

    let (bad_tools, bad_agents) = validate_prompt_refs(&agent_files);

    if bad_tools.is_empty() && bad_agents.is_empty() {
        return CheckResult {
            name: "Prompt/runtime consistency".to_string(),
            status: CheckStatus::Pass,
            message: format!(
                "Validated {} agent prompt(s) against {} MCP tool(s).",
                agent_files.len(),
                tool_count
            ),
        };
    }

    let mut parts = Vec::new();
    if !bad_tools.is_empty() {
        parts.push(format!("unknown MCP tools: {}", format_bad_refs(&bad_tools)));
    }
    if !bad_agents.is_empty() {
        parts.push(format!("unknown agent refs: {}", format_bad_refs(&bad_agents)));
    }

    CheckResult {
        name: "Prompt/runtime consistency".to_string(),
        status: CheckStatus::Fail,
        message: parts.join("; "),
    }
}

fn check_documentation_count_drift() -> CheckResult {
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let expected_agents = crate::skills_mgr::bundled_agent_names().len();
    let expected_tools = crate::mcp::server::canonical_tool_names().len();
    let expected_checks = 9usize;

    let expectations = [
        CountExpectation {
            path: root.join("README.md"),
            needle: "bundled AI agent files",
            expected: expected_agents,
            label: "bundled agents",
        },
        CountExpectation {
            path: root.join("docs/PRD.md"),
            needle: "bundled agent markdown files",
            expected: expected_agents,
            label: "bundled agents",
        },
        CountExpectation {
            path: root.join("docs/PRD.md"),
            needle: "canonical tools",
            expected: expected_tools,
            label: "canonical tools",
        },
        CountExpectation {
            path: root.join("docs/PRD.md"),
            needle: "health checks",
            expected: expected_checks,
            label: "health checks",
        },
        CountExpectation {
            path: root.join("docs/adr/ADR-001-architecture.md"),
            needle: "bundled agent markdown files",
            expected: expected_agents,
            label: "bundled agents",
        },
        CountExpectation {
            path: root.join("docs/adr/ADR-001-architecture.md"),
            needle: "canonical tools",
            expected: expected_tools,
            label: "canonical tools",
        },
    ];

    let mismatches = validate_doc_count_expectations(&expectations);

    if mismatches.is_empty() {
        return CheckResult {
            name: "Documentation count drift".to_string(),
            status: CheckStatus::Pass,
            message: format!(
                "Validated documentation counts for {} bundled agents, {} MCP tools, and {} doctor checks.",
                expected_agents, expected_tools, expected_checks
            ),
        };
    }

    CheckResult {
        name: "Documentation count drift".to_string(),
        status: CheckStatus::Fail,
        message: mismatches.join("; "),
    }
}

struct CountExpectation {
    path: PathBuf,
    needle: &'static str,
    expected: usize,
    label: &'static str,
}

fn validate_doc_count_expectations(expectations: &[CountExpectation]) -> Vec<String> {
    let mut mismatches = Vec::new();

    for expectation in expectations {
        let raw = match std::fs::read_to_string(&expectation.path) {
            Ok(raw) => raw,
            Err(err) => {
                mismatches.push(format!(
                    "{}: could not read {}: {}",
                    expectation.label,
                    expectation.path.display(),
                    err
                ));
                continue;
            }
        };

        let actual = extract_count_for_phrase(&raw, expectation.needle);
        match actual {
            Some(actual) if actual == expectation.expected => {}
            Some(actual) => mismatches.push(format!(
                "{}: {} says {} but expected {}",
                expectation.label,
                expectation.path.display(),
                actual,
                expectation.expected
            )),
            None => mismatches.push(format!(
                "{}: {} does not contain a parseable count for '{}'",
                expectation.label,
                expectation.path.display(),
                expectation.needle
            )),
        }
    }

    mismatches
}

fn extract_count_for_phrase(content: &str, needle: &str) -> Option<usize> {
    for line in content.lines() {
        if !line.contains(needle) {
            continue;
        }
        if let Some(value) = first_ascii_number(line) {
            return Some(value);
        }
    }
    None
}

fn first_ascii_number(line: &str) -> Option<usize> {
    let bytes = line.as_bytes();
    let mut i = 0;
    while i < bytes.len() {
        if !bytes[i].is_ascii_digit() {
            i += 1;
            continue;
        }
        let start = i;
        while i < bytes.len() && bytes[i].is_ascii_digit() {
            i += 1;
        }
        return line[start..i].parse().ok();
    }
    None
}

fn validate_prompt_refs(
    agent_files: &BTreeMap<String, PathBuf>,
) -> (BTreeMap<String, Vec<String>>, BTreeMap<String, Vec<String>>) {
    let canonical_tools: BTreeSet<String> = crate::mcp::server::canonical_tool_names()
        .into_iter()
        .collect();
    let available_agents: BTreeSet<String> = agent_files
        .keys()
        .cloned()
        .chain(crate::skills_mgr::bundled_agent_names())
        .collect();

    let mut bad_tools: BTreeMap<String, Vec<String>> = BTreeMap::new();
    let mut bad_agents: BTreeMap<String, Vec<String>> = BTreeMap::new();

    for (agent_name, path) in agent_files {
        let Ok(content) = std::fs::read_to_string(path) else {
            continue;
        };

        let unknown_tools: Vec<String> = extract_mcp_tool_references(&content)
            .into_iter()
            .filter(|tool| !canonical_tools.contains(tool))
            .collect();
        if !unknown_tools.is_empty() {
            bad_tools.insert(agent_name.clone(), unknown_tools);
        }

        let unknown_agents: Vec<String> = extract_agent_references(&content)
            .into_iter()
            .filter(|agent| !available_agents.contains(agent))
            .collect();
        if !unknown_agents.is_empty() {
            bad_agents.insert(agent_name.clone(), unknown_agents);
        }
    }

    (bad_tools, bad_agents)
}

fn discover_agent_prompt_files() -> Option<BTreeMap<String, PathBuf>> {
    let cwd = std::env::current_dir().unwrap_or_default();
    let local_agents_dir = cwd.join("agents");
    if local_agents_dir.exists() {
        let files = collect_agent_prompt_files(&local_agents_dir);
        if !files.is_empty() {
            return Some(files);
        }
    }

    let installed_agents_dir = crate::cli::util::opencode_config_dir()
        .unwrap_or_default()
        .join("agents");
    if installed_agents_dir.exists() {
        let files = collect_agent_prompt_files(&installed_agents_dir);
        if !files.is_empty() {
            return Some(files);
        }
    }

    None
}

fn collect_agent_prompt_files(dir: &Path) -> BTreeMap<String, PathBuf> {
    let mut files = BTreeMap::new();
    if let Ok(entries) = std::fs::read_dir(dir) {
        for entry in entries.filter_map(|entry| entry.ok()) {
            let path = entry.path();
            if path.extension().and_then(|ext| ext.to_str()) != Some("md") {
                continue;
            }
            if let Some(stem) = path.file_stem().and_then(|stem| stem.to_str()) {
                files.insert(stem.to_string(), path);
            }
        }
    }
    files
}

fn extract_mcp_tool_references(content: &str) -> Vec<String> {
    extract_prefixed_identifiers(content, &["state_", "memory_"])
}

fn extract_agent_references(content: &str) -> Vec<String> {
    let mut refs = BTreeSet::new();
    let chars: Vec<char> = content.chars().collect();
    let mut i = 0;
    while i < chars.len() {
        if chars[i] != '@' {
            i += 1;
            continue;
        }

        let start = i + 1;
        let mut end = start;
        while end < chars.len() && (chars[end].is_ascii_alphanumeric() || chars[end] == '-' || chars[end] == '_') {
            end += 1;
        }

        if end > start {
            refs.insert(chars[start..end].iter().collect::<String>());
        }
        i = end;
    }
    refs.into_iter().collect()
}

fn extract_prefixed_identifiers(content: &str, prefixes: &[&str]) -> Vec<String> {
    let mut found = BTreeSet::new();
    let chars: Vec<char> = content.chars().collect();
    let mut i = 0;

    while i < chars.len() {
        if !chars[i].is_ascii_lowercase() {
            i += 1;
            continue;
        }

        let start = i;
        let mut end = i;
        while end < chars.len() && (chars[end].is_ascii_lowercase() || chars[end] == '_') {
            end += 1;
        }
        let token: String = chars[start..end].iter().collect();
        if prefixes.iter().any(|prefix| token.starts_with(prefix)) {
            found.insert(token);
        }
        i = end;
    }

    found.into_iter().collect()
}

fn format_bad_refs(entries: &BTreeMap<String, Vec<String>>) -> String {
    entries
        .iter()
        .map(|(agent, refs)| format!("{}=[{}]", agent, refs.join(", ")))
        .collect::<Vec<_>>()
        .join("; ")
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
            Ok(_) => results.push((
                "State DB".to_string(),
                format!("Created {}", spex_dir.display()),
            )),
            Err(e) => results.push((
                "State DB".to_string(),
                format!("Could not create .spex/: {}", e),
            )),
        }
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

    // Fix 3: Install agents if missing
    let agents_dir = crate::cli::util::opencode_config_dir()
        .unwrap_or_default()
        .join("agents");
    let agents_missing = !agents_dir.exists()
        || std::fs::read_dir(&agents_dir)
            .map(|mut d| d.next().is_none())
            .unwrap_or(true);
    if agents_missing {
        match crate::skills_mgr::install_bundled_agents(&agents_dir) {
            Ok(n) => results.push((
                "Agents installed".to_string(),
                format!("Installed {} agent file(s) to {}", n, agents_dir.display()),
            )),
            Err(e) => results.push((
                "Agents installed".to_string(),
                format!("Could not install agents: {}", e),
            )),
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
                Ok(_) => results.push((
                    "opencode.json".to_string(),
                    "Created opencode.json with spex-state MCP entry".to_string(),
                )),
                Err(e) => results.push((
                    "opencode.json".to_string(),
                    format!("Could not create opencode.json: {}", e),
                )),
            },
            Err(e) => results.push((
                "opencode.json".to_string(),
                format!("Serialization error: {}", e),
            )),
        }
    }

    // Fix 5: Init git repo if missing
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

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    #[test]
    fn extracts_mcp_tool_references_from_prompt_content() {
        let content = r#"
Run `state_snapshot`, then call memory_get(agent="x", key="y") and state_event_query.
Ignore state_fake_tool and memory_missing_tool later.
"#;

        let refs = extract_mcp_tool_references(content);

        assert!(refs.contains(&"state_snapshot".to_string()));
        assert!(refs.contains(&"memory_get".to_string()));
        assert!(refs.contains(&"state_event_query".to_string()));
        assert!(refs.contains(&"state_fake_tool".to_string()));
        assert!(refs.contains(&"memory_missing_tool".to_string()));
    }

    #[test]
    fn extracts_agent_references_from_prompt_content() {
        let content = "Delegate to @debugger, @repo-explorer, or @security-reviewer if needed.";

        let refs = extract_agent_references(content);

        assert!(refs.contains(&"debugger".to_string()));
        assert!(refs.contains(&"repo-explorer".to_string()));
        assert!(refs.contains(&"security-reviewer".to_string()));
    }

    #[test]
    fn formats_bad_refs_compactly() {
        let mut bad = BTreeMap::new();
        bad.insert(
            "agent-a".to_string(),
            vec!["state_fake".to_string(), "memory_missing".to_string()],
        );

        let formatted = format_bad_refs(&bad);
        assert_eq!(formatted, "agent-a=[state_fake, memory_missing]");
    }

    #[test]
    fn extracts_first_ascii_number_from_line() {
        assert_eq!(first_ascii_number("12 bundled agent markdown files"), Some(12));
        assert_eq!(first_ascii_number("No count here"), None);
    }

    #[test]
    fn extract_count_for_phrase_reads_matching_line() {
        let content = "foo\n23 canonical tools covering things\nbar\n";
        assert_eq!(extract_count_for_phrase(content, "canonical tools"), Some(23));
        assert_eq!(extract_count_for_phrase(content, "health checks"), None);
    }

    #[test]
    fn validate_doc_count_expectations_reports_mismatch() {
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("README.md");
        std::fs::write(&file, "12 bundled AI agent files\n").unwrap();

        let expectations = [CountExpectation {
            path: file,
            needle: "bundled AI agent files",
            expected: 99,
            label: "bundled agents",
        }];

        let mismatches = validate_doc_count_expectations(&expectations);
        assert_eq!(mismatches.len(), 1);
        assert!(mismatches[0].contains("expected 99"));
    }

    #[test]
    fn bundled_agent_prompts_match_runtime_tools_and_agent_refs() {
        let root = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
        let agents_dir = root.join("agents");
        let agent_files = collect_agent_prompt_files(&agents_dir);

        assert!(
            !agent_files.is_empty(),
            "expected bundled agent prompts under {}",
            agents_dir.display()
        );

        let (bad_tools, bad_agents) = validate_prompt_refs(&agent_files);

        assert!(
            bad_tools.is_empty(),
            "bundled prompts reference unknown MCP tools: {}",
            format_bad_refs(&bad_tools)
        );
        assert!(
            bad_agents.is_empty(),
            "bundled prompts reference unknown agents: {}",
            format_bad_refs(&bad_agents)
        );
    }
}
