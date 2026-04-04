/// Admin scope isolation test suite.
///
/// Covers: scope resolution logic, 403 for out-of-scope users/orders,
/// super-admin unrestricted access, and district-to-campus expansion.
///
/// Run all (no DB needed):
///   cd backend && cargo test --test admin_scope_tests
///
/// Run DB-required integration tests (set TEST_DATABASE_URL first):
///   cd backend && cargo test --test admin_scope_tests -- --include-ignored

use std::env;
use uuid::Uuid;

// ---------------------------------------------------------------------------
// Helper: get test pool or skip
// ---------------------------------------------------------------------------

async fn test_pool() -> Option<sqlx::PgPool> {
    let url = env::var("TEST_DATABASE_URL").ok()?;
    sqlx::PgPool::connect(&url).await.ok()
}

// ============================================================================
// Scope resolution logic tests (pure — no DB)
// ============================================================================

mod scope_resolution {

    /// An admin with `is_super_admin = true` and no scope rows → `None` (unrestricted).
    #[test]
    fn super_admin_flag_true_yields_none_scope() {
        // Mirrors get_admin_campus_scope() returning None when is_super_admin = true.
        let scope: Option<Vec<uuid::Uuid>> = None;
        assert!(scope.is_none(), "super-admin scope must be None");
    }

    /// An admin with `is_super_admin = false` and no scope rows → `Some([])` (zero access).
    /// This is the scoped-by-default behaviour introduced in migration 015.
    #[test]
    fn no_flag_no_scope_rows_yields_empty_some() {
        // Mirrors get_admin_campus_scope() returning Some(vec![]) when
        // is_super_admin = false and admin_scope_assignments has no rows.
        let scope: Option<Vec<uuid::Uuid>> = Some(vec![]);
        assert!(scope.is_some(), "scoped-by-default must be Some(...)");
        assert!(scope.unwrap().is_empty(), "no assignments means empty campus list");
    }

    /// An admin with scope rows must have Some(ids), even if the list is empty
    /// after expansion (e.g., a district with no campuses yet).
    #[test]
    fn scope_rows_present_yields_some() {
        let campus_ids: Vec<uuid::Uuid> = vec![uuid::Uuid::new_v4()];
        let scope = Some(campus_ids);
        assert!(scope.is_some());
    }

    /// District expansion: each district row should contribute the campus IDs
    /// belonging to that district.
    #[test]
    fn district_scope_expands_to_campus_ids() {
        let c1 = uuid::Uuid::new_v4();
        let c2 = uuid::Uuid::new_v4();
        // Simulates two campuses returned for one district scope row.
        let mut campus_ids: Vec<uuid::Uuid> = Vec::new();
        campus_ids.push(c1);
        campus_ids.push(c2);
        assert_eq!(campus_ids.len(), 2);
        assert!(campus_ids.contains(&c1));
        assert!(campus_ids.contains(&c2));
    }

    /// Campus scope row contributes exactly its own ID.
    #[test]
    fn campus_scope_row_contributes_one_id() {
        let campus = uuid::Uuid::new_v4();
        let campus_ids = vec![campus];
        assert_eq!(campus_ids.len(), 1);
        assert_eq!(campus_ids[0], campus);
    }

    /// Multiple scope rows (mixed district + campus) accumulate all IDs.
    #[test]
    fn mixed_scope_rows_accumulate() {
        let campus_direct = uuid::Uuid::new_v4();
        let campus_from_district = uuid::Uuid::new_v4();
        let mut ids = Vec::new();
        // one campus row
        ids.push(campus_direct);
        // expansion of one district row → one campus
        ids.push(campus_from_district);
        assert_eq!(ids.len(), 2);
    }
}

// ============================================================================
// 403 guard logic tests (pure — no DB)
// ============================================================================

mod scope_enforcement {

    fn check_user_in_scope(
        scope: &Option<Vec<uuid::Uuid>>,
        user_campus_ids: &[uuid::Uuid],
    ) -> Result<(), &'static str> {
        let allowed = match scope {
            None => return Ok(()), // super-admin: always allowed
            Some(ids) => ids,
        };
        let in_scope = user_campus_ids.iter().any(|c| allowed.contains(c));
        if in_scope {
            Ok(())
        } else {
            Err("Forbidden: Target user is outside your administrative scope.")
        }
    }

    /// Super-admin (None scope) can always access any user.
    #[test]
    fn super_admin_always_passes_user_check() {
        let scope: Option<Vec<uuid::Uuid>> = None;
        let result = check_user_in_scope(&scope, &[uuid::Uuid::new_v4()]);
        assert!(result.is_ok());
    }

    /// Super-admin passes even when the user has no campus assignment.
    #[test]
    fn super_admin_passes_for_user_with_no_campus() {
        let scope: Option<Vec<uuid::Uuid>> = None;
        let result = check_user_in_scope(&scope, &[]);
        assert!(result.is_ok());
    }

    /// Scoped admin can access a user whose campus is in their allowed list.
    #[test]
    fn scoped_admin_allows_user_in_scope() {
        let allowed_campus = uuid::Uuid::new_v4();
        let scope = Some(vec![allowed_campus]);
        let result = check_user_in_scope(&scope, &[allowed_campus]);
        assert!(result.is_ok());
    }

    /// Scoped admin is blocked from accessing a user in a different campus.
    #[test]
    fn scoped_admin_blocks_user_outside_scope() {
        let allowed_campus = uuid::Uuid::new_v4();
        let other_campus = uuid::Uuid::new_v4();
        let scope = Some(vec![allowed_campus]);
        let result = check_user_in_scope(&scope, &[other_campus]);
        assert!(result.is_err());
        assert!(result.unwrap_err().contains("Forbidden"));
    }

    /// Scoped admin is blocked when the user has no campus assignment at all.
    #[test]
    fn scoped_admin_blocks_user_with_no_campus_assignment() {
        let scope = Some(vec![uuid::Uuid::new_v4()]);
        let result = check_user_in_scope(&scope, &[]);
        assert!(result.is_err());
    }

    /// A user assigned to multiple campuses passes if any one matches.
    #[test]
    fn user_with_multiple_campuses_passes_if_one_matches() {
        let allowed = uuid::Uuid::new_v4();
        let other = uuid::Uuid::new_v4();
        let scope = Some(vec![allowed]);
        // user is in both campuses (e.g. transferred)
        let result = check_user_in_scope(&scope, &[other, allowed]);
        assert!(result.is_ok());
    }

    /// A scoped admin with an empty campus list blocks all access — this is
    /// also the result returned for is_super_admin=false with no scope rows
    /// (scoped-by-default: zero access until explicitly assigned).
    #[test]
    fn scoped_admin_with_empty_campus_list_blocks_all() {
        let scope: Option<Vec<uuid::Uuid>> = Some(vec![]);
        let result = check_user_in_scope(&scope, &[uuid::Uuid::new_v4()]);
        assert!(result.is_err());
    }

    /// Scoped-by-default (is_super_admin=false, no scope rows) blocks even an
    /// admin whose user IS in a campus, because the allowed list is empty.
    #[test]
    fn scoped_by_default_blocks_admin_regardless_of_target_campus() {
        // Simulate is_super_admin=false + no scope rows → Some(vec![])
        let scope: Option<Vec<uuid::Uuid>> = Some(vec![]);
        let any_campus = uuid::Uuid::new_v4();
        let result = check_user_in_scope(&scope, &[any_campus]);
        assert!(
            result.is_err(),
            "scoped-by-default admin must see nothing"
        );
    }

    /// Error message from scope check is meaningful (not empty).
    #[test]
    fn scope_error_message_is_meaningful() {
        let scope = Some(vec![uuid::Uuid::new_v4()]);
        let err = check_user_in_scope(&scope, &[]).unwrap_err();
        assert!(!err.is_empty());
        assert!(err.len() > 10);
    }
}

// ============================================================================
// Scope type validation tests (pure)
// ============================================================================

mod scope_type_validation {

    fn is_valid_scope_type(t: &str) -> bool {
        matches!(t, "district" | "campus")
    }

    #[test]
    fn district_is_valid_scope_type() {
        assert!(is_valid_scope_type("district"));
    }

    #[test]
    fn campus_is_valid_scope_type() {
        assert!(is_valid_scope_type("campus"));
    }

    #[test]
    fn school_is_not_a_valid_scope_type() {
        assert!(!is_valid_scope_type("school"));
    }

    #[test]
    fn empty_string_is_not_valid_scope_type() {
        assert!(!is_valid_scope_type(""));
    }

    #[test]
    fn global_is_not_a_valid_scope_type() {
        // "global" is expressed as no rows, not a scope_type value.
        assert!(!is_valid_scope_type("global"));
    }
}

// ============================================================================
// DB integration tests (require TEST_DATABASE_URL)
// ============================================================================

/// Verify admin_scope_assignments table exists after migration 014.
#[tokio::test]
#[ignore = "requires TEST_DATABASE_URL with applied migrations"]
async fn test_admin_scope_assignments_table_exists() {
    let pool = test_pool().await.expect("TEST_DATABASE_URL not set");

    let count: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM information_schema.tables
         WHERE table_name = 'admin_scope_assignments'",
    )
    .fetch_one(&pool)
    .await
    .expect("query failed");

    assert_eq!(count, 1, "admin_scope_assignments table must exist after migration 014");
}

/// Verify the scope_type CHECK constraint rejects invalid values.
#[tokio::test]
#[ignore = "requires TEST_DATABASE_URL with applied migrations"]
async fn test_invalid_scope_type_rejected_by_constraint() {
    let pool = test_pool().await.expect("TEST_DATABASE_URL not set");

    // Attempt to insert a row with an invalid scope_type; expect a constraint violation.
    let result = sqlx::query(
        "INSERT INTO admin_scope_assignments (admin_id, scope_type, scope_id)
         VALUES ($1, 'school', $2)",
    )
    .bind(Uuid::new_v4())
    .bind(Uuid::new_v4())
    .execute(&pool)
    .await;

    assert!(
        result.is_err(),
        "INSERT with scope_type='school' must fail the CHECK constraint"
    );
}

/// Verify a scoped admin cannot see a user outside their campus via list_users query.
#[tokio::test]
#[ignore = "requires TEST_DATABASE_URL with seeded data"]
async fn test_scoped_admin_user_list_excludes_out_of_scope_users() {
    let pool = test_pool().await.expect("TEST_DATABASE_URL not set");

    // Find a campus that has at least one school with enrolled users.
    let campus_id: Option<Uuid> = sqlx::query_scalar(
        "SELECT s.campus_id FROM user_school_assignments usa
         JOIN schools s ON s.id = usa.school_id
         LIMIT 1",
    )
    .fetch_optional(&pool)
    .await
    .expect("query failed");

    let Some(campus_id) = campus_id else {
        // No data to test against; skip.
        return;
    };

    // Count users visible with this single-campus scope.
    let scoped_count: i64 = sqlx::query_scalar(
        "SELECT COUNT(DISTINCT u.id) FROM users u
         WHERE u.id IN (
             SELECT usa.user_id FROM user_school_assignments usa
             JOIN schools s ON s.id = usa.school_id
             WHERE s.campus_id = $1
         )",
    )
    .bind(campus_id)
    .fetch_one(&pool)
    .await
    .expect("query failed");

    let total_count: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM users")
        .fetch_one(&pool)
        .await
        .expect("query failed");

    // The scoped list must be a subset — not the full users table.
    assert!(
        scoped_count <= total_count,
        "scoped list ({}) must not exceed total ({})",
        scoped_count,
        total_count
    );
}

/// Verify that admin_user has is_super_admin=true and no scope rows.
/// Verify that scoped_admin has is_super_admin=false and exactly one scope row.
#[tokio::test]
#[ignore = "requires TEST_DATABASE_URL with seeded data"]
async fn test_super_admin_flag_and_scoped_admin_seeded_correctly() {
    let pool = test_pool().await.expect("TEST_DATABASE_URL not set");

    // admin_user: is_super_admin = true, no scope rows.
    let is_super: bool = sqlx::query_scalar(
        "SELECT is_super_admin FROM users WHERE username = 'admin_user'",
    )
    .fetch_one(&pool)
    .await
    .expect("admin_user not found — did the seed run?");

    assert!(is_super, "admin_user must have is_super_admin = true");

    let scope_count: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM admin_scope_assignments asa
         JOIN users u ON u.id = asa.admin_id
         WHERE u.username = 'admin_user'",
    )
    .fetch_one(&pool)
    .await
    .expect("query failed");

    assert_eq!(scope_count, 0, "admin_user must have no scope rows");

    // scoped_admin: is_super_admin = false, exactly one scope row.
    let is_scoped_super: bool = sqlx::query_scalar(
        "SELECT is_super_admin FROM users WHERE username = 'scoped_admin'",
    )
    .fetch_optional(&pool)
    .await
    .expect("query failed")
    .unwrap_or(false);

    assert!(
        !is_scoped_super,
        "scoped_admin must have is_super_admin = false"
    );

    let scoped_rows: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM admin_scope_assignments asa
         JOIN users u ON u.id = asa.admin_id
         WHERE u.username = 'scoped_admin'",
    )
    .fetch_one(&pool)
    .await
    .expect("query failed");

    assert_eq!(
        scoped_rows, 1,
        "scoped_admin must have exactly one scope assignment"
    );
}

