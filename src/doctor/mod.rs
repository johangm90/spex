use anyhow::Result;
use sqlx::SqlitePool;
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

    // 3. At least one host agents dir exists
    results.push(check_agents_dir());

    // 4. At least one agent is installed
    results.push(check_agents_installed());

    // 5. opencode.json exists in current dir with spex-state MCP entry
    results.push(check_opencode_json());

    // 6. Git repo detected
    results.push(check_git_repo());

    // 7. Control-plane lifecycle and relationship invariants hold
    results.push(check_control_plane_invariants().await);

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

fn check_agents_dir() -> CheckResult {
    use crate::host::{Host, HostProfile};

    let hosts = [Host::OpenCode, Host::Copilot, Host::Pi];
    let mut found_dirs: Vec<String> = Vec::new();

    for host in &hosts {
        if let Some(profile) = HostProfile::for_host(host.clone()) {
            if let Some(agents_dir) = &profile.agents_dir {
                if agents_dir.exists() {
                    found_dirs.push(format!("{} ({})", agents_dir.display(), host.name()));
                }
            }
        }
    }

    if found_dirs.is_empty() {
        CheckResult {
            name: "Agents dir".to_string(),
            status: CheckStatus::Warn,
            message: "No agents directory found for any host. Run `spex setup`.".to_string(),
        }
    } else {
        CheckResult {
            name: "Agents dir".to_string(),
            status: CheckStatus::Pass,
            message: found_dirs.join(", "),
        }
    }
}

fn check_agents_installed() -> CheckResult {
    use crate::host::{Host, HostProfile};

    let hosts = [Host::OpenCode, Host::Copilot, Host::Pi];
    let mut total = 0usize;
    let mut details: Vec<String> = Vec::new();

    for host in &hosts {
        let Some(profile) = HostProfile::for_host(host.clone()) else {
            continue;
        };
        let Some(agents_dir) = &profile.agents_dir else {
            continue;
        };
        if !agents_dir.exists() {
            continue;
        }
        let extension = format!(".{}", profile.agent_extension.unwrap_or("md"));
        let count = std::fs::read_dir(agents_dir)
            .map(|entries| {
                entries
                    .filter_map(|e| e.ok())
                    .filter(|e| {
                        e.path()
                            .file_name()
                            .and_then(|s| s.to_str())
                            .map(|name| name.ends_with(&extension))
                            .unwrap_or(false)
                    })
                    .count()
            })
            .unwrap_or(0);
        if count > 0 {
            total += count;
            details.push(format!("{} for {}", count, host.name()));
        }
    }

    if total == 0 {
        CheckResult {
            name: "Agents installed".to_string(),
            status: CheckStatus::Warn,
            message: "No agent files found for any host. Run `spex setup`.".to_string(),
        }
    } else {
        CheckResult {
            name: "Agents installed".to_string(),
            status: CheckStatus::Pass,
            message: format!("{} agent(s) total: {}", total, details.join(", ")),
        }
    }
}

fn check_opencode_json() -> CheckResult {
    let cwd = std::env::current_dir().unwrap_or_default();

    // Check opencode.json (OpenCode / Copilot project config)
    let opencode_path = cwd.join("opencode.json");
    // Check .vscode/mcp.json (VS Code project config)
    let vscode_path = cwd.join(".vscode").join("mcp.json");

    let candidates = [
        ("opencode.json", &opencode_path),
        (".vscode/mcp.json", &vscode_path),
    ];

    let mut found: Vec<String> = Vec::new();
    let mut missing: Vec<String> = Vec::new();

    for (label, path) in &candidates {
        if !path.exists() {
            missing.push(label.to_string());
            continue;
        }
        match std::fs::read_to_string(path) {
            Err(e) => {
                return CheckResult {
                    name: "MCP project config".to_string(),
                    status: CheckStatus::Fail,
                    message: format!("Cannot read {}: {}", label, e),
                }
            }
            Ok(content) => {
                if content.contains("spex-state") {
                    found.push(label.to_string());
                } else {
                    return CheckResult {
                        name: "MCP project config".to_string(),
                        status: CheckStatus::Warn,
                        message: format!(
                            "{} exists but missing spex-state entry. Run `spex mcp setup`.",
                            label
                        ),
                    };
                }
            }
        }
    }

    if found.is_empty() {
        CheckResult {
            name: "MCP project config".to_string(),
            status: CheckStatus::Warn,
            message: "No MCP project config found (opencode.json or .vscode/mcp.json). Run `spex mcp setup`.".to_string(),
        }
    } else {
        CheckResult {
            name: "MCP project config".to_string(),
            status: CheckStatus::Pass,
            message: format!("MCP entry found in: {}", found.join(", ")),
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

async fn check_control_plane_invariants() -> CheckResult {
    match crate::sdd::db::open_project_db().await {
        Err(_) => CheckResult {
            name: "Control-plane invariants".to_string(),
            status: CheckStatus::Warn,
            message: "Cannot open project DB to check.".to_string(),
        },
        Ok(pool) => match evaluate_control_plane_invariants(&pool).await {
            Err(e) => CheckResult {
                name: "Control-plane invariants".to_string(),
                status: CheckStatus::Fail,
                message: format!("Error: {}", e),
            },
            Ok(result) => result,
        },
    }
}

async fn evaluate_control_plane_invariants(pool: &SqlitePool) -> Result<CheckResult> {
    let specs = crate::sdd::spec::list_specs(pool, None, None).await?;
    let tasks = crate::sdd::task::list_tasks(pool, None, None, None).await?;
    let events =
        crate::sdd::event::query_events(pool, None, None, None, None, None, None, None).await?;

    let spec_ids: BTreeSet<String> = specs.iter().map(|spec| spec.id.clone()).collect();
    let specs_by_id: BTreeMap<String, crate::sdd::spec::Spec> = specs
        .iter()
        .cloned()
        .map(|spec| (spec.id.clone(), spec))
        .collect();
    let tasks_by_id: BTreeMap<String, crate::sdd::task::Task> = tasks
        .iter()
        .cloned()
        .map(|task| (task.id.clone(), task))
        .collect();

    let mut failures = Vec::new();
    let mut warnings = Vec::new();

    let mut unfinished_by_done_spec: BTreeMap<String, Vec<String>> = BTreeMap::new();
    for task in &tasks {
        if task.status != "done" {
            if let Some(spec) = specs_by_id.get(&task.spec) {
                if spec.status == "done" {
                    unfinished_by_done_spec
                        .entry(spec.id.clone())
                        .or_default()
                        .push(format!("{}({})", task.id, task.status));
                }
            }
        }
    }
    if !unfinished_by_done_spec.is_empty() {
        failures.push(format!(
            "done specs with unfinished tasks: {}",
            format_grouped_ids(&unfinished_by_done_spec)
        ));
    }

    let mut missing_spec_dependencies: BTreeMap<String, Vec<String>> = BTreeMap::new();
    for spec in &specs {
        let depends_on: Vec<String> = serde_json::from_str(&spec.depends_on).unwrap_or_default();
        let missing: Vec<String> = depends_on
            .into_iter()
            .filter(|dependency| !spec_ids.contains(dependency))
            .collect();
        if !missing.is_empty() {
            missing_spec_dependencies.insert(spec.id.clone(), missing);
        }
    }
    if !missing_spec_dependencies.is_empty() {
        failures.push(format!(
            "specs with missing depends_on references: {}",
            format_grouped_ids(&missing_spec_dependencies)
        ));
    }

    let mut missing_task_inputs: BTreeMap<String, Vec<String>> = BTreeMap::new();
    for task in &tasks {
        let inputs: Vec<String> = serde_json::from_str(&task.inputs).unwrap_or_default();
        let missing: Vec<String> = inputs
            .into_iter()
            .filter(|input| !tasks_by_id.contains_key(input))
            .collect();
        if !missing.is_empty() {
            missing_task_inputs.insert(task.id.clone(), missing);
        }
    }
    if !missing_task_inputs.is_empty() {
        failures.push(format!(
            "tasks with missing input references: {}",
            format_grouped_ids(&missing_task_inputs)
        ));
    }

    let mut spec_started = BTreeSet::new();
    let mut spec_completed = BTreeSet::new();
    let mut task_started = BTreeSet::new();
    let mut task_completed = BTreeSet::new();
    let mut task_failed = BTreeSet::new();
    let mut orphaned_spec_events = Vec::new();
    let mut orphaned_task_events = Vec::new();
    let mut malformed_task_events = Vec::new();
    let mut task_event_spec_mismatches = Vec::new();

    for event in &events {
        match event.r#type.as_str() {
            "SpecStarted" => {
                if let Some(spec_id) = event.spec.as_deref() {
                    if specs_by_id.contains_key(spec_id) {
                        spec_started.insert(spec_id.to_string());
                    } else {
                        orphaned_spec_events
                            .push(format!("#{}:{}({})", event.id, event.r#type, spec_id));
                    }
                }
            }
            "SpecCompleted" => {
                if let Some(spec_id) = event.spec.as_deref() {
                    if specs_by_id.contains_key(spec_id) {
                        spec_completed.insert(spec_id.to_string());
                    } else {
                        orphaned_spec_events
                            .push(format!("#{}:{}({})", event.id, event.r#type, spec_id));
                    }
                }
            }
            "TaskStarted" | "TaskCompleted" | "TaskFailed" => {
                let task_id = extract_task_id_from_payload(&event.payload);
                let Some(task_id) = task_id else {
                    malformed_task_events.push(format!("#{}:{}", event.id, event.r#type));
                    continue;
                };

                let Some(task) = tasks_by_id.get(&task_id) else {
                    orphaned_task_events
                        .push(format!("#{}:{}({})", event.id, event.r#type, task_id));
                    continue;
                };

                if event.spec.as_deref() != Some(task.spec.as_str()) {
                    task_event_spec_mismatches.push(format!(
                        "#{}:{}({} -> event spec {:?}, task spec {})",
                        event.id, event.r#type, task_id, event.spec, task.spec
                    ));
                }

                match event.r#type.as_str() {
                    "TaskStarted" => {
                        task_started.insert(task_id);
                    }
                    "TaskCompleted" => {
                        task_completed.insert(task_id);
                    }
                    "TaskFailed" => {
                        task_failed.insert(task_id);
                    }
                    _ => {}
                }
            }
            _ => {}
        }
    }

    if !orphaned_spec_events.is_empty() {
        failures.push(format!(
            "orphaned spec lifecycle events: {}",
            orphaned_spec_events.join(", ")
        ));
    }
    if !orphaned_task_events.is_empty() {
        failures.push(format!(
            "orphaned task lifecycle events: {}",
            orphaned_task_events.join(", ")
        ));
    }
    if !malformed_task_events.is_empty() {
        warnings.push(format!(
            "legacy task lifecycle events missing payload.task: {}",
            malformed_task_events.join(", ")
        ));
    }
    if !task_event_spec_mismatches.is_empty() {
        failures.push(format!(
            "task lifecycle event spec mismatches: {}",
            task_event_spec_mismatches.join(", ")
        ));
    }

    let missing_started_specs: Vec<String> = specs
        .iter()
        .filter(|spec| matches!(spec.status.as_str(), "in_progress" | "done"))
        .filter(|spec| !spec_started.contains(spec.id.as_str()))
        .map(|spec| format!("{}({})", spec.id, spec.status))
        .collect();
    if !missing_started_specs.is_empty() {
        failures.push(format!(
            "specs missing SpecStarted events: {}",
            missing_started_specs.join(", ")
        ));
    }

    let missing_completed_specs: Vec<String> = specs
        .iter()
        .filter(|spec| spec.status == "done")
        .filter(|spec| !spec_completed.contains(spec.id.as_str()))
        .map(|spec| spec.id.clone())
        .collect();
    if !missing_completed_specs.is_empty() {
        failures.push(format!(
            "done specs missing SpecCompleted events: {}",
            missing_completed_specs.join(", ")
        ));
    }

    let missing_started_tasks: Vec<String> = tasks
        .iter()
        .filter(|task| matches!(task.status.as_str(), "in_progress" | "done" | "failed"))
        .filter(|task| !task_started.contains(task.id.as_str()))
        .map(|task| format!("{}({})", task.id, task.status))
        .collect();
    if !missing_started_tasks.is_empty() {
        failures.push(format!(
            "tasks missing TaskStarted events: {}",
            missing_started_tasks.join(", ")
        ));
    }

    let missing_completed_tasks: Vec<String> = tasks
        .iter()
        .filter(|task| task.status == "done")
        .filter(|task| !task_completed.contains(task.id.as_str()))
        .map(|task| task.id.clone())
        .collect();
    if !missing_completed_tasks.is_empty() {
        failures.push(format!(
            "done tasks missing TaskCompleted events: {}",
            missing_completed_tasks.join(", ")
        ));
    }

    let missing_failed_tasks: Vec<String> = tasks
        .iter()
        .filter(|task| task.status == "failed")
        .filter(|task| !task_failed.contains(task.id.as_str()))
        .map(|task| task.id.clone())
        .collect();
    if !missing_failed_tasks.is_empty() {
        failures.push(format!(
            "failed tasks missing TaskFailed events: {}",
            missing_failed_tasks.join(", ")
        ));
    }

    let in_progress_specs: Vec<String> = specs
        .iter()
        .filter(|spec| spec.status == "in_progress")
        .map(|spec| spec.id.clone())
        .collect();
    if !in_progress_specs.is_empty() {
        warnings.push(format!(
            "specs still in_progress: {}",
            in_progress_specs.join(", ")
        ));
    }

    let (status, message) = if !failures.is_empty() {
        let mut message = failures.join("; ");
        if !warnings.is_empty() {
            message.push_str("; ");
            message.push_str(&warnings.join("; "));
        }
        (CheckStatus::Fail, message)
    } else if !warnings.is_empty() {
        (CheckStatus::Warn, warnings.join("; "))
    } else {
        (
            CheckStatus::Pass,
            format!(
                "Validated {} specs, {} tasks, and {} events for lifecycle consistency.",
                specs.len(),
                tasks.len(),
                events.len()
            ),
        )
    };

    Ok(CheckResult {
        name: "Control-plane invariants".to_string(),
        status,
        message,
    })
}

fn extract_task_id_from_payload(payload: &str) -> Option<String> {
    let value = serde_json::from_str::<serde_json::Value>(payload).ok()?;
    value
        .get("task")
        .and_then(|v| v.as_str())
        .or_else(|| value.get("task_id").and_then(|v| v.as_str()))
        .map(ToOwned::to_owned)
}

fn format_grouped_ids(entries: &BTreeMap<String, Vec<String>>) -> String {
    entries
        .iter()
        .map(|(parent, children)| format!("{}=[{}]", parent, children.join(", ")))
        .collect::<Vec<_>>()
        .join("; ")
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
        parts.push(format!(
            "unknown MCP tools: {}",
            format_bad_refs(&bad_tools)
        ));
    }
    if !bad_agents.is_empty() {
        parts.push(format!(
            "unknown agent refs: {}",
            format_bad_refs(&bad_agents)
        ));
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
        if let Some(value) = nearest_ascii_number_before_phrase(line, needle) {
            return Some(value);
        }
        if let Some(value) = first_ascii_number(line) {
            return Some(value);
        }
    }
    None
}

fn nearest_ascii_number_before_phrase(line: &str, needle: &str) -> Option<usize> {
    let needle_start = line.find(needle)?;
    let prefix = &line[..needle_start];
    let bytes = prefix.as_bytes();

    let mut end = bytes.len();
    while end > 0 {
        let idx = end - 1;
        if !bytes[idx].is_ascii_digit() {
            end -= 1;
            continue;
        }

        let mut start = idx;
        while start > 0 && bytes[start - 1].is_ascii_digit() {
            start -= 1;
        }

        return prefix[start..=idx].parse().ok();
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
    use crate::host::{Host, HostProfile};

    let cwd = std::env::current_dir().unwrap_or_default();
    let local_agents_dir = cwd.join("agents");
    if local_agents_dir.exists() {
        let files = collect_agent_prompt_files(&local_agents_dir);
        if !files.is_empty() {
            return Some(files);
        }
    }

    // Check installed agents for all known hosts
    let hosts = [Host::OpenCode, Host::Copilot, Host::Pi];
    for host in &hosts {
        if let Some(profile) = HostProfile::for_host(host.clone()) {
            if let Some(agents_dir) = &profile.agents_dir {
                if agents_dir.exists() {
                    let files = collect_agent_prompt_files(agents_dir);
                    if !files.is_empty() {
                        return Some(files);
                    }
                }
            }
        }
    }

    None
}

fn collect_agent_prompt_files(dir: &Path) -> BTreeMap<String, PathBuf> {
    let mut files = BTreeMap::new();
    if let Ok(entries) = std::fs::read_dir(dir) {
        for entry in entries.filter_map(|entry| entry.ok()) {
            let path = entry.path();
            let name = match path.file_name().and_then(|n| n.to_str()) {
                Some(n) => n.to_string(),
                None => continue,
            };
            // Accept both .md and .agent.md files
            if !name.ends_with(".md") {
                continue;
            }
            // Derive a clean stem: strip .agent.md or .md
            let stem = if name.ends_with(".agent.md") {
                name.trim_end_matches(".agent.md").to_string()
            } else {
                name.trim_end_matches(".md").to_string()
            };
            if !stem.is_empty() {
                files.insert(stem, path);
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
        while end < chars.len()
            && (chars[end].is_ascii_alphanumeric() || chars[end] == '-' || chars[end] == '_')
        {
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

    // Fix 3: Install agents if missing (OpenCode by default)
    let agents_dir = crate::host::HostProfile::for_host(crate::host::Host::OpenCode)
        .and_then(|p| p.agents_dir)
        .unwrap_or_else(|| {
            crate::cli::util::opencode_config_dir()
                .unwrap_or_default()
                .join("agents")
        });
    let agents_missing = !agents_dir.exists()
        || std::fs::read_dir(&agents_dir)
            .map(|mut d| d.next().is_none())
            .unwrap_or(true);
    if agents_missing {
        match crate::skills_mgr::install_bundled_agents(&agents_dir, "md") {
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
                    "MCP project config".to_string(),
                    "Created opencode.json with spex-state MCP entry".to_string(),
                )),
                Err(e) => results.push((
                    "MCP project config".to_string(),
                    format!("Could not create opencode.json: {}", e),
                )),
            },
            Err(e) => results.push((
                "MCP project config".to_string(),
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
    use crate::sdd::event::emit_event;
    use std::path::PathBuf;

    async fn insert_consistent_completed_spec_fixture(pool: &SqlitePool) {
        sqlx::query(
            "INSERT INTO specs (id, title, status, priority, depends_on, agents, ac_total, ac_passed, created_at, updated_at, updated_by) VALUES (?, ?, 'done', 'P0', '[]', '[]', 1, 1, '2026-01-01T00:00:00Z', '2026-01-01T00:00:00Z', 'tester')",
        )
        .bind("SPEC-OK")
        .bind("Healthy spec")
        .execute(pool)
        .await
        .unwrap();

        sqlx::query(
            "INSERT INTO tasks (id, spec, title, agent, status, inputs, output_artifact, created_at, updated_at) VALUES (?, ?, ?, ?, 'done', '[]', ?, '2026-01-01T00:00:00Z', '2026-01-01T00:00:00Z')",
        )
        .bind("TASK-OK")
        .bind("SPEC-OK")
        .bind("Healthy task")
        .bind("sdd-builder")
        .bind("src/doctor/mod.rs")
        .execute(pool)
        .await
        .unwrap();

        emit_event(pool, "SpecStarted", Some("SPEC-OK"), Some("tester"), "{}")
            .await
            .unwrap();
        emit_event(
            pool,
            "TaskStarted",
            Some("SPEC-OK"),
            Some("sdd-builder"),
            r#"{"task":"TASK-OK"}"#,
        )
        .await
        .unwrap();
        emit_event(
            pool,
            "TaskCompleted",
            Some("SPEC-OK"),
            Some("sdd-builder"),
            r#"{"task":"TASK-OK"}"#,
        )
        .await
        .unwrap();
        emit_event(pool, "SpecCompleted", Some("SPEC-OK"), Some("tester"), "{}")
            .await
            .unwrap();
    }

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
    fn extract_task_id_from_payload_supports_legacy_task_id_key() {
        assert_eq!(
            extract_task_id_from_payload(r#"{"task_id":"T034"}"#),
            Some("T034".to_string())
        );
    }

    #[test]
    fn extracts_first_ascii_number_from_line() {
        assert_eq!(
            first_ascii_number("12 bundled agent markdown files"),
            Some(12)
        );
        assert_eq!(first_ascii_number("No count here"), None);
    }

    #[test]
    fn extract_count_for_phrase_reads_matching_line() {
        let content = "foo\n23 canonical tools covering things\nbar\n";
        assert_eq!(
            extract_count_for_phrase(content, "canonical tools"),
            Some(23)
        );
        assert_eq!(extract_count_for_phrase(content, "health checks"), None);
    }

    #[test]
    fn extract_count_for_phrase_prefers_number_nearest_phrase() {
        let content = "3. One binary embeds 13 bundled agent markdown files\nJSON-RPC 2.0 over stdio. 38 canonical tools are exposed\n";

        assert_eq!(
            extract_count_for_phrase(content, "bundled agent markdown files"),
            Some(13)
        );
        assert_eq!(
            extract_count_for_phrase(content, "canonical tools"),
            Some(38)
        );
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

    #[tokio::test]
    async fn control_plane_invariants_pass_for_consistent_completed_fixture() {
        let pool = crate::sdd::test_helpers::make_pool().await;
        insert_consistent_completed_spec_fixture(&pool).await;

        let result = evaluate_control_plane_invariants(&pool).await.unwrap();

        assert!(matches!(result.status, CheckStatus::Pass));
        assert!(result
            .message
            .contains("Validated 1 specs, 1 tasks, and 4 events"));
    }

    #[tokio::test]
    async fn control_plane_invariants_fail_for_done_spec_with_unfinished_task() {
        let pool = crate::sdd::test_helpers::make_pool().await;
        sqlx::query(
            "INSERT INTO specs (id, title, status, priority, depends_on, agents, ac_total, ac_passed, created_at, updated_at, updated_by) VALUES ('SPEC-DRIFT', 'Drifted spec', 'done', 'P0', '[]', '[]', 1, 1, '2026-01-01T00:00:00Z', '2026-01-01T00:00:00Z', 'tester')",
        )
        .execute(&pool)
        .await
        .unwrap();
        sqlx::query(
            "INSERT INTO tasks (id, spec, title, agent, status, inputs, output_artifact, created_at, updated_at) VALUES ('TASK-PENDING', 'SPEC-DRIFT', 'Pending task', 'sdd-builder', 'pending', '[]', NULL, '2026-01-01T00:00:00Z', '2026-01-01T00:00:00Z')",
        )
        .execute(&pool)
        .await
        .unwrap();
        emit_event(
            &pool,
            "SpecStarted",
            Some("SPEC-DRIFT"),
            Some("tester"),
            "{}",
        )
        .await
        .unwrap();
        emit_event(
            &pool,
            "SpecCompleted",
            Some("SPEC-DRIFT"),
            Some("tester"),
            "{}",
        )
        .await
        .unwrap();

        let result = evaluate_control_plane_invariants(&pool).await.unwrap();

        assert!(matches!(result.status, CheckStatus::Fail));
        assert!(result.message.contains("done specs with unfinished tasks"));
        assert!(result
            .message
            .contains("SPEC-DRIFT=[TASK-PENDING(pending)]"));
    }

    #[tokio::test]
    async fn control_plane_invariants_fail_for_missing_events_and_orphaned_refs() {
        let pool = crate::sdd::test_helpers::make_pool().await;
        sqlx::query(
            "INSERT INTO specs (id, title, status, priority, depends_on, agents, ac_total, ac_passed, created_at, updated_at, updated_by) VALUES ('SPEC-EVENT', 'Event drift', 'in_progress', 'P0', '[\"SPEC-MISSING\"]', '[]', 0, 0, '2026-01-01T00:00:00Z', '2026-01-01T00:00:00Z', 'tester')",
        )
        .execute(&pool)
        .await
        .unwrap();
        sqlx::query(
            "INSERT INTO tasks (id, spec, title, agent, status, inputs, output_artifact, created_at, updated_at) VALUES ('TASK-EVENT', 'SPEC-EVENT', 'Event drift task', 'sdd-builder', 'done', '[\"TASK-GHOST\"]', NULL, '2026-01-01T00:00:00Z', '2026-01-01T00:00:00Z')",
        )
        .execute(&pool)
        .await
        .unwrap();
        emit_event(
            &pool,
            "TaskCompleted",
            Some("SPEC-WRONG"),
            Some("sdd-builder"),
            r#"{"task":"TASK-EVENT"}"#,
        )
        .await
        .unwrap();
        emit_event(
            &pool,
            "TaskStarted",
            Some("SPEC-EVENT"),
            Some("sdd-builder"),
            r#"{"task":"TASK-GHOST"}"#,
        )
        .await
        .unwrap();

        let result = evaluate_control_plane_invariants(&pool).await.unwrap();

        assert!(matches!(result.status, CheckStatus::Fail));
        assert!(result
            .message
            .contains("specs with missing depends_on references"));
        assert!(result.message.contains("SPEC-EVENT=[SPEC-MISSING]"));
        assert!(result
            .message
            .contains("tasks with missing input references"));
        assert!(result.message.contains("TASK-EVENT=[TASK-GHOST]"));
        assert!(result.message.contains("orphaned task lifecycle events"));
        assert!(result.message.contains("TASK-GHOST"));
        assert!(result
            .message
            .contains("task lifecycle event spec mismatches"));
        assert!(result.message.contains("SPEC-WRONG"));
        assert!(result.message.contains("specs missing SpecStarted events"));
        assert!(result.message.contains("tasks missing TaskStarted events"));
    }
}
