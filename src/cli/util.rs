use colored::ColoredString;
use colored::Colorize;
use std::path::PathBuf;

/// Returns the OpenCode config directory: ~/.config/opencode
/// Always uses ~/.config (XDG convention) regardless of platform,
/// because OpenCode hard-codes this path on all platforms including macOS.
pub fn opencode_config_dir() -> Option<PathBuf> {
    dirs::home_dir().map(|h| h.join(".config").join("opencode"))
}

/// Returns ~/.copilot (GitHub Copilot CLI config directory).
#[allow(dead_code)]
pub fn copilot_config_dir() -> Option<PathBuf> {
    dirs::home_dir().map(|h| h.join(".copilot"))
}

pub fn colorize_status(status: &str) -> ColoredString {
    match status {
        "draft" => status.white(),
        "approved" => status.yellow(),
        "in_progress" => status.blue(),
        "claimed" => status.cyan(),
        "running" => status.blue(),
        "awaiting_review" => status.cyan(),
        "verifying" => status.yellow(),
        "done" => status.green(),
        "pass" => status.green(),
        "pass_with_risk" => status.yellow(),
        "blocked" | "failed" | "critical" => status.red(),
        "paused" | "cancelled" | "discarded" | "superseded" => status.dimmed(),
        "stabilizing" | "triaged" | "fix_planned" | "fix_in_progress" => status.yellow(),
        "resolved" => status.green(),
        _ => status.normal(),
    }
}
