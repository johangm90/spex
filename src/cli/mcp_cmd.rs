use anyhow::{anyhow, Result};
use colored::Colorize;
use serde_json::{json, Value};
use sqlx::SqlitePool;

use crate::host::{Host, HostProfile};
use crate::mcp::server::run_mcp_server;
use crate::scaffold::mcp_entry_json;
use std::sync::Arc;

pub async fn cmd_mcp_serve(pool: SqlitePool) -> Result<()> {
    let pool = Arc::new(pool);
    run_mcp_server(pool).await
}

pub fn cmd_mcp_setup(global: bool, host: Option<&str>) -> Result<()> {
    let resolved_host = resolve_host(host)?;

    let path = if global {
        match &resolved_host {
            Some(profile) => {
                if let Some(parent) = profile.mcp_config_path.parent() {
                    std::fs::create_dir_all(parent)?;
                }
                profile.mcp_config_path.clone()
            }
            None => {
                // Default to OpenCode global config when no host specified
                let opencode_dir = crate::cli::util::opencode_config_dir()
                    .ok_or_else(|| anyhow!("Could not find home directory"))?;
                std::fs::create_dir_all(&opencode_dir)?;
                opencode_dir.join("config.json")
            }
        }
    } else {
        // Project-local path: .vscode/mcp.json for VSCode, opencode.json for others
        match &resolved_host {
            Some(profile) if profile.host == crate::host::Host::VSCode => {
                let vscode_dir = std::env::current_dir()?.join(".vscode");
                std::fs::create_dir_all(&vscode_dir)?;
                vscode_dir.join("mcp.json")
            }
            _ => std::env::current_dir()?.join("opencode.json"),
        }
    };

    let existing = if path.exists() {
        let raw = std::fs::read_to_string(&path)?;
        serde_json::from_str::<Value>(&raw).unwrap_or_else(|_| json!({}))
    } else {
        json!({})
    };

    let servers_key = resolved_host
        .as_ref()
        .map(|p| p.mcp_servers_key)
        .unwrap_or("mcp");
    let command_is_array = resolved_host
        .as_ref()
        .map(|p| p.mcp_command_is_array)
        .unwrap_or(true);

    let (config, changed) = merge_mcp_entries(existing, servers_key, command_is_array);
    std::fs::write(&path, serde_json::to_string_pretty(&config)?)?;

    if changed {
        println!("{} Updated {}", "✓".green(), path.display());
    } else {
        println!(
            "{} {} already contains MCP entries",
            "•".dimmed(),
            path.display()
        );
    }

    println!("  MCP server: {} mcp serve", "spex".cyan());
    Ok(())
}

/// Resolve an optional host name string to a `HostProfile`.
/// Returns `None` when no host is specified (caller uses defaults).
fn resolve_host(host: Option<&str>) -> Result<Option<HostProfile>> {
    match host {
        None => Ok(None),
        Some(s) => {
            let h = Host::from_str(s).ok_or_else(|| {
                anyhow!(
                    "Unknown host '{}'. Valid values: opencode, copilot, vscode",
                    s
                )
            })?;
            let profile = HostProfile::for_host(h)
                .ok_or_else(|| anyhow!("Could not determine home directory"))?;
            Ok(Some(profile))
        }
    }
}

fn merge_mcp_entries(
    mut config: Value,
    servers_key: &str,
    command_is_array: bool,
) -> (Value, bool) {
    let mut changed = false;

    if !config.is_object() {
        config = json!({});
        changed = true;
    }

    let root = config.as_object_mut().expect("config must be object");
    let section_exists = root.get(servers_key).map(Value::is_object).unwrap_or(false);
    if !section_exists {
        root.insert(servers_key.to_string(), json!({}));
        changed = true;
    }

    let section = root
        .get_mut(servers_key)
        .and_then(Value::as_object_mut)
        .expect("section must be object");

    if !section.contains_key("spex-state") {
        let entry = if command_is_array {
            // OpenCode format: "command": ["spex", "mcp", "serve"]
            mcp_entry_json()
        } else {
            // Copilot CLI format: "command": "spex", "args": ["mcp", "serve"]
            json!({
                "command": "spex",
                "args": ["mcp", "serve"]
            })
        };
        section.insert("spex-state".to_string(), entry);
        changed = true;
    }

    (config, changed)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn merge_mcp_entries_opencode_format_uses_array_command() {
        let config = json!({});
        let (result, changed) = merge_mcp_entries(config, "mcp", true);
        assert!(changed);
        let entry = &result["mcp"]["spex-state"];
        assert!(
            entry["command"].is_array(),
            "OpenCode format must use array command"
        );
    }

    #[test]
    fn merge_mcp_entries_copilot_format_uses_string_command_with_args() {
        let config = json!({});
        let (result, changed) = merge_mcp_entries(config, "mcpServers", false);
        assert!(changed);
        let entry = &result["mcpServers"]["spex-state"];
        assert_eq!(
            entry["command"].as_str(),
            Some("spex"),
            "Copilot format must use string command"
        );
        assert!(
            entry["args"].is_array(),
            "Copilot format must have args array"
        );
        let args: Vec<&str> = entry["args"]
            .as_array()
            .unwrap()
            .iter()
            .filter_map(|v| v.as_str())
            .collect();
        assert_eq!(args, vec!["mcp", "serve"]);
    }

    #[test]
    fn merge_mcp_entries_is_idempotent() {
        let config = json!({});
        let (config, _) = merge_mcp_entries(config, "mcp", true);
        let (_, changed) = merge_mcp_entries(config, "mcp", true);
        assert!(!changed, "second merge must report no changes");
    }

    #[test]
    fn resolve_host_returns_none_for_no_host() {
        let result = resolve_host(None).unwrap();
        assert!(result.is_none());
    }

    #[test]
    fn resolve_host_returns_profile_for_opencode() {
        let result = resolve_host(Some("opencode")).unwrap();
        assert!(result.is_some());
        assert_eq!(result.unwrap().mcp_servers_key, "mcp");
    }

    #[test]
    fn resolve_host_returns_profile_for_copilot() {
        let result = resolve_host(Some("copilot")).unwrap();
        assert!(result.is_some());
        assert_eq!(result.unwrap().mcp_servers_key, "mcpServers");
    }

    #[test]
    fn resolve_host_errors_on_unknown_host() {
        let result = resolve_host(Some("notepad"));
        assert!(result.is_err());
    }

    #[test]
    fn resolve_host_returns_profile_for_vscode() {
        let result = resolve_host(Some("vscode")).unwrap();
        assert!(result.is_some());
        let profile = result.unwrap();
        assert_eq!(profile.mcp_servers_key, "servers");
        assert!(!profile.mcp_command_is_array);
    }

    #[test]
    fn merge_mcp_entries_vscode_format_uses_servers_key() {
        let config = json!({});
        let (result, changed) = merge_mcp_entries(config, "servers", false);
        assert!(changed);
        let entry = &result["servers"]["spex-state"];
        assert_eq!(
            entry["command"].as_str(),
            Some("spex"),
            "VSCode format must use string command"
        );
        assert!(
            entry["args"].is_array(),
            "VSCode format must have args array"
        );
    }
}
