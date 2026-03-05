use colored::ColoredString;
use colored::Colorize;

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
