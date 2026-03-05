use anyhow::{anyhow, Result};
use sqlx::{
    sqlite::{SqliteConnectOptions, SqliteJournalMode, SqlitePoolOptions, SqliteSynchronous},
    SqlitePool,
};
use std::path::{Path, PathBuf};

/// Walk up from current dir looking for `.spex/` directory.
pub fn find_project_root() -> Result<PathBuf> {
    let mut current = std::env::current_dir()?;
    loop {
        let spex_dir = current.join(".spex");
        if spex_dir.exists() && spex_dir.is_dir() {
            return Ok(current);
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

/// Ensure the `.spex/` directory exists under the given root.
pub fn ensure_spex_dir(root: &Path) -> Result<()> {
    let spex_dir = root.join(".spex");
    if !spex_dir.exists() {
        std::fs::create_dir_all(&spex_dir)?;
    }
    Ok(())
}

/// Returns path to `root/.spex/state.db`.
pub fn get_db_path(root: &Path) -> PathBuf {
    root.join(".spex").join("state.db")
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

/// Find project root and open its database.
pub async fn open_project_db() -> Result<SqlitePool> {
    let root = find_project_root()?;
    let db_path = get_db_path(&root);
    open_db(&db_path).await
}
