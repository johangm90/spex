use colored::ColoredString;
use colored::Colorize;
use std::path::PathBuf;

/// Returns the OpenCode config directory: ~/.config/opencode
/// Always uses ~/.config (XDG convention) regardless of platform,
/// because OpenCode hard-codes this path on all platforms including macOS.
pub fn opencode_config_dir() -> Option<PathBuf> {
    dirs::home_dir().map(|h| h.join(".config").join("opencode"))
}

pub fn colorize_status(status: &str) -> ColoredString {
    match status {
        "draft" => status.white(),
        "approved" => status.yellow(),
        "in_progress" => status.blue(),
        "done" => status.green(),
        "failed" => status.red(),
        "paused" => status.dimmed(),
        _ => status.normal(),
    }
}
