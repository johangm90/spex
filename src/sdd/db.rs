use anyhow::{anyhow, Result};
use sqlx::{
    sqlite::{SqliteConnectOptions, SqliteJournalMode, SqlitePoolOptions, SqliteSynchronous},
    SqlitePool,
};
use std::path::{Path, PathBuf};

/// Ensure the `.spex/` directory exists under the given root.
pub fn ensure_spex_dir(root: &Path) -> Result<()> {
    let spex_dir = root.join(".spex");
    if !spex_dir.exists() {
        std::fs::create_dir_all(&spex_dir)?;
    }
    Ok(())
}

/// Returns path to `root/.spex/state.db`.
// TODO(SLICE-005): remove after T05-11 (scaffold) and T05-09 (migrate-to-global) are complete
pub fn get_db_path(root: &Path) -> PathBuf {
    root.join(".spex").join("state.db")
}

/// Returns the path to the global spex database: `~/.local/share/spex/global-state.db`.
pub fn global_db_path() -> Result<PathBuf> {
    let data_dir = dirs::data_dir()
        .ok_or_else(|| anyhow!("Could not determine user data directory"))?;
    Ok(data_dir.join("spex").join("global-state.db"))
}

/// Open (or create) the SQLite database at the given path, apply migrations.
///
/// WAL mode and foreign-key enforcement are set via `SqliteConnectOptions` so
/// they are applied at connection-open time — before sqlx ever starts a
/// migration transaction — avoiding the "cannot start a transaction within a
/// transaction" error that occurs when raw PRAGMA queries are run on the pool
/// after it is created.
pub async fn open_db(path: &Path) -> Result<SqlitePool> {
    // Ensure parent directory exists
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }

    let options = SqliteConnectOptions::new()
        .filename(path)
        .create_if_missing(true)
        .journal_mode(SqliteJournalMode::Wal)
        .synchronous(SqliteSynchronous::Normal)
        .foreign_keys(true);

    let pool = SqlitePoolOptions::new()
        .max_connections(5)
        .connect_with(options)
        .await?;

    // Run migrations
    sqlx::migrate!("./migrations").run(&pool).await?;

    Ok(pool)
}

/// The canonical entry point for opening the global spex database.
///
/// Opens (or creates) `~/.local/share/spex/global-state.db` and applies all
/// pending migrations. This is the only DB-open entry point used by spex
/// commands going forward — the old per-project `open_project_db()` is
/// deprecated and will be removed once all callers are updated (SLICE-005).
pub async fn open_global_db() -> Result<SqlitePool> {
    open_db(&global_db_path()?).await
}

/// Open the project-local database (`.spex/state.db`).
// TODO(SLICE-005): remove after T05-12 (main.rs callers updated) and T05-01 (doctor) are complete
pub async fn open_project_db() -> Result<SqlitePool> {
    let mut current = std::env::current_dir()?;
    loop {
        let spex_dir = current.join(".spex");
        if spex_dir.exists() && spex_dir.is_dir() {
            let db_path = get_db_path(&current);
            return open_db(&db_path).await;
        }
        match current.parent() {
            Some(parent) => current = parent.to_path_buf(),
            None => {
                return Err(anyhow!(
                    "Not in a spex project. Run `spex new <name>` to create one."
                ))
            }
        }
    }
}
