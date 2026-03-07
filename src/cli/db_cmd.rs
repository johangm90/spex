/// CLI command: `spex db migrate-to-global`
///
/// Reads an existing per-project `.spex/state.db` (old schema without
/// `project_dir`) and copies all rows into the global DB at
/// `~/.local/share/spex/global-state.db`, tagging each row with the
/// canonicalized project path.
use anyhow::{anyhow, Result};
use colored::Colorize;
use sqlx::sqlite::{SqliteConnectOptions, SqlitePoolOptions};

use crate::sdd::db::{get_db_path, open_global_db};

/// The ordered list of tables to migrate (skipping `meta`, `constitution`,
/// and `memory_fts` which is rebuilt automatically by triggers).
const TABLES: &[&str] = &[
    "specs",
    "tasks",
    "events",
    "memory",
    "artifacts",
    "incidents",
    "context_gaps",
    "verification_runs",
    "interrupts",
    "handoff_snapshots",
    "plan_versions",
    "task_leases",
    "task_locks",
    "replan_requests",
];

/// Find the project root walking upward from `start`, returning the first
/// directory that contains a `.spex/` subdirectory.
fn find_project_root(start: &std::path::Path) -> Option<std::path::PathBuf> {
    let mut current = start.to_path_buf();
    loop {
        if current.join(".spex").is_dir() {
            return Some(current);
        }
        match current.parent() {
            Some(parent) => current = parent.to_path_buf(),
            None => return None,
        }
    }
}

/// Migrate a per-project `.spex/state.db` into the global DB.
///
/// # Arguments
/// * `project_dir_arg` – optional explicit project directory; defaults to `.`
/// * `dry_run` – when true, print row counts but make no writes
#[allow(dead_code)]
pub async fn cmd_db_migrate_to_global(
    project_dir_arg: Option<&str>,
    dry_run: bool,
) -> Result<()> {
    // ── 1. Resolve the project directory ──────────────────────────────────
    let raw_dir = project_dir_arg.unwrap_or(".");
    let project_dir = std::fs::canonicalize(raw_dir).map_err(|e| {
        anyhow!(
            "Cannot resolve project directory '{}': {}",
            raw_dir,
            e
        )
    })?;
    let project_dir_str = project_dir
        .to_str()
        .ok_or_else(|| anyhow!("Project path contains non-UTF-8 characters"))?
        .to_owned();

    // ── 2. Find per-project `.spex/state.db` ──────────────────────────────
    let project_root = find_project_root(&project_dir).ok_or_else(|| {
        anyhow!(
            "No .spex/ directory found in '{}' or any parent. \
             Run `spex init` first.",
            project_dir.display()
        )
    })?;
    let old_db_path = get_db_path(&project_root);
    if !old_db_path.exists() {
        return Err(anyhow!(
            "Per-project DB not found at '{}'",
            old_db_path.display()
        ));
    }
    let old_db_path_str = old_db_path
        .to_str()
        .ok_or_else(|| anyhow!("Old DB path contains non-UTF-8 characters"))?
        .to_owned();

    println!(
        "{}",
        "╔══════════════════════════════════════════════════╗".cyan()
    );
    println!(
        "{}",
        "║         spex db migrate-to-global                ║".cyan()
    );
    println!(
        "{}",
        "╚══════════════════════════════════════════════════╝".cyan()
    );
    println!();
    println!(
        "  {}  {}",
        "Source DB:".bold(),
        old_db_path.display()
    );
    println!("  {}  {}", "Project:  ".bold(), project_dir_str);
    if dry_run {
        println!("  {}", "(dry-run — no writes will be made)".yellow());
    }
    println!();

    // ── 3. Open old DB without running migrations ──────────────────────────
    // We must NOT run sqlx migrations against the old DB: its schema predates
    // the global-project_dir migration and the migrator would either try to
    // apply it (breaking the old DB) or fail on the already-applied earlier
    // migrations.  Open it read-only for the purposes of counting / attaching.
    let old_options = SqliteConnectOptions::new()
        .filename(&old_db_path)
        .create_if_missing(false)
        .journal_mode(sqlx::sqlite::SqliteJournalMode::Wal)
        .read_only(false); // needs to be writable for ATTACH in new conn
    let _old_pool = SqlitePoolOptions::new()
        .max_connections(1)
        .connect_with(old_options)
        .await
        .map_err(|e| anyhow!("Failed to open old DB at '{}': {}", old_db_path.display(), e))?;

    // ── 4. Open / create global DB (runs migrations) ───────────────────────
    let global_pool = open_global_db().await?;
    let global_db_path = crate::sdd::db::global_db_path()?;
    let global_db_path_str = global_db_path
        .to_str()
        .ok_or_else(|| anyhow!("Global DB path contains non-UTF-8 characters"))?
        .to_owned();

    // ── 5. Count rows in old DB (always done — shown in dry-run too) ───────
    //
    // We use a single connection from the old pool to ATTACH the global DB and
    // perform the INSERT … SELECT in one transaction.  For counting we query
    // the old pool directly.
    let mut counts: Vec<(&str, i64)> = Vec::new();
    for &table in TABLES {
        let row: (i64,) =
            sqlx::query_as(&format!("SELECT COUNT(*) FROM {table}"))
                .fetch_one(&_old_pool)
                .await
                .unwrap_or((0,));
        counts.push((table, row.0));
    }

    // Print dry-run summary or proceed with migration
    if dry_run {
        println!("{}", "Would migrate:".bold());
        println!();
        for (table, count) in &counts {
            println!(
                "  {} {:>6} rows  →  {}",
                "·".dimmed(),
                count.to_string().cyan(),
                table.bold()
            );
        }
        println!();
        let total: i64 = counts.iter().map(|(_, c)| c).sum();
        println!(
            "  {} {} rows from {} → global DB",
            "Total:".bold(),
            total.to_string().cyan(),
            old_db_path.display()
        );
        println!();
        println!(
            "  {}",
            "Re-run without --dry-run to apply.".dimmed()
        );
        return Ok(());
    }

    // ── 6. Migrate using ATTACH DATABASE + INSERT OR IGNORE SELECT ─────────
    //
    // We acquire a single dedicated connection, ATTACH the old DB as `old`,
    // run one INSERT … SELECT per table inside a transaction, then DETACH.
    // Using `INSERT OR IGNORE` ensures idempotency — running the command
    // twice is safe.

    let mut conn = global_pool.acquire().await?;

    // ATTACH the old (per-project) DB onto the global connection.
    sqlx::query(&format!("ATTACH DATABASE '{}' AS old", old_db_path_str))
        .execute(&mut *conn)
        .await
        .map_err(|e| anyhow!("ATTACH DATABASE failed: {}", e))?;

    // Disable FK enforcement for the duration of the bulk copy so that the
    // insert order does not matter (the old DB's referential integrity is
    // already established).
    sqlx::query("PRAGMA foreign_keys = OFF")
        .execute(&mut *conn)
        .await?;

    // We intentionally do NOT wrap everything in a single BEGIN/COMMIT here
    // because SQLite's ATTACH + large transactions can lock the attached DB.
    // Instead we run each table in its own implicit transaction (autocommit).

    let mut migrated: Vec<(&str, u64)> = Vec::new();

    // ── specs ──────────────────────────────────────────────────────────────
    let r = sqlx::query(
        "INSERT OR IGNORE INTO specs \
           (id, project_dir, title, status, priority, depends_on, agents, \
            ac_total, ac_passed, created_at, updated_at, updated_by) \
         SELECT id, ?, title, status, priority, depends_on, agents, \
                ac_total, ac_passed, created_at, updated_at, updated_by \
         FROM old.specs",
    )
    .bind(&project_dir_str)
    .execute(&mut *conn)
    .await?;
    migrated.push(("specs", r.rows_affected()));

    // ── tasks ──────────────────────────────────────────────────────────────
    let r = sqlx::query(
        "INSERT OR IGNORE INTO tasks \
           (id, project_dir, spec, title, agent, status, inputs, output_artifact, \
            created_at, updated_at, depends_on, conflicts_with, lock_set, \
            plan_version, lock_requirements, priority, risk_level, \
            execution_bucket, estimate_points, unblock_value) \
         SELECT id, ?, spec, title, agent, status, inputs, output_artifact, \
                created_at, updated_at, \
                COALESCE(depends_on,'[]'), COALESCE(conflicts_with,'[]'), \
                COALESCE(lock_set,'[]'), plan_version, \
                COALESCE(lock_requirements,'[]'), \
                COALESCE(priority,100), COALESCE(risk_level,'medium'), \
                COALESCE(execution_bucket,'coordinated_parallel'), \
                COALESCE(estimate_points,3), COALESCE(unblock_value,0) \
         FROM old.tasks",
    )
    .bind(&project_dir_str)
    .execute(&mut *conn)
    .await?;
    migrated.push(("tasks", r.rows_affected()));

    // ── events ─────────────────────────────────────────────────────────────
    // events has an AUTOINCREMENT PK — we omit `id` so global IDs are new.
    let r = sqlx::query(
        "INSERT OR IGNORE INTO events \
           (project_dir, type, spec, agent, payload, timestamp) \
         SELECT ?, type, spec, agent, payload, timestamp \
         FROM old.events",
    )
    .bind(&project_dir_str)
    .execute(&mut *conn)
    .await?;
    migrated.push(("events", r.rows_affected()));

    // ── memory ─────────────────────────────────────────────────────────────
    // memory also has AUTOINCREMENT PK — omit `id`.
    let r = sqlx::query(
        "INSERT OR IGNORE INTO memory \
           (project_dir, agent, key, value, spec, updated_at, \
            type, deleted_at, expires_at, access_count, last_accessed_at, \
            revision_count) \
         SELECT ?, agent, key, value, COALESCE(spec,''), updated_at, \
                NULL, NULL, NULL, 0, NULL, 1 \
         FROM old.memory",
    )
    .bind(&project_dir_str)
    .execute(&mut *conn)
    .await?;
    migrated.push(("memory", r.rows_affected()));

    // ── artifacts ──────────────────────────────────────────────────────────
    let r = sqlx::query(
        "INSERT OR IGNORE INTO artifacts \
           (id, project_dir, spec, task, agent, type, path, description, created_at) \
         SELECT id, ?, spec, task, agent, type, path, description, created_at \
         FROM old.artifacts",
    )
    .bind(&project_dir_str)
    .execute(&mut *conn)
    .await?;
    migrated.push(("artifacts", r.rows_affected()));

    // ── incidents ──────────────────────────────────────────────────────────
    let r = sqlx::query(
        "INSERT OR IGNORE INTO incidents \
           (id, project_dir, spec_id, task_id, title, severity, status, source, \
            blocking, repro_steps, root_cause, fix_strategy, created_at, updated_at) \
         SELECT id, ?, spec_id, task_id, title, severity, status, source, \
                blocking, repro_steps, root_cause, fix_strategy, created_at, updated_at \
         FROM old.incidents",
    )
    .bind(&project_dir_str)
    .execute(&mut *conn)
    .await?;
    migrated.push(("incidents", r.rows_affected()));

    // ── context_gaps ───────────────────────────────────────────────────────
    let r = sqlx::query(
        "INSERT OR IGNORE INTO context_gaps \
           (id, project_dir, spec_id, task_id, kind, criticality, status, blocking, \
            question, assumption, resolution, created_at, updated_at) \
         SELECT id, ?, spec_id, task_id, kind, criticality, status, blocking, \
                question, assumption, resolution, created_at, updated_at \
         FROM old.context_gaps",
    )
    .bind(&project_dir_str)
    .execute(&mut *conn)
    .await?;
    migrated.push(("context_gaps", r.rows_affected()));

    // ── verification_runs ──────────────────────────────────────────────────
    let r = sqlx::query(
        "INSERT OR IGNORE INTO verification_runs \
           (id, project_dir, spec_id, task_id, slice_id, kind, status, \
            command, summary, evidence, created_at) \
         SELECT id, ?, spec_id, task_id, slice_id, kind, status, \
                command, summary, evidence, created_at \
         FROM old.verification_runs",
    )
    .bind(&project_dir_str)
    .execute(&mut *conn)
    .await?;
    migrated.push(("verification_runs", r.rows_affected()));

    // ── interrupts ─────────────────────────────────────────────────────────
    let r = sqlx::query(
        "INSERT OR IGNORE INTO interrupts \
           (id, project_dir, spec_id, reason_type, status, preempted_tasks, \
            resume_hint, created_at, updated_at) \
         SELECT id, ?, spec_id, reason_type, status, \
                COALESCE(preempted_tasks,'[]'), \
                resume_hint, created_at, updated_at \
         FROM old.interrupts",
    )
    .bind(&project_dir_str)
    .execute(&mut *conn)
    .await?;
    migrated.push(("interrupts", r.rows_affected()));

    // ── handoff_snapshots ──────────────────────────────────────────────────
    let r = sqlx::query(
        "INSERT OR IGNORE INTO handoff_snapshots \
           (id, project_dir, spec_id, interrupt_id, last_wave, last_task, \
            files_touched, decisions, open_risks, next_steps, created_at) \
         SELECT id, ?, spec_id, interrupt_id, last_wave, last_task, \
                COALESCE(files_touched,'[]'), COALESCE(decisions,'[]'), \
                COALESCE(open_risks,'[]'), COALESCE(next_steps,'[]'), \
                created_at \
         FROM old.handoff_snapshots",
    )
    .bind(&project_dir_str)
    .execute(&mut *conn)
    .await?;
    migrated.push(("handoff_snapshots", r.rows_affected()));

    // ── plan_versions ──────────────────────────────────────────────────────
    let r = sqlx::query(
        "INSERT OR IGNORE INTO plan_versions \
           (id, project_dir, spec_id, version, status, reason, plan_json, created_at) \
         SELECT id, ?, spec_id, version, status, reason, plan_json, created_at \
         FROM old.plan_versions",
    )
    .bind(&project_dir_str)
    .execute(&mut *conn)
    .await?;
    migrated.push(("plan_versions", r.rows_affected()));

    // ── task_leases ────────────────────────────────────────────────────────
    // task_leases PK is (project_dir, task_id) — no separate `id` column.
    let r = sqlx::query(
        "INSERT OR IGNORE INTO task_leases \
           (project_dir, task_id, agent_id, status, lease_expires_at, heartbeat_at, \
            attempt_count, created_at, updated_at) \
         SELECT ?, task_id, agent_id, status, lease_expires_at, heartbeat_at, \
                attempt_count, created_at, updated_at \
         FROM old.task_leases",
    )
    .bind(&project_dir_str)
    .execute(&mut *conn)
    .await?;
    migrated.push(("task_leases", r.rows_affected()));

    // ── task_locks ─────────────────────────────────────────────────────────
    let r = sqlx::query(
        "INSERT OR IGNORE INTO task_locks \
           (id, project_dir, task_id, spec_id, lock_type, resource, status, \
            acquired_at, released_at) \
         SELECT id, ?, task_id, spec_id, lock_type, resource, status, \
                acquired_at, released_at \
         FROM old.task_locks",
    )
    .bind(&project_dir_str)
    .execute(&mut *conn)
    .await?;
    migrated.push(("task_locks", r.rows_affected()));

    // ── replan_requests ────────────────────────────────────────────────────
    let r = sqlx::query(
        "INSERT OR IGNORE INTO replan_requests \
           (id, project_dir, spec_id, task_id, agent_id, reason, impact, \
            proposed_action, status, created_at, updated_at) \
         SELECT id, ?, spec_id, task_id, agent_id, reason, \
                COALESCE(impact,'[]'), proposed_action, status, \
                created_at, updated_at \
         FROM old.replan_requests",
    )
    .bind(&project_dir_str)
    .execute(&mut *conn)
    .await?;
    migrated.push(("replan_requests", r.rows_affected()));

    // Re-enable FK enforcement.
    sqlx::query("PRAGMA foreign_keys = ON")
        .execute(&mut *conn)
        .await?;

    // Detach the old DB.
    sqlx::query("DETACH DATABASE old")
        .execute(&mut *conn)
        .await
        .map_err(|e| anyhow!("DETACH DATABASE failed: {}", e))?;

    drop(conn);

    // ── 7. Print summary ───────────────────────────────────────────────────
    let total: u64 = migrated.iter().map(|(_, c)| c).sum();
    println!("{}", "Migration complete:".green().bold());
    println!();
    for (table, count) in &migrated {
        if *count > 0 {
            println!(
                "  {} {:>6} rows  →  {}",
                "✓".green(),
                count.to_string().cyan(),
                table.bold()
            );
        } else {
            println!(
                "  {} {:>6} rows     {}",
                "·".dimmed(),
                "0".dimmed(),
                table.dimmed()
            );
        }
    }
    println!();
    println!(
        "  {} {} rows migrated",
        "Total:".bold(),
        total.to_string().green().bold()
    );
    println!();
    println!(
        "  {}  {}",
        "From:".bold(),
        old_db_path.display().to_string().dimmed()
    );
    println!(
        "  {}  {}",
        "To:  ".bold(),
        global_db_path_str.dimmed()
    );
    println!(
        "  {}  {}",
        "Tag: ".bold(),
        project_dir_str.cyan()
    );

    Ok(())
}
