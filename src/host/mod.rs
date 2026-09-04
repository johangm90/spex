use std::path::PathBuf;

/// Supported AI tool hosts that spex can integrate with.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Host {
    /// OpenCode — uses `~/.config/opencode/` for global config and agents.
    OpenCode,
    /// GitHub Copilot CLI — uses `~/.copilot/` for global config and agents.
    Copilot,
    /// VS Code — uses a platform-specific `mcp.json` for global config; no per-agent files.
    VSCode,
    /// Pi / pi-subagents — uses `~/.pi/agent/agents/` for user-scoped agent files.
    Pi,
}

impl Host {
    /// Parse a host name string (case-insensitive).
    pub fn from_str(s: &str) -> Option<Self> {
        match s.to_lowercase().as_str() {
            "opencode" => Some(Host::OpenCode),
            "copilot" | "github-copilot" | "copilot-cli" => Some(Host::Copilot),
            "vscode" | "vs-code" | "code" => Some(Host::VSCode),
            "pi" | "pi-subagents" => Some(Host::Pi),
            _ => None,
        }
    }

    /// Canonical name used in CLI output and config keys.
    pub fn name(&self) -> &'static str {
        match self {
            Host::OpenCode => "opencode",
            Host::Copilot => "copilot",
            Host::VSCode => "vscode",
            Host::Pi => "pi",
        }
    }
}

/// Per-host path and format profile.
#[derive(Debug, Clone)]
pub struct HostProfile {
    pub host: Host,
    /// Global agents directory (where `.md` / `.agent.md` files are installed).
    /// `None` for hosts that do not use per-agent files (e.g. VS Code).
    pub agents_dir: Option<PathBuf>,
    /// File extension for agent files (without leading dot).
    /// `None` for hosts that do not use per-agent files.
    pub agent_extension: Option<&'static str>,
    /// Global MCP config file path.
    pub mcp_config_path: Option<PathBuf>,
    /// JSON key under which MCP servers are registered in the config file.
    pub mcp_servers_key: &'static str,
    /// Whether the MCP command should be a JSON array (`["spex","mcp","serve"]`)
    /// or a plain string (`"spex"`) with a separate `args` array.
    pub mcp_command_is_array: bool,
    /// Whether this host supports spex MCP config installation.
    pub supports_mcp: bool,
}

impl HostProfile {
    /// Build the profile for a given host, rooted at the user's home directory.
    pub fn for_host(host: Host) -> Option<Self> {
        let home = dirs::home_dir()?;
        Some(match host {
            Host::OpenCode => HostProfile {
                host: Host::OpenCode,
                agents_dir: Some(home.join(".config").join("opencode").join("agents")),
                agent_extension: Some("md"),
                mcp_config_path: Some(home.join(".config").join("opencode").join("opencode.json")),
                mcp_servers_key: "mcp",
                mcp_command_is_array: true,
                supports_mcp: true,
            },
            Host::Copilot => HostProfile {
                host: Host::Copilot,
                agents_dir: Some(home.join(".copilot").join("agents")),
                agent_extension: Some("agent.md"),
                mcp_config_path: Some(home.join(".copilot").join("mcp-config.json")),
                mcp_servers_key: "mcpServers",
                mcp_command_is_array: false,
                supports_mcp: true,
            },
            Host::VSCode => {
                let mcp_config_path = vscode_user_mcp_path(&home);
                HostProfile {
                    host: Host::VSCode,
                    agents_dir: None,
                    agent_extension: None,
                    mcp_config_path: Some(mcp_config_path),
                    mcp_servers_key: "servers",
                    mcp_command_is_array: false,
                    supports_mcp: true,
                }
            }
            Host::Pi => HostProfile {
                host: Host::Pi,
                agents_dir: Some(home.join(".pi").join("agent").join("agents")),
                agent_extension: Some("md"),
                mcp_config_path: None,
                mcp_servers_key: "",
                mcp_command_is_array: false,
                supports_mcp: false,
            },
        })
    }
}

/// Returns the platform-specific VS Code user MCP config path.
fn vscode_user_mcp_path(home: &std::path::Path) -> PathBuf {
    #[cfg(target_os = "macos")]
    {
        home.join("Library")
            .join("Application Support")
            .join("Code")
            .join("User")
            .join("mcp.json")
    }
    #[cfg(target_os = "windows")]
    {
        // %APPDATA%\Code\User\mcp.json
        std::env::var("APPDATA")
            .map(|p| PathBuf::from(p).join("Code").join("User").join("mcp.json"))
            .unwrap_or_else(|_| {
                home.join("AppData")
                    .join("Roaming")
                    .join("Code")
                    .join("User")
                    .join("mcp.json")
            })
    }
    #[cfg(not(any(target_os = "macos", target_os = "windows")))]
    {
        home.join(".config")
            .join("Code")
            .join("User")
            .join("mcp.json")
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

    if home.join(".pi").join("agent").exists() {
        found.push(Host::Pi);
    }

    // VS Code: check for the user data directory
    let vscode_exists = {
        #[cfg(target_os = "macos")]
        {
            home.join("Library")
                .join("Application Support")
                .join("Code")
                .exists()
        }
        #[cfg(target_os = "windows")]
        {
            std::env::var("APPDATA")
                .map(|p| PathBuf::from(p).join("Code").exists())
                .unwrap_or(false)
        }
        #[cfg(not(any(target_os = "macos", target_os = "windows")))]
        {
            home.join(".config").join("Code").exists()
        }
    };
    if vscode_exists {
        found.push(Host::VSCode);
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
        assert_eq!(Host::from_str("vscode"), Some(Host::VSCode));
        assert_eq!(Host::from_str("vs-code"), Some(Host::VSCode));
        assert_eq!(Host::from_str("code"), Some(Host::VSCode));
        assert_eq!(Host::from_str("pi"), Some(Host::Pi));
        assert_eq!(Host::from_str("pi-subagents"), Some(Host::Pi));
        assert_eq!(Host::from_str("unknown"), None);
    }

    #[test]
    fn host_profile_opencode_uses_correct_paths() {
        let profile = HostProfile::for_host(Host::OpenCode).expect("home dir must exist");
        let home = dirs::home_dir().unwrap();

        assert_eq!(
            profile.agents_dir,
            Some(home.join(".config").join("opencode").join("agents"))
        );
        assert_eq!(profile.agent_extension, Some("md"));
        assert_eq!(
            profile.mcp_config_path,
            Some(home.join(".config").join("opencode").join("opencode.json"))
        );
        assert_eq!(profile.mcp_servers_key, "mcp");
        assert!(profile.mcp_command_is_array);
        assert!(profile.supports_mcp);
    }

    #[test]
    fn host_profile_copilot_uses_correct_paths() {
        let profile = HostProfile::for_host(Host::Copilot).expect("home dir must exist");
        let home = dirs::home_dir().unwrap();

        assert_eq!(
            profile.agents_dir,
            Some(home.join(".copilot").join("agents"))
        );
        assert_eq!(profile.agent_extension, Some("agent.md"));
        assert_eq!(
            profile.mcp_config_path,
            Some(home.join(".copilot").join("mcp-config.json"))
        );
        assert_eq!(profile.mcp_servers_key, "mcpServers");
        assert!(!profile.mcp_command_is_array);
        assert!(profile.supports_mcp);
    }

    #[test]
    fn host_profile_vscode_has_no_agents_dir() {
        let profile = HostProfile::for_host(Host::VSCode).expect("home dir must exist");
        assert!(profile.agents_dir.is_none());
        assert!(profile.agent_extension.is_none());
        assert_eq!(profile.mcp_servers_key, "servers");
        assert!(!profile.mcp_command_is_array);
        assert!(profile.supports_mcp);
    }

    #[test]
    fn host_profile_pi_uses_correct_paths() {
        let profile = HostProfile::for_host(Host::Pi).expect("home dir must exist");
        let home = dirs::home_dir().unwrap();

        assert_eq!(
            profile.agents_dir,
            Some(home.join(".pi").join("agent").join("agents"))
        );
        assert_eq!(profile.agent_extension, Some("md"));
        assert!(profile.mcp_config_path.is_none());
        assert!(!profile.supports_mcp);
    }
}
