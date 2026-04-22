use sqlx::SqlitePool;

/// Create a fresh in-memory SQLite pool with all migrations applied.
pub async fn make_pool() -> SqlitePool {
    let pool = SqlitePool::connect(":memory:")
        .await
        .expect("failed to open in-memory SQLite");
    sqlx::migrate!("./migrations")
        .run(&pool)
        .await
        .expect("failed to run migrations");
    pool
}

/// Alias for [`make_pool`] — used by tests that follow the `open_test_db` naming convention.
pub async fn open_test_db() -> SqlitePool {
    make_pool().await
}
