use anyhow::Result;
use sqlx::sqlite::{SqliteConnectOptions, SqlitePoolOptions};
use std::path::Path;

struct WorkspaceStatus {
    path: String,
    open_specs: i64,
    open_tasks: i64,
    last_activity: Option<String>,
}

pub async fn cmd_workspace_status(paths: &[String]) -> Result<()> {
    let mut rows: Vec<WorkspaceStatus> = Vec::new();

    for path_str in paths {
        let db_path = Path::new(path_str).join(".spex").join("state.db");

        if !db_path.exists() {
            eprintln!(
                "warning: no state.db found at {} — skipping",
                db_path.display()
            );
            continue;
        }

        let options = SqliteConnectOptions::new()
            .filename(&db_path)
            .read_only(true);

        let pool = match SqlitePoolOptions::new()
            .max_connections(1)
            .connect_with(options)
            .await
        {
            Ok(p) => p,
            Err(e) => {
                eprintln!(
                    "warning: could not open {} — {e} — skipping",
                    db_path.display()
                );
                continue;
            }
        };

        let open_specs: i64 =
            sqlx::query_scalar("SELECT COUNT(*) FROM specs WHERE status NOT IN ('done', 'paused')")
                .fetch_one(&pool)
                .await
                .unwrap_or(0);

        let open_tasks: i64 =
            sqlx::query_scalar("SELECT COUNT(*) FROM tasks WHERE status NOT IN ('done', 'failed')")
                .fetch_one(&pool)
                .await
                .unwrap_or(0);

        let last_activity: Option<String> = sqlx::query_scalar("SELECT MAX(timestamp) FROM events")
            .fetch_one(&pool)
            .await
            .unwrap_or(None);

        // Format timestamp: keep only "YYYY-MM-DD HH:MM" portion
        let last_activity = last_activity.map(|ts| {
            // timestamps are ISO-8601; trim to minute precision
            ts.chars().take(16).collect::<String>().replace('T', " ")
        });

        rows.push(WorkspaceStatus {
            path: path_str.clone(),
            open_specs,
            open_tasks,
            last_activity,
        });

        pool.close().await;
    }

    // ── Print table ──────────────────────────────────────────────────────────
    let path_col = 32usize;
    let specs_col = 11usize;
    let tasks_col = 11usize;
    let activity_col = 16usize;

    println!(
        "{:<path_col$}  {:<specs_col$}  {:<tasks_col$}  Last Activity",
        "Project", "Open Specs", "Open Tasks"
    );
    println!(
        "{}",
        "─".repeat(path_col + specs_col + tasks_col + activity_col + 6)
    );

    for row in &rows {
        let activity = row.last_activity.as_deref().unwrap_or("—");
        println!(
            "{:<path_col$}  {:<specs_col$}  {:<tasks_col$}  {}",
            row.path, row.open_specs, row.open_tasks, activity
        );
    }

    Ok(())
}
