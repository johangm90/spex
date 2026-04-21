use std::path::PathBuf;

/// Supported AI tool hosts that spex can integrate with.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Host {
    /// OpenCode — uses `~/.config/opencode/` for global config and agents.
    OpenCode,
    /// GitHub Copilot CLI — uses `~/.copilot/` for global config and agents.
    Copilot,
}

impl Host {
    /// Parse a host name string (case-insensitive).
    pub fn from_str(s: &str) -> Option<Self> {
        match s.to_lowercase().as_str() {
            "opencode" => Some(Host::OpenCode),
            "copilot" | "github-copilot" | "copilot-cli" => Some(Host::Copilot),
            _ => None,
        }
    }

    /// Canonical name used in CLI output and config keys.
    pub fn name(&self) -> &'static str {
        match self {
            Host::OpenCode => "opencode",
            Host::Copilot => "copilot",
        }
    }
}

/// Per-host path and format profile.
#[derive(Debug, Clone)]
pub struct HostProfile {
    pub host: Host,
    /// Global agents directory (where `.md` / `.agent.md` files are installed).
    pub agents_dir: PathBuf,
    /// File extension for agent files (without leading dot).
    pub agent_extension: &'static str,
    /// Global MCP config file path.
    pub mcp_config_path: PathBuf,
    /// JSON key under which MCP servers are registered in the config file.
    pub mcp_servers_key: &'static str,
    /// Whether the MCP command should be a JSON array (`["spex","mcp","serve"]`)
    /// or a plain string (`"spex"`) with a separate `args` array.
    pub mcp_command_is_array: bool,
}

impl HostProfile {
    /// Build the profile for a given host, rooted at the user's home directory.
    pub fn for_host(host: Host) -> Option<Self> {
        let home = dirs::home_dir()?;
        Some(match host {
            Host::OpenCode => HostProfile {
                host: Host::OpenCode,
                agents_dir: home.join(".config").join("opencode").join("agents"),
                agent_extension: "md",
                mcp_config_path: home.join(".config").join("opencode").join("config.json"),
                mcp_servers_key: "mcp",
                mcp_command_is_array: true,
            },
            Host::Copilot => HostProfile {
                host: Host::Copilot,
                agents_dir: home.join(".copilot").join("agents"),
                agent_extension: "agent.md",
                mcp_config_path: home.join(".copilot").join("mcp-config.json"),
                mcp_servers_key: "mcpServers",
                mcp_command_is_array: false,
            },
        })
    }
}

/// Detect which hosts appear to be installed on this machine.
/// A host is considered "installed" when its global config directory exists.
#[allow(dead_code)]
pub fn detect_installed_hosts() -> Vec<Host> {
    let Some(home) = dirs::home_dir() else {
        return vec![];
    };

    let mut found = Vec::new();

    if home.join(".config").join("opencode").exists() {
        found.push(Host::OpenCode);
    }

    if home.join(".copilot").exists() {
        found.push(Host::Copilot);
    }

    found
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn host_from_str_parses_known_values() {
        assert_eq!(Host::from_str("opencode"), Some(Host::OpenCode));
        assert_eq!(Host::from_str("OpenCode"), Some(Host::OpenCode));
        assert_eq!(Host::from_str("copilot"), Some(Host::Copilot));
        assert_eq!(Host::from_str("github-copilot"), Some(Host::Copilot));
        assert_eq!(Host::from_str("copilot-cli"), Some(Host::Copilot));
        assert_eq!(Host::from_str("unknown"), None);
    }

    #[test]
    fn host_profile_opencode_uses_correct_paths() {
        let profile = HostProfile::for_host(Host::OpenCode).expect("home dir must exist");
        let home = dirs::home_dir().unwrap();

        assert_eq!(
            profile.agents_dir,
            home.join(".config").join("opencode").join("agents")
        );
        assert_eq!(profile.agent_extension, "md");
        assert_eq!(
            profile.mcp_config_path,
            home.join(".config").join("opencode").join("config.json")
        );
        assert_eq!(profile.mcp_servers_key, "mcp");
        assert!(profile.mcp_command_is_array);
    }

    #[test]
    fn host_profile_copilot_uses_correct_paths() {
        let profile = HostProfile::for_host(Host::Copilot).expect("home dir must exist");
        let home = dirs::home_dir().unwrap();

        assert_eq!(profile.agents_dir, home.join(".copilot").join("agents"));
        assert_eq!(profile.agent_extension, "agent.md");
        assert_eq!(
            profile.mcp_config_path,
            home.join(".copilot").join("mcp-config.json")
        );
        assert_eq!(profile.mcp_servers_key, "mcpServers");
        assert!(!profile.mcp_command_is_array);
    }
}
