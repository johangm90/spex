/// Integration tests for SLICE-005 global DB, project_dir filtering,
/// scaffold no-state-db, and migrate-to-global.
///
/// All tests use `tempfile::TempDir` so no real files are left behind, and
/// none of them touch `~/.local/share/spex/global-state.db`.
use spex::sdd::db::{ensure_spex_dir, get_db_path, global_db_path, open_db};

// ── Helper ────────────────────────────────────────────────────────────────────

/// Open a fresh in-file SQLite DB at the given path and run all migrations.
async fn open_temp_db(path: &std::path::Path) -> sqlx::SqlitePool {
    open_db(path).await.expect("open_db failed")
}

// ─────────────────────────────────────────────────────────────────────────────
// Test 1 — global DB path ends with "spex/global-state.db"
// ─────────────────────────────────────────────────────────────────────────────

#[tokio::test]
async fn test_global_db_path_ends_with_expected_suffix() {
    let path = global_db_path().expect("global_db_path() should succeed");
    // Convert to forward-slash string for cross-platform suffix check.
    let path_str = path.to_string_lossy().replace('\\', "/");
    assert!(
        path_str.ends_with("spex/global-state.db"),
        "global_db_path() should end with 'spex/global-state.db', got: {path_str}"
    );
}

/// Test 1b — a DB opened at a temp path (simulating global DB) is usable.
#[tokio::test]
async fn test_global_db_opens_at_expected_path() {
    let tmp = tempfile::TempDir::new().expect("tempdir");
    let db_path = tmp.path().join("spex").join("global-state.db");

    // open_db creates parent dirs and runs migrations.
    let pool = open_temp_db(&db_path).await;

    // Run a trivial PRAGMA to confirm the connection is live.
    let (page_size,): (i64,) = sqlx::query_as("PRAGMA page_size")
        .fetch_one(&pool)
        .await
        .expect("PRAGMA page_size should succeed");
    assert!(page_size > 0, "page_size should be positive");

    // File must have been created on disk.
    assert!(db_path.exists(), "DB file should exist after open_db()");
}

// ─────────────────────────────────────────────────────────────────────────────
// Test 2 — project_dir filtering
// ─────────────────────────────────────────────────────────────────────────────

#[tokio::test]
async fn test_project_dir_filtering() {
    let tmp = tempfile::TempDir::new().expect("tempdir");
    let db_path = tmp.path().join("global-state.db");
    let pool = open_temp_db(&db_path).await;

    let alpha = "/project/alpha";
    let beta = "/project/beta";

    // Insert a spec scoped to alpha.
    spex::sdd::spec::create_spec(&pool, alpha, "SPEC-001", "Alpha spec", "P1", &[])
        .await
        .expect("create_spec alpha");

    // Insert a spec scoped to beta.
    spex::sdd::spec::create_spec(&pool, beta, "SPEC-001", "Beta spec", "P1", &[])
        .await
        .expect("create_spec beta");

    // Querying with project_dir = beta should return exactly 1 result.
    let beta_specs = spex::sdd::spec::list_specs(&pool, beta)
        .await
        .expect("list_specs beta");
    assert_eq!(
        beta_specs.len(),
        1,
        "Expected 1 spec for /project/beta, got {}",
        beta_specs.len()
    );
    assert_eq!(beta_specs[0].id, "SPEC-001");
    assert_eq!(beta_specs[0].title, "Beta spec");

    // Querying with project_dir = alpha should also return exactly 1 result.
    let alpha_specs = spex::sdd::spec::list_specs(&pool, alpha)
        .await
        .expect("list_specs alpha");
    assert_eq!(
        alpha_specs.len(),
        1,
        "Expected 1 spec for /project/alpha, got {}",
        alpha_specs.len()
    );
    assert_eq!(alpha_specs[0].title, "Alpha spec");

    // Querying with an unknown project_dir should return nothing.
    let unknown_specs = spex::sdd::spec::list_specs(&pool, "/project/gamma")
        .await
        .expect("list_specs gamma");
    assert!(
        unknown_specs.is_empty(),
        "Expected 0 specs for unknown project_dir"
    );
}

// ─────────────────────────────────────────────────────────────────────────────
// Test 3 — migrate-to-global logic
// ─────────────────────────────────────────────────────────────────────────────

/// Reproduces the migrate-to-global logic inline (using temp DBs only, so the
/// real `~/.local/share/spex/global-state.db` is never touched).
#[tokio::test]
async fn test_migrate_to_global() {
    let src_tmp = tempfile::TempDir::new().expect("src tempdir");
    let dst_tmp = tempfile::TempDir::new().expect("dst tempdir");

    let src_path = src_tmp.path().join("state.db");
    let dst_path = dst_tmp.path().join("global-state.db");

    // ── Set up the source (per-project) DB with 3 specs (project_dir = "") ──
    let src_pool = open_temp_db(&src_path).await;
    for i in 1u8..=3 {
        spex::sdd::spec::create_spec(
            &src_pool,
            "",
            &format!("SPEC-{i:03}"),
            &format!("Spec {i}"),
            "P1",
            &[],
        )
        .await
        .expect("create_spec in src");
    }

    // Confirm 3 specs with project_dir = "" before migration.
    let pre = spex::sdd::spec::list_specs(&src_pool, "")
        .await
        .expect("list_specs src pre");
    assert_eq!(pre.len(), 3, "Expected 3 specs in source DB before migrate");

    drop(src_pool);

    // ── Set up the target (global) DB ─────────────────────────────────────
    let dst_pool = open_temp_db(&dst_path).await;

    // ── Reproduce the migrate logic inline ────────────────────────────────
    let project_dir_tag = src_tmp
        .path()
        .canonicalize()
        .expect("canonicalize src_tmp")
        .to_string_lossy()
        .to_string();

    let src_path_str = src_path.to_str().expect("src path to str").to_owned();

    // Acquire a single connection and ATTACH the source DB.
    let mut conn = dst_pool.acquire().await.expect("acquire conn");

    sqlx::query(&format!("ATTACH DATABASE '{src_path_str}' AS old"))
        .execute(&mut *conn)
        .await
        .expect("ATTACH src DB");

    sqlx::query("PRAGMA foreign_keys = OFF")
        .execute(&mut *conn)
        .await
        .expect("disable FK");

    // Migrate specs only (sufficient for this test's AC).
    let r = sqlx::query(
        "INSERT OR IGNORE INTO specs \
           (id, project_dir, title, status, priority, depends_on, agents, \
            ac_total, ac_passed, created_at, updated_at, updated_by) \
         SELECT id, ?, title, status, priority, depends_on, agents, \
                ac_total, ac_passed, created_at, updated_at, updated_by \
         FROM old.specs",
    )
    .bind(&project_dir_tag)
    .execute(&mut *conn)
    .await
    .expect("migrate specs");

    assert_eq!(
        r.rows_affected(),
        3,
        "Expected 3 rows migrated into global DB"
    );

    sqlx::query("PRAGMA foreign_keys = ON")
        .execute(&mut *conn)
        .await
        .expect("enable FK");

    sqlx::query("DETACH DATABASE old")
        .execute(&mut *conn)
        .await
        .expect("DETACH src DB");

    drop(conn);

    // ── Assert: global DB has 3 specs tagged with the canonicalized path ──
    let migrated = spex::sdd::spec::list_specs(&dst_pool, &project_dir_tag)
        .await
        .expect("list_specs dst after migrate");

    assert_eq!(
        migrated.len(),
        3,
        "Expected 3 specs in global DB after migration, got {}",
        migrated.len()
    );

    // Every spec should carry the canonical project_dir tag.
    // We verify via a raw query that project_dir matches on all rows.
    let tagged: (i64,) = sqlx::query_as(
        "SELECT COUNT(*) FROM specs WHERE project_dir = ?",
    )
    .bind(&project_dir_tag)
    .fetch_one(&dst_pool)
    .await
    .expect("count tagged specs");
    assert_eq!(tagged.0, 3, "All 3 migrated specs should carry the project_dir tag");
}

// ─────────────────────────────────────────────────────────────────────────────
// Test 4 — scaffold (init_project) does NOT create .spex/state.db
// ─────────────────────────────────────────────────────────────────────────────

#[tokio::test]
async fn test_scaffold_does_not_create_state_db() {
    let tmp = tempfile::TempDir::new().expect("tempdir");

    // Call init_project (the spex init equivalent) — it runs without prompts.
    spex::scaffold::init_project(tmp.path())
        .await
        .expect("init_project should succeed");

    // .spex/ directory must exist.
    let spex_dir = tmp.path().join(".spex");
    assert!(
        spex_dir.exists() && spex_dir.is_dir(),
        ".spex/ directory should have been created by init_project"
    );

    // .spex/state.db must NOT exist (global-only design: DB lives in data_dir).
    let state_db = get_db_path(tmp.path());
    assert!(
        !state_db.exists(),
        ".spex/state.db should NOT be created by init_project (global DB only); \
         found: {}",
        state_db.display()
    );
}

// ─────────────────────────────────────────────────────────────────────────────
// Test 5 — ensure_spex_dir is idempotent
// ─────────────────────────────────────────────────────────────────────────────

#[tokio::test]
async fn test_ensure_spex_dir_is_idempotent() {
    let tmp = tempfile::TempDir::new().expect("tempdir");

    ensure_spex_dir(tmp.path()).expect("first call");
    ensure_spex_dir(tmp.path()).expect("second call should be idempotent");

    let spex_dir = tmp.path().join(".spex");
    assert!(spex_dir.exists() && spex_dir.is_dir());
}
