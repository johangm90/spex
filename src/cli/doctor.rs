use anyhow::Result;
use colored::Colorize;

use crate::doctor::{fix_issues, run_checks, CheckStatus};

pub async fn cmd_doctor(fix: bool) -> Result<()> {
    println!("{}", "╔══════════════════════════════════════════╗".cyan());
    println!("{}", "║            spex — doctor                 ║".cyan());
    println!("{}", "╚══════════════════════════════════════════╝".cyan());
    println!();

    let results = run_checks().await;

    let mut any_issue = false;
    for result in &results {
        let icon = match result.status {
            CheckStatus::Pass => "✓".green().to_string(),
            CheckStatus::Warn => {
                any_issue = true;
                "⚠".yellow().to_string()
            }
            CheckStatus::Fail => {
                any_issue = true;
                "✗".red().to_string()
            }
        };
        println!("  {} {}: {}", icon, result.name.bold(), result.message);
    }

    println!();

    if fix && any_issue {
        println!("{}", "── Auto-fix ────────────────────────────────".cyan());
        println!();
        let fixes = fix_issues().await;
        if fixes.is_empty() {
            println!("  {}", "Nothing to fix automatically.".dimmed());
        } else {
            for (name, msg) in &fixes {
                println!("  {} {}: {}", "→".cyan(), name.bold(), msg);
            }
        }
        println!();

        // Re-run checks to show updated state
        println!("{}", "── Re-checking ─────────────────────────────".cyan());
        println!();
        let results2 = run_checks().await;
        let mut still_fail = false;
        for result in &results2 {
            let icon = match result.status {
                CheckStatus::Pass => "✓".green().to_string(),
                CheckStatus::Warn => "⚠".yellow().to_string(),
                CheckStatus::Fail => {
                    still_fail = true;
                    "✗".red().to_string()
                }
            };
            println!("  {} {}: {}", icon, result.name.bold(), result.message);
        }
        println!();
        if still_fail {
            println!(
                "{}",
                "Some checks still failing — manual action required.".red()
            );
            std::process::exit(1);
        } else {
            println!("{}", "All checks passed after auto-fix!".green().bold());
        }
    } else if any_issue {
        let fail_count = results
            .iter()
            .filter(|r| matches!(r.status, CheckStatus::Fail))
            .count();
        if fail_count > 0 {
            println!(
                "{}",
                "Some checks failed. Fix issues and re-run `spex doctor`.".red()
            );
            println!(
                "  {}",
                "Tip: run `spex doctor --fix` to attempt automatic fixes.".dimmed()
            );
            std::process::exit(1);
        } else {
            println!(
                "{}",
                "Some warnings found. Run `spex doctor --fix` to attempt automatic fixes.".yellow()
            );
        }
    } else {
        println!("{}", "All checks passed!".green().bold());
    }

    Ok(())
}
