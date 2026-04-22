use anyhow::Result;
use serde_json::{json, Value};
use sqlx::SqlitePool;
use std::path::{Path, PathBuf};

use crate::sdd::{
    artifact::query_artifacts,
    event::query_events,
    memory::memory_stats,
    project_profile::{bootstrap_project_context, inspect_project_at_subpath},
    spec::list_specs,
    task::list_tasks,
};

use super::args::optional_str;

pub(super) async fn handle_snapshot(pool: &SqlitePool, args: Value) -> Result<Value> {
    let specs = list_specs(pool, None, None).await?;
    let tasks = list_tasks(pool, None, None, None).await?;
    let events = query_events(pool, None, None, None, Some(10), None, None, None).await?;
    let artifacts = query_artifacts(pool, None, None, None, None).await?;
    let project_dir = detect_project_dir();
    let config_source = detect_config_source(&project_dir);
    let project_root = Path::new(&project_dir);
    let project_context = bootstrap_project_context(pool, project_root).await?;

    let mut agents: Vec<String> = Vec::new();
    for spec in &specs {
        if let Ok(parsed) = serde_json::from_str::<Vec<String>>(&spec.agents) {
            for agent in parsed {
                if !agents.contains(&agent) {
                    agents.push(agent);
                }
            }
        }
    }
    for task in &tasks {
        if !agents.contains(&task.agent) {
            agents.push(task.agent.clone());
        }
    }

    let agent_param = optional_str(&args, "agent");
    let spec_param = optional_str(&args, "spec");
    let memory_summary = if let Some(agent) = agent_param {
        Some(memory_stats(pool, agent, spec_param).await?)
    } else {
        None
    };
    let subprojects_summary = summarize_subprojects(
        project_context
            .project_profile
            .get("subprojects")
            .and_then(Value::as_array),
    );

    let mut payload = json!({
        "specs": specs,
        "tasks": tasks,
        "recent_events": events,
        "artifacts": artifacts,
        "agents": agents,
        "project_dir": project_dir,
        "config_source": config_source,
        "active_project": project_context.active_project,
        "project_profile": project_context.project_profile,
        "subprojects_summary": subprojects_summary,
        "repo_map": project_context.repo_map,
        "validation_commands": project_context.validation_commands
    });

    if let Some(ms) = memory_summary {
        payload["memory_stats"] = ms;
    }

    if payload
        .get("config_source")
        .and_then(Value::as_str)
        .unwrap_or("unknown")
        == "global-opencode.json"
    {
        payload["isolation_warning"] = json!(
            "MCP is configured globally; run `spex mcp setup` in this project for isolation."
        );
    }

    Ok(payload)
}

pub(super) async fn handle_project_context(pool: &SqlitePool, args: Value) -> Result<Value> {
    let project_dir = detect_project_dir();
    let project_root = Path::new(&project_dir);
    let subpath = optional_str(&args, "subpath");
    let context = if let Some(subpath) = subpath {
        inspect_project_at_subpath(project_root, subpath)?
    } else {
        bootstrap_project_context(pool, project_root).await?
    };

    Ok(json!({
        "project_dir": project_dir,
        "subpath": subpath,
        "active_project": context.active_project,
        "project_profile": context.project_profile,
        "repo_map": context.repo_map,
        "validation_commands": context.validation_commands,
    }))
}

pub(super) async fn handle_prd_get(_pool: &SqlitePool, _args: Value) -> Result<Value> {
    let project_dir = detect_project_dir();
    let prd_path = Path::new(&project_dir).join("docs").join("PRD.md");
    let (content, exists) = if prd_path.exists() {
        (std::fs::read_to_string(&prd_path).unwrap_or_default(), true)
    } else {
        (String::new(), false)
    };
    let is_template = content.contains("<!-- What is this project?")
        || content.contains("<!-- Top 3 measurable goals")
        || content.contains("<!-- What is explicitly out of scope?")
        || !exists;

    Ok(json!({
        "content": content,
        "path": prd_path.display().to_string(),
        "exists": exists,
        "is_template": is_template
    }))
}

pub(super) async fn handle_prd_set(_pool: &SqlitePool, args: Value) -> Result<Value> {
    let content = args
        .get("content")
        .and_then(Value::as_str)
        .ok_or_else(|| anyhow::anyhow!("Missing field: content"))?;
    let project_dir = detect_project_dir();
    let docs_dir = Path::new(&project_dir).join("docs");
    std::fs::create_dir_all(&docs_dir)?;
    let prd_path = docs_dir.join("PRD.md");
    std::fs::write(&prd_path, content)?;

    Ok(json!({
        "ok": true,
        "path": prd_path.display().to_string(),
    }))
}

fn summarize_subprojects(subprojects: Option<&Vec<Value>>) -> Value {
    let Some(subprojects) = subprojects else {
        return json!({
            "count": 0,
            "items": []
        });
    };

    let items: Vec<Value> = subprojects
        .iter()
        .map(|subproject| {
            json!({
                "name": subproject.get("name").cloned().unwrap_or(Value::Null),
                "path": subproject.get("path").cloned().unwrap_or(Value::Null),
                "workspace_group": subproject.get("workspace_group").cloned().unwrap_or(Value::Null),
                "languages": subproject.get("languages").cloned().unwrap_or_else(|| json!([])),
                "frameworks": subproject.get("frameworks").cloned().unwrap_or_else(|| json!([])),
                "primary_validation": subproject
                    .get("validation_commands")
                    .and_then(|v| v.get("primary"))
                    .cloned()
                    .unwrap_or(Value::Null),
            })
        })
        .collect();

    json!({
        "count": items.len(),
        "items": items,
    })
}

fn detect_project_dir() -> String {
    std::env::current_dir()
        .ok()
        .and_then(|p| p.canonicalize().ok())
        .unwrap_or_else(|| PathBuf::from("."))
        .display()
        .to_string()
}

fn detect_config_source(project_dir: &str) -> &'static str {
    if std::env::var_os("OPENCODE_CONFIG").is_some() {
        return "env";
    }

    let local_path = Path::new(project_dir).join("opencode.json");
    if local_path.exists() {
        return "local-opencode.json";
    }

    if let Some(opencode_dir) = crate::cli::util::opencode_config_dir() {
        let global_path = opencode_dir.join("config.json");
        if global_path.exists() {
            return "global-opencode.json";
        }
    }

    "unknown"
}
