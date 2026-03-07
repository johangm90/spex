use serde_json::{json, Value};
use std::path::PathBuf;

/// Supported AI coding tool targets.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ToolTarget {
    OpenCode,
    CopilotCli,
}

impl ToolTarget {
    /// Human-readable name for CLI output.
    pub fn display_name(&self) -> &'static str {
        match self {
            ToolTarget::OpenCode => "OpenCode",
            ToolTarget::CopilotCli => "GitHub Copilot CLI",
        }
    }

    /// Root config directory for this tool (~/.config/opencode or ~/.copilot).
    pub fn config_dir(&self) -> Option<PathBuf> {
        match self {
            ToolTarget::OpenCode => crate::cli::util::opencode_config_dir(),
            ToolTarget::CopilotCli => crate::cli::util::copilot_config_dir(),
        }
    }

    /// Directory where skills (SKILL.md) are installed.
    pub fn skills_dir(&self) -> Option<PathBuf> {
        self.config_dir().map(|d| d.join("skills"))
    }

    /// Directory where agent files are installed.
    pub fn agents_dir(&self) -> Option<PathBuf> {
        match self {
            // OpenCode: ~/.config/opencode/agents/<name>.md
            ToolTarget::OpenCode => self.config_dir().map(|d| d.join("agents")),
            // CopilotCli: ~/.copilot/agents/<name>.agent.md
            ToolTarget::CopilotCli => self.config_dir().map(|d| d.join("agents")),
        }
    }

    /// Path to the global MCP config file for this tool.
    pub fn global_mcp_config_path(&self) -> Option<PathBuf> {
        match self {
            ToolTarget::OpenCode => self.config_dir().map(|d| d.join("config.json")),
            ToolTarget::CopilotCli => self.config_dir().map(|d| d.join("mcp-config.json")),
        }
    }

    /// Path to a per-project MCP config file (OpenCode only: ./opencode.json).
    /// Returns None for CopilotCli (no per-project config concept).
    pub fn local_mcp_config_path(&self) -> Option<PathBuf> {
        match self {
            ToolTarget::OpenCode => std::env::current_dir()
                .ok()
                .map(|d| d.join("opencode.json")),
            ToolTarget::CopilotCli => None,
        }
    }

    /// The JSON value to insert as the spex-state MCP server entry.
    pub fn mcp_entry_json(&self) -> Value {
        match self {
            ToolTarget::OpenCode => json!({
                "type": "local",
                "enabled": true,
                "command": ["spex", "mcp", "serve"]
            }),
            ToolTarget::CopilotCli => json!({
                "type": "local",
                "command": "spex",
                "args": ["mcp", "serve"],
                "env": {},
                "tools": ["*"]
            }),
        }
    }

    /// Merge the spex-state entry into an existing config JSON (loaded from the config file).
    /// Returns (updated_config, changed: bool).
    pub fn merge_mcp_config(&self, existing: Value) -> (Value, bool) {
        let mut config = if existing.is_object() {
            existing
        } else {
            json!({})
        };

        let mut changed = false;

        // Determine the key path for each tool:
        // OpenCode: config["mcp"]["spex-state"]
        // CopilotCli: config["mcpServers"]["spex-state"]
        let top_key = match self {
            ToolTarget::OpenCode => "mcp",
            ToolTarget::CopilotCli => "mcpServers",
        };

        let root = config.as_object_mut().expect("config must be object");

        // Ensure top-level key exists and is an object
        let top_exists = root.get(top_key).map(Value::is_object).unwrap_or(false);
        if !top_exists {
            root.insert(top_key.to_string(), json!({}));
            changed = true;
        }

        let servers = root
            .get_mut(top_key)
            .and_then(Value::as_object_mut)
            .expect("top-level MCP key must be an object");

        if !servers.contains_key("spex-state") {
            servers.insert("spex-state".to_string(), self.mcp_entry_json());
            changed = true;
        }

        (config, changed)
    }

    /// Detect which tools appear to be installed on this machine by checking if
    /// their config directories exist under the home directory.
    pub fn detect_installed() -> Vec<ToolTarget> {
        let candidates = [ToolTarget::OpenCode, ToolTarget::CopilotCli];
        candidates
            .into_iter()
            .filter(|t| t.config_dir().map(|d| d.exists()).unwrap_or(false))
            .collect()
    }
}
