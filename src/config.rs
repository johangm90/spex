use std::path::Path;

use anyhow::{Context, Result};
use serde::Deserialize;

use crate::sdd::db::find_project_root;

// ─── Public types ─────────────────────────────────────────────────────────────

/// Top-level configuration loaded from `.spex/config.toml`.
#[derive(Debug, Default)]
pub struct SpexConfig {
    pub webhooks: Option<WebhookConfig>,
    pub tickets: Option<TicketsConfig>,
}

/// Configuration for the pluggable ticket backend (`spex task export`).
#[derive(Debug, Clone)]
pub struct TicketsConfig {
    /// `spex-state` (default), `github`, or `markdown`.
    pub backend: String,
    /// `owner/name` for the GitHub backend; `gh` infers from the cwd remote when absent.
    pub github_repo: Option<String>,
    /// Output directory for the Markdown backend (default `.spex/tasks`).
    pub markdown_dir: Option<String>,
}

/// Configuration for the outbound webhook integration.
#[derive(Debug)]
pub struct WebhookConfig {
    /// Destination URL that receives POST requests.
    pub url: String,
    /// Event types that trigger the webhook. Empty vec means "all events".
    pub events: Vec<String>,
    /// HTTP request timeout in seconds (default: 5).
    pub timeout_secs: u64,
}

// ─── Private TOML-deserialization types ──────────────────────────────────────

#[derive(Deserialize, Default)]
struct RawConfig {
    webhooks: Option<RawWebhookConfig>,
    tickets: Option<RawTicketsConfig>,
}

#[derive(Deserialize)]
struct RawWebhookConfig {
    url: String,
    events: Option<Vec<String>>,
    timeout_secs: Option<u64>,
}

#[derive(Deserialize)]
struct RawTicketsConfig {
    backend: Option<String>,
    github_repo: Option<String>,
    markdown_dir: Option<String>,
}

// ─── Default event list ───────────────────────────────────────────────────────

fn default_events() -> Vec<String> {
    vec![
        "TaskDone".to_string(),
        "SpecApproved".to_string(),
        "SpecDone".to_string(),
        "ApprovalRequested".to_string(),
    ]
}

// ─── SpexConfig::load ─────────────────────────────────────────────────────────

impl SpexConfig {
    /// Load from `.spex/config.toml` relative to `project_root`.
    ///
    /// Returns `Ok(SpexConfig { webhooks: None })` when the file does not exist.
    /// Returns `Err` with a descriptive message when the file exists but is malformed.
    pub fn load(project_root: &Path) -> Result<Self> {
        let config_path = project_root.join(".spex").join("config.toml");

        if !config_path.exists() {
            return Ok(SpexConfig::default());
        }

        let raw_text = std::fs::read_to_string(&config_path)
            .with_context(|| format!("Failed to read {}", config_path.display()))?;

        let raw: RawConfig = toml::from_str(&raw_text)
            .with_context(|| format!("Malformed config file: {}", config_path.display()))?;

        let webhooks = raw.webhooks.map(|w| WebhookConfig {
            url: w.url,
            events: w.events.unwrap_or_else(default_events),
            timeout_secs: w.timeout_secs.unwrap_or(5),
        });

        let tickets = raw.tickets.map(|t| TicketsConfig {
            backend: t.backend.unwrap_or_else(|| "spex-state".to_string()),
            github_repo: t.github_repo,
            markdown_dir: t.markdown_dir,
        });

        Ok(SpexConfig { webhooks, tickets })
    }
}

pub fn load_config() -> Result<SpexConfig> {
    let project_root = find_project_root()?;
    SpexConfig::load(&project_root)
}

// ─── Tests ────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use tempfile::TempDir;

    fn write_config(dir: &TempDir, content: &str) {
        let spex_dir = dir.path().join(".spex");
        fs::create_dir_all(&spex_dir).unwrap();
        fs::write(spex_dir.join("config.toml"), content).unwrap();
    }

    #[test]
    fn no_config_file_returns_default() {
        let dir = TempDir::new().unwrap();
        let cfg = SpexConfig::load(dir.path()).unwrap();
        assert!(cfg.webhooks.is_none());
    }

    #[test]
    fn full_webhook_config_parsed() {
        let dir = TempDir::new().unwrap();
        write_config(
            &dir,
            r#"
[webhooks]
url = "https://hooks.example.com/spex"
events = ["TaskDone", "SpecApproved"]
timeout_secs = 10
"#,
        );
        let cfg = SpexConfig::load(dir.path()).unwrap();
        let wh = cfg.webhooks.expect("webhooks should be present");
        assert_eq!(wh.url, "https://hooks.example.com/spex");
        assert_eq!(wh.events, vec!["TaskDone", "SpecApproved"]);
        assert_eq!(wh.timeout_secs, 10);
    }

    #[test]
    fn webhook_defaults_applied_when_optional_fields_absent() {
        let dir = TempDir::new().unwrap();
        write_config(
            &dir,
            r#"
[webhooks]
url = "https://hooks.example.com/spex"
"#,
        );
        let cfg = SpexConfig::load(dir.path()).unwrap();
        let wh = cfg.webhooks.expect("webhooks should be present");
        assert_eq!(wh.timeout_secs, 5);
        assert_eq!(wh.events, default_events());
    }

    #[test]
    fn malformed_toml_returns_err() {
        let dir = TempDir::new().unwrap();
        write_config(&dir, "this is not valid toml ][");
        let result = SpexConfig::load(dir.path());
        assert!(result.is_err());
        let msg = format!("{:#}", result.unwrap_err());
        assert!(msg.contains("Malformed config file"));
    }
}
