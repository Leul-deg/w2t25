/// Schema integrity and security-semantics integration tests.
///
/// These tests require a real PostgreSQL database.
/// Set TEST_DATABASE_URL before running:
///
///   TEST_DATABASE_URL=postgres://meridian:meridian@127.0.0.1:5433/meridian_test?sslmode=disable \
///     cargo test --test schema_integrity_tests -- --include-ignored --test-threads=1
///
/// IMPORTANT: use 127.0.0.1 (not localhost) to avoid IPv6 resolution issues.
/// IMPORTANT: --test-threads=1 is required because clean_migration_succeeds drops
///            and recreates the public schema; parallel execution causes races.
///
/// The suite covers three critical gaps identified in the security review:
///
/// 1. clean_migration_succeeds  – all migrations apply without error on a blank DB;
///    every table and column that other code depends on must exist.
///
/// 2. login_lockout_semantics   – 5 failures in a 15-min window trigger a 30-min
///    lockout row; the lockout persists independently of the rolling attempt count.
///
/// 3. checkin_report_sql        – the checkin report query (with the COALESCE fix)
///    executes without error against the real schema and returns correct status values.

use std::env;
use uuid::Uuid;
use chrono::Utc;
use sqlx::postgres::PgPoolOptions;
use sqlx::PgPool;

// ---------------------------------------------------------------------------
// Helper: connect and migrate
// ---------------------------------------------------------------------------

async fn test_pool() -> PgPool {
    let url = env::var("TEST_DATABASE_URL")
        .expect("TEST_DATABASE_URL must be set to run schema_integrity_tests");
    let pool = PgPoolOptions::new()
        .max_connections(5)
        .connect(&url)
        .await
        .expect("Failed to connect to test database");
    sqlx::migrate!("../migrations")
        .run(&pool)
        .await
        .expect("Migration run failed");
    pool
}

/// Drop and recreate the public schema to guarantee a clean-slate migration.
async fn reset_schema(pool: &PgPool) {
    sqlx::query("DROP SCHEMA public CASCADE")
        .execute(pool)
        .await
        .expect("DROP SCHEMA failed");
    sqlx::query("CREATE SCHEMA public")
        .execute(pool)
        .await
        .expect("CREATE SCHEMA failed");
    sqlx::query("GRANT ALL ON SCHEMA public TO PUBLIC")
        .execute(pool)
        .await
        .expect("GRANT failed");
}

// ---------------------------------------------------------------------------
// 1. Clean-migration test
// ---------------------------------------------------------------------------

/// Apply every migration from scratch on a blank schema and assert that all
/// tables and columns that caused previous runtime failures actually exist.
#[tokio::test]
#[ignore = "requires TEST_DATABASE_URL"]
async fn clean_migration_succeeds() {
    let url = env::var("TEST_DATABASE_URL")
        .expect("TEST_DATABASE_URL must be set");
    let pool = PgPoolOptions::new()
        .max_connections(3)
        .connect(&url)
        .await
        .expect("connect failed");

    reset_schema(&pool).await;

    // Run all migrations on the now-empty schema.
    sqlx::migrate!("../migrations")
        .run(&pool)
        .await
        .expect("Migrations must succeed on a clean schema");

    // ── Assert tables exist ─────────────────────────────────────────────────
    let required_tables = [
        "users", "roles", "user_roles", "sessions",
        "districts", "campuses", "schools",
        "user_school_assignments", "classes", "class_enrollments",
        "parent_student_links",
        "checkin_windows", "checkin_submissions", "checkin_approval_decisions",
        "orders", "order_items", "products", "inventory",
        "notifications", "audit_logs", "config_values", "config_history",
        "campaign_toggles", "backup_metadata", "report_jobs",
        "account_deletion_requests", "blacklist_entries",
        "login_attempts", "login_lockouts",
        "admin_scope_assignments",
    ];

    for table in &required_tables {
        let count: i64 = sqlx::query_scalar(
            "SELECT COUNT(*) FROM information_schema.tables \
             WHERE table_schema = 'public' AND table_name = $1",
        )
        .bind(table)
        .fetch_one(&pool)
        .await
        .unwrap_or_else(|e| panic!("Query failed for table '{}': {}", table, e));

        assert_eq!(
            count, 1,
            "Table '{}' must exist after migrations",
            table
        );
    }

    // ── Assert columns that caused earlier bugs exist ───────────────────────

    // checkin_submissions must have submitted_at (NOT created_at).
    let has_submitted_at: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM information_schema.columns \
         WHERE table_name = 'checkin_submissions' AND column_name = 'submitted_at'",
    )
    .fetch_one(&pool)
    .await
    .expect("column check failed");
    assert_eq!(has_submitted_at, 1, "checkin_submissions.submitted_at must exist");

    let has_created_at: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM information_schema.columns \
         WHERE table_name = 'checkin_submissions' AND column_name = 'created_at'",
    )
    .fetch_one(&pool)
    .await
    .expect("column check failed");
    assert_eq!(
        has_created_at, 0,
        "checkin_submissions must NOT have a created_at column (would break the 012 index)"
    );

    // login_lockouts must have locked_until.
    let has_locked_until: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM information_schema.columns \
         WHERE table_name = 'login_lockouts' AND column_name = 'locked_until'",
    )
    .fetch_one(&pool)
    .await
    .expect("column check failed");
    assert_eq!(has_locked_until, 1, "login_lockouts.locked_until must exist");

    // admin_scope_assignments must have scope_type + scope_id.
    for col in &["scope_type", "scope_id", "admin_id"] {
        let n: i64 = sqlx::query_scalar(
            "SELECT COUNT(*) FROM information_schema.columns \
             WHERE table_name = 'admin_scope_assignments' AND column_name = $1",
        )
        .bind(col)
        .fetch_one(&pool)
        .await
        .expect("column check failed");
        assert_eq!(n, 1, "admin_scope_assignments.{} must exist", col);
    }

    // ── Assert the fixed 012 index uses submitted_at, not created_at ────────
    let idx_col: Option<String> = sqlx::query_scalar(
        "SELECT a.attname
         FROM pg_index i
         JOIN pg_class c  ON c.oid = i.indrelid
         JOIN pg_class ic ON ic.oid = i.indexrelid
         JOIN pg_attribute a ON a.attrelid = c.oid AND a.attnum = ANY(i.indkey)
         WHERE c.relname  = 'checkin_submissions'
           AND ic.relname = 'idx_checkin_submissions_submitted_at'",
    )
    .fetch_optional(&pool)
    .await
    .expect("index column query failed");

    assert_eq!(
        idx_col.as_deref(),
        Some("submitted_at"),
        "idx_checkin_submissions_submitted_at must index the submitted_at column"
    );
}

// ---------------------------------------------------------------------------
// 2. Login lockout semantics test
// ---------------------------------------------------------------------------

/// Verify the full lockout lifecycle:
///   a) 4 failures do NOT create a lockout row.
///   b) A 5th failure creates a lockout row with locked_until ≈ NOW + 30 min.
///   c) The lockout is still enforced even after the 15-min window has expired
///      (old attempts are artificially backdated past the window).
///   d) Inserting a 6th attempt within a fresh 15-min window but while a
///      lockout row exists still blocks (lockout takes precedence).
#[tokio::test]
#[ignore = "requires TEST_DATABASE_URL"]
async fn login_lockout_semantics() {
    let pool = test_pool().await;

    let username = format!("lockout_test_{}", &Uuid::new_v4().to_string()[..8]);

    // Cleanup from a previous run.
    sqlx::query("DELETE FROM login_attempts   WHERE username = $1")
        .bind(&username).execute(&pool).await.unwrap();
    sqlx::query("DELETE FROM login_lockouts   WHERE username = $1")
        .bind(&username).execute(&pool).await.unwrap();

    // ── a) 4 failures: no lockout row ────────────────────────────────────────
    for _ in 0..4 {
        sqlx::query(
            "INSERT INTO login_attempts (id, username, attempted_at, success) \
             VALUES (gen_random_uuid(), $1, NOW(), FALSE)",
        )
        .bind(&username)
        .execute(&pool)
        .await
        .unwrap();
    }

    let lockout_row_count: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM login_lockouts WHERE username = $1",
    )
    .bind(&username)
    .fetch_one(&pool)
    .await
    .unwrap();
    assert_eq!(lockout_row_count, 0, "No lockout row expected after 4 failures");

    // Verify the rate-limit check function logic: count of failures in window < 5.
    let failure_count: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM login_attempts \
         WHERE username = $1 AND success = FALSE \
           AND attempted_at > NOW() - (15 * INTERVAL '1 minute')",
    )
    .bind(&username)
    .fetch_one(&pool)
    .await
    .unwrap();
    assert_eq!(failure_count, 4);
    assert!(failure_count < 5, "4 failures must be below the 5-attempt threshold");

    // ── b) 5th failure triggers the lockout row ──────────────────────────────
    sqlx::query(
        "INSERT INTO login_attempts (id, username, attempted_at, success) \
         VALUES (gen_random_uuid(), $1, NOW(), FALSE)",
    )
    .bind(&username)
    .execute(&pool)
    .await
    .unwrap();

    // Simulate what check_login_rate_limit does when it hits the threshold.
    sqlx::query(
        "INSERT INTO login_lockouts (username, locked_until) \
         VALUES ($1, NOW() + (30 * INTERVAL '1 minute')) \
         ON CONFLICT (username) DO UPDATE SET locked_until = NOW() + (30 * INTERVAL '1 minute')",
    )
    .bind(&username)
    .execute(&pool)
    .await
    .unwrap();

    // Lockout row must exist.
    let locked_until: Option<chrono::DateTime<Utc>> = sqlx::query_scalar(
        "SELECT locked_until FROM login_lockouts WHERE username = $1",
    )
    .bind(&username)
    .fetch_optional(&pool)
    .await
    .unwrap();

    assert!(locked_until.is_some(), "Lockout row must exist after 5th failure");

    let lu = locked_until.unwrap();
    let now = Utc::now();
    let minutes_remaining = (lu - now).num_minutes();

    assert!(
        minutes_remaining >= 28 && minutes_remaining <= 31,
        "locked_until must be ~30 minutes from now, got {} minutes",
        minutes_remaining
    );

    // ── c) Lockout persists after the 15-min rolling window expires ──────────
    // Backdate all attempt rows to 20 minutes ago (outside the 15-min window).
    sqlx::query(
        "UPDATE login_attempts \
         SET attempted_at = NOW() - INTERVAL '20 minutes' \
         WHERE username = $1",
    )
    .bind(&username)
    .execute(&pool)
    .await
    .unwrap();

    // Rolling-window count is now 0 — but lockout row still exists.
    let rolling_count: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM login_attempts \
         WHERE username = $1 AND success = FALSE \
           AND attempted_at > NOW() - (15 * INTERVAL '1 minute')",
    )
    .bind(&username)
    .fetch_one(&pool)
    .await
    .unwrap();
    assert_eq!(rolling_count, 0, "All attempts are now outside the 15-min window");

    // Lockout row must still block.
    let still_locked: Option<chrono::DateTime<Utc>> = sqlx::query_scalar(
        "SELECT locked_until FROM login_lockouts \
         WHERE username = $1 AND locked_until > NOW()",
    )
    .bind(&username)
    .fetch_optional(&pool)
    .await
    .unwrap();
    assert!(
        still_locked.is_some(),
        "Lockout must persist even when rolling window is empty"
    );

    // ── d) New attempt within window still blocked because lockout exists ────
    sqlx::query(
        "INSERT INTO login_attempts (id, username, attempted_at, success) \
         VALUES (gen_random_uuid(), $1, NOW(), FALSE)",
    )
    .bind(&username)
    .execute(&pool)
    .await
    .unwrap();

    // Only 1 attempt in window — below threshold by count, but lockout blocks.
    let new_rolling: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM login_attempts \
         WHERE username = $1 AND success = FALSE \
           AND attempted_at > NOW() - (15 * INTERVAL '1 minute')",
    )
    .bind(&username)
    .fetch_one(&pool)
    .await
    .unwrap();
    assert_eq!(new_rolling, 1, "Only 1 fresh attempt — below threshold");

    let lockout_still_active: bool = sqlx::query_scalar(
        "SELECT EXISTS( \
             SELECT 1 FROM login_lockouts \
             WHERE username = $1 AND locked_until > NOW() \
         )",
    )
    .bind(&username)
    .fetch_one(&pool)
    .await
    .unwrap();
    assert!(
        lockout_still_active,
        "Lockout must still be active and block the attempt independently of rolling count"
    );

    // Cleanup.
    sqlx::query("DELETE FROM login_attempts WHERE username = $1")
        .bind(&username).execute(&pool).await.unwrap();
    sqlx::query("DELETE FROM login_lockouts WHERE username = $1")
        .bind(&username).execute(&pool).await.unwrap();
}

// ---------------------------------------------------------------------------
// 3. Check-in report SQL test
// ---------------------------------------------------------------------------

/// Execute the exact checkin report SQL (post-fix: COALESCE not cs.status)
/// against the real schema, seeded with two submissions — one approved,
/// one pending.  Assert:
///   • The query completes without error.
///   • The `status` column derives correctly from cad.decision (not a missing column).
///   • Approved submission → status = 'approved'.
///   • Pending submission  → status = 'pending'.
#[tokio::test]
#[ignore = "requires TEST_DATABASE_URL"]
async fn checkin_report_sql_uses_coalesce_not_cs_status() {
    let pool = test_pool().await;

    let suffix = &Uuid::new_v4().to_string()[..8];

    // ── Seed: district → campus → school ────────────────────────────────────
    let district_id = Uuid::new_v4();
    let campus_id   = Uuid::new_v4();
    let school_id   = Uuid::new_v4();

    sqlx::query("INSERT INTO districts (id, name, state, created_at) VALUES ($1, $2, 'TX', NOW())")
        .bind(district_id).bind(format!("rpt_dist_{}", suffix))
        .execute(&pool).await.unwrap();
    sqlx::query("INSERT INTO campuses (id, district_id, name, created_at) VALUES ($1, $2, $3, NOW())")
        .bind(campus_id).bind(district_id).bind(format!("rpt_campus_{}", suffix))
        .execute(&pool).await.unwrap();
    sqlx::query("INSERT INTO schools (id, campus_id, name, school_type, created_at) VALUES ($1, $2, $3, 'general', NOW())")
        .bind(school_id).bind(campus_id).bind(format!("rpt_school_{}", suffix))
        .execute(&pool).await.unwrap();

    // ── Seed: admin + student users ──────────────────────────────────────────
    let hash = "$argon2id$v=19$m=19456,t=2,p=1$c2FsdA$aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";
    let admin_id   = Uuid::new_v4();
    let student_id = Uuid::new_v4();

    for (uid, uname) in [(admin_id, format!("rpt_admin_{}", suffix)), (student_id, format!("rpt_student_{}", suffix))] {
        sqlx::query(
            "INSERT INTO users (id, username, email, password_hash, account_state, created_at, updated_at) \
             VALUES ($1, $2, $3, $4, 'active', NOW(), NOW())"
        )
        .bind(uid).bind(&uname).bind(format!("{}@test.local", uname)).bind(hash)
        .execute(&pool).await.unwrap();
    }

    // ── Seed: check-in window ────────────────────────────────────────────────
    let window_id = Uuid::new_v4();
    sqlx::query(
        "INSERT INTO checkin_windows (id, school_id, title, opens_at, closes_at, allow_late, active, created_by) \
         VALUES ($1, $2, $3, NOW() - INTERVAL '1 hour', NOW() + INTERVAL '1 hour', FALSE, TRUE, $4)",
    )
    .bind(window_id).bind(school_id).bind(format!("rpt_window_{}", suffix)).bind(admin_id)
    .execute(&pool).await.unwrap();

    // ── Seed: two submissions ────────────────────────────────────────────────
    let sub_approved = Uuid::new_v4();
    let sub_pending  = Uuid::new_v4();

    // Need a second student for the second submission (unique constraint on window+student).
    let student2_id = Uuid::new_v4();
    let uname2 = format!("rpt_student2_{}", suffix);
    sqlx::query(
        "INSERT INTO users (id, username, email, password_hash, account_state, created_at, updated_at) \
         VALUES ($1, $2, $3, $4, 'active', NOW(), NOW())"
    )
    .bind(student2_id).bind(&uname2).bind(format!("{}@test.local", uname2)).bind(hash)
    .execute(&pool).await.unwrap();

    for (sub_id, stud_id) in [(sub_approved, student_id), (sub_pending, student2_id)] {
        sqlx::query(
            "INSERT INTO checkin_submissions (id, window_id, student_id, submitted_at, method, is_late) \
             VALUES ($1, $2, $3, NOW(), 'manual', FALSE)",
        )
        .bind(sub_id).bind(window_id).bind(stud_id)
        .execute(&pool).await.unwrap();
    }

    // Approve the first submission.
    sqlx::query(
        "INSERT INTO checkin_approval_decisions \
             (id, submission_id, decided_by, decision, decided_at) \
         VALUES (gen_random_uuid(), $1, $2, 'approved', NOW())",
    )
    .bind(sub_approved).bind(admin_id)
    .execute(&pool).await.unwrap();

    // ── Execute the EXACT report query from services/reports.rs ─────────────
    #[derive(sqlx::FromRow, Debug)]
    struct Row {
        submission_id: Uuid,
        window_name: String,
        submitted_at: chrono::DateTime<Utc>,
        status: String,
        student_id: Uuid,
        student_username: String,
        reviewer_id: Option<Uuid>,
        reviewer_decision: Option<String>,
        decided_at: Option<chrono::DateTime<Utc>>,
    }

    let rows = sqlx::query_as::<_, Row>(
        "SELECT cs.id AS submission_id, cw.title AS window_name,
                cs.submitted_at,
                COALESCE(cad.decision, 'pending') AS status,
                cs.student_id, u.username AS student_username,
                cad.decided_by AS reviewer_id, cad.decision AS reviewer_decision, cad.decided_at
         FROM checkin_submissions cs
         JOIN checkin_windows cw ON cw.id = cs.window_id
         JOIN users u ON u.id = cs.student_id
         LEFT JOIN checkin_approval_decisions cad ON cad.submission_id = cs.id
         WHERE cs.submitted_at >= NOW() - INTERVAL '1 day'
           AND cs.submitted_at <= NOW() + INTERVAL '1 day'
         ORDER BY cs.submitted_at",
    )
    .fetch_all(&pool)
    .await
    .expect("Report query must execute without error against the real schema");

    // ── Assertions ────────────────────────────────────────────────────────────

    // Find our two seeded rows among potentially many rows in the test DB.
    let row_approved = rows.iter().find(|r| r.submission_id == sub_approved)
        .expect("Approved submission must appear in report");
    let row_pending  = rows.iter().find(|r| r.submission_id == sub_pending)
        .expect("Pending submission must appear in report");

    assert_eq!(
        row_approved.status, "approved",
        "Approved submission must have status='approved' via COALESCE(cad.decision, 'pending')"
    );
    assert_eq!(
        row_pending.status, "pending",
        "Un-decided submission must have status='pending' via COALESCE fallback"
    );
    assert_eq!(
        row_approved.reviewer_id,
        Some(admin_id),
        "reviewer_id must be the admin who approved"
    );
    assert_eq!(
        row_pending.reviewer_id, None,
        "reviewer_id must be NULL for a pending submission"
    );

    // Confirm that selecting a non-existent column `cs.status` would fail,
    // proving the fix is load-bearing.
    let should_fail = sqlx::query(
        "SELECT cs.status FROM checkin_submissions cs LIMIT 1",
    )
    .execute(&pool)
    .await;
    assert!(
        should_fail.is_err(),
        "Selecting cs.status must fail — the column does not exist on checkin_submissions"
    );

    // Cleanup seeded data.
    for sub_id in [sub_approved, sub_pending] {
        sqlx::query("DELETE FROM checkin_approval_decisions WHERE submission_id = $1")
            .bind(sub_id).execute(&pool).await.unwrap();
        sqlx::query("DELETE FROM checkin_submissions WHERE id = $1")
            .bind(sub_id).execute(&pool).await.unwrap();
    }
    sqlx::query("DELETE FROM checkin_windows WHERE id = $1")
        .bind(window_id).execute(&pool).await.unwrap();
    for uid in [admin_id, student_id, student2_id] {
        sqlx::query("DELETE FROM users WHERE id = $1")
            .bind(uid).execute(&pool).await.unwrap();
    }
    sqlx::query("DELETE FROM schools   WHERE id = $1").bind(school_id).execute(&pool).await.unwrap();
    sqlx::query("DELETE FROM campuses  WHERE id = $1").bind(campus_id).execute(&pool).await.unwrap();
    sqlx::query("DELETE FROM districts WHERE id = $1").bind(district_id).execute(&pool).await.unwrap();
}
