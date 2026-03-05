use anyhow::{anyhow, Result};
use colored::Colorize;
use serde_json::{json, Value};
use sqlx::SqlitePool;

use crate::mcp::server::run_mcp_server;
use crate::scaffold::mcp_entry_json;
use std::sync::Arc;

pub async fn cmd_mcp_serve(pool: SqlitePool) -> Result<()> {
    let pool = Arc::new(pool);
    run_mcp_server(pool).await
}

pub fn cmd_mcp_setup(global: bool) -> Result<()> {
    let path = if global {
        let config_dir =
            dirs::config_dir().ok_or_else(|| anyhow!("Could not find config directory"))?;
        let opencode_dir = config_dir.join("opencode");
        std::fs::create_dir_all(&opencode_dir)?;
        opencode_dir.join("config.json")
    } else {
        std::env::current_dir()?.join("opencode.json")
    };

    let existing = if path.exists() {
        let raw = std::fs::read_to_string(&path)?;
        serde_json::from_str::<Value>(&raw).unwrap_or_else(|_| json!({}))
    } else {
        json!({})
    };

    let (config, changed) = merge_mcp_entries(existing);
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

fn merge_mcp_entries(mut config: Value) -> (Value, bool) {
    let mut changed = false;

    if !config.is_object() {
        config = json!({});
        changed = true;
    }

    let root = config.as_object_mut().expect("config must be object");
    let mcp_exists = root.get("mcp").map(Value::is_object).unwrap_or(false);
    if !mcp_exists {
        root.insert("mcp".to_string(), json!({}));
        changed = true;
    }

    let mcp = root
        .get_mut("mcp")
        .and_then(Value::as_object_mut)
        .expect("mcp must be object");

    if !mcp.contains_key("spex-state") {
        mcp.insert("spex-state".to_string(), mcp_entry_json());
        changed = true;
    }

    (config, changed)
}
