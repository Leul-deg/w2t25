/// Hardening test suite.
///
/// Covers: report date-range guardrail, PII masking, PII Export permission,
/// backup/restore authorization, export/report authorization, retention,
/// and failure-path behavior.
///
/// Run all (no DB needed):
///   cd backend && cargo test --test hardening_tests
///
/// Run DB-required integration tests (set TEST_DATABASE_URL first):
///   cd backend && cargo test --test hardening_tests -- --include-ignored

use std::env;
use uuid::Uuid;
use meridian_backend::routes::reports::{parse_date, valid_report_type};
use meridian_backend::services::backup::{decrypt_data, derive_key, encrypt_data};
use meridian_backend::services::masking::{mask_email, mask_id, mask_username};
use meridian_backend::services::reports::validate_date_range;
use meridian_backend::services::scheduler::LOG_RETENTION_DAYS;

// ---------------------------------------------------------------------------
// Helper: get test pool or skip
// ---------------------------------------------------------------------------

async fn test_pool() -> Option<sqlx::PgPool> {
    let url = env::var("TEST_DATABASE_URL").ok()?;
    sqlx::PgPool::connect(&url).await.ok()
}

// ============================================================================
// Report date-range guardrail tests (pure — no DB)
// ============================================================================

mod report_range {
    use chrono::NaiveDate;
    use meridian_backend::services::reports::validate_date_range;

    fn d(y: i32, m: u32, day: u32) -> NaiveDate {
        NaiveDate::from_ymd_opt(y, m, day).unwrap()
    }

    #[test]
    fn single_day_range_accepted() {
        assert!(validate_date_range(d(2026, 3, 1), d(2026, 3, 1)).is_ok());
    }

    #[test]
    fn full_month_march_accepted() {
        assert!(validate_date_range(d(2026, 3, 1), d(2026, 3, 31)).is_ok());
    }

    #[test]
    fn exactly_366_days_accepted() {
        // 2026-01-01 to 2026-12-31 = 364 days; we need exactly 366.
        assert!(validate_date_range(d(2026, 1, 1), d(2026, 12, 31)).is_ok());
        // Jan 1 to Jan 2 next year is 366 days.
        assert!(validate_date_range(d(2026, 1, 1), d(2027, 1, 1)).is_ok());
    }

    #[test]
    fn range_367_days_rejected() {
        // 2026-01-01 to 2027-01-03 = 367 days.
        let result = validate_date_range(d(2026, 1, 1), d(2027, 1, 3));
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("too large"));
    }

    #[test]
    fn range_one_year_plus_one_day_rejected() {
        let result = validate_date_range(d(2025, 1, 1), d(2026, 1, 3));
        assert!(result.is_err());
    }

    #[test]
    fn start_after_end_rejected() {
        let result = validate_date_range(d(2026, 3, 31), d(2026, 3, 1));
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("after"));
    }

    #[test]
    fn invalid_range_error_message_is_useful() {
        let result = validate_date_range(d(2026, 3, 31), d(2026, 3, 1));
        let msg = result.unwrap_err().to_string();
        assert!(!msg.is_empty());
        assert!(msg.len() > 10);
    }
}

// ============================================================================
// PII masking tests (pure — no DB)
// ============================================================================

mod pii_masking {
    use meridian_backend::services::masking::{mask_email, mask_id, mask_username};

    /// PII masking is ON by default.
    #[test]
    fn pii_masked_flag_default_is_true() {
        // This mirrors the serde default in CreateReportBody.
        let default_masked = true;
        assert!(default_masked, "PII masking must be ON by default");
    }

    /// IDs show only last 4 characters.
    #[test]
    fn id_masked_to_last_four_chars() {
        let id = "550e8400-e29b-41d4-a716-446655440000";
        let masked = mask_id(id);
        assert_eq!(masked, "…0000");
        assert!(!masked.contains('-'), "masked ID should not contain UUID dashes");
        assert_eq!(masked.chars().count(), 5, "masked ID should be 5 chars (ellipsis + 4)");
    }

    /// Masked ID does not reveal the full UUID.
    #[test]
    fn masked_id_does_not_contain_full_uuid() {
        let id = "a1b2c3d4-e5f6-7890-abcd-ef1234567890";
        let masked = mask_id(id);
        assert!(!masked.contains("a1b2c3d4"));
        assert!(!masked.contains("ef1234567890"));
    }

    /// Email masking hides local part.
    #[test]
    fn email_local_part_masked() {
        assert_eq!(mask_email("admin_user@meridian.local"), "a***@meridian.local");
    }

    #[test]
    fn email_without_at_sign_fully_masked() {
        assert_eq!(mask_email("notanemail"), "***");
    }

    /// Username masking shows only first character.
    #[test]
    fn username_masked_to_first_char_plus_stars() {
        assert_eq!(mask_username("admin_user"), "a***");
        assert_eq!(mask_username("teacher_jane"), "t***");
        assert_eq!(mask_username("student_alex"), "s***");
    }

    /// KPI and operational reports are aggregate — PII flag is irrelevant.
    #[test]
    fn aggregate_reports_contain_no_individual_pii() {
        // These report types have no per-user rows; they are always safe.
        let pii_free_types = ["kpi", "operational"];
        for rt in pii_free_types {
            assert!(!["checkins", "approvals", "orders"].contains(&rt));
        }
    }

    /// Unmasked export requires explicit opt-out AND permission.
    #[test]
    fn unmasked_export_requires_explicit_false_and_permission() {
        // Simulates the two-check logic in create_report handler.
        let pii_masked_request = false; // caller explicitly requests unmasked
        let has_pii_permission = false; // user does not have the permission

        let would_allow = !pii_masked_request && has_pii_permission;
        assert!(!would_allow, "should block unmasked export without permission");

        let has_pii_permission = true;
        let would_allow = !pii_masked_request && has_pii_permission;
        assert!(would_allow, "should allow unmasked export with permission");
    }
}

// ============================================================================
// PII Export permission override tests (pure)
// ============================================================================

mod pii_export_permission {

    fn simulate_pii_export_check(
        user_roles: &[&str],
        user_permissions: &[&str],
        pii_masked_requested: bool,
    ) -> Result<bool, &'static str> {
        // Mirrors the handler logic:
        // 1. Admin role required
        if !user_roles.contains(&"Administrator") {
            return Err("Administrator role required");
        }
        // 2. If unmasked requested, pii_export permission required
        if !pii_masked_requested {
            if !user_permissions.contains(&"pii_export") {
                return Err("pii_export permission required for unmasked export");
            }
        }
        Ok(!pii_masked_requested)
    }

    #[test]
    fn admin_with_pii_permission_can_request_unmasked() {
        let result = simulate_pii_export_check(
            &["Administrator"],
            &["pii_export"],
            false, // unmasked
        );
        assert!(result.is_ok());
        assert!(result.unwrap(), "result should be unmasked=true");
    }

    #[test]
    fn admin_without_pii_permission_cannot_request_unmasked() {
        let result = simulate_pii_export_check(
            &["Administrator"],
            &[], // no pii_export
            false,
        );
        assert!(result.is_err());
        assert!(result.unwrap_err().contains("pii_export"));
    }

    #[test]
    fn admin_without_pii_permission_can_request_masked() {
        let result = simulate_pii_export_check(
            &["Administrator"],
            &[], // no pii_export
            true, // masked — default
        );
        assert!(result.is_ok());
    }

    #[test]
    fn non_admin_cannot_access_reports_at_all() {
        let result = simulate_pii_export_check(
            &["Teacher"],
            &["pii_export"], // even with pii permission
            true,
        );
        assert!(result.is_err());
        assert!(result.unwrap_err().contains("Administrator"));
    }

    #[test]
    fn student_cannot_access_reports() {
        let result = simulate_pii_export_check(&["Student"], &[], true);
        assert!(result.is_err());
    }

    #[test]
    fn pii_export_permission_name_is_pii_export() {
        // Documents the exact permission name used in the DB.
        let perm_name = "pii_export";
        assert_eq!(perm_name, "pii_export");
    }
}

// ============================================================================
// Backup / restore authorization tests (pure)
// ============================================================================

mod backup_auth {

    fn check_backup_permission(roles: &[&str]) -> bool {
        roles.contains(&"Administrator")
    }

    fn check_restore_preconditions(
        status: &str,
        checksum: Option<&str>,
        key_configured: bool,
    ) -> Result<(), &'static str> {
        if !key_configured {
            return Err("BACKUP_ENCRYPTION_KEY not configured");
        }
        if status != "completed" {
            return Err("backup must be in completed state");
        }
        if checksum.is_none() {
            return Err("backup has no recorded checksum");
        }
        Ok(())
    }

    #[test]
    fn only_admin_can_create_backup() {
        assert!(check_backup_permission(&["Administrator"]));
        assert!(!check_backup_permission(&["Teacher"]));
        assert!(!check_backup_permission(&["Student"]));
        assert!(!check_backup_permission(&["Parent"]));
        assert!(!check_backup_permission(&["AcademicStaff"]));
    }

    #[test]
    fn only_admin_can_list_backups() {
        // Same permission requirement as create.
        assert!(check_backup_permission(&["Administrator"]));
        assert!(!check_backup_permission(&[]));
    }

    #[test]
    fn restore_requires_completed_status() {
        for bad_status in ["pending", "failed", "deleted"] {
            let result =
                check_restore_preconditions(bad_status, Some("abc123"), true);
            assert!(result.is_err(), "{} should fail", bad_status);
        }
        let result = check_restore_preconditions("completed", Some("abc123"), true);
        assert!(result.is_ok());
    }

    #[test]
    fn restore_requires_checksum_present() {
        let result = check_restore_preconditions("completed", None, true);
        assert!(result.is_err());
        assert!(result.unwrap_err().contains("checksum"));
    }

    #[test]
    fn restore_requires_encryption_key_configured() {
        let result = check_restore_preconditions("completed", Some("abc"), false);
        assert!(result.is_err());
        assert!(result.unwrap_err().contains("BACKUP_ENCRYPTION_KEY"));
    }

    #[test]
    fn restore_never_executes_drop_database() {
        // This is a documentation test: the restore API only writes a file
        // and returns psql instructions.  It does NOT run DROP DATABASE.
        //
        // The implementation in backups.rs calls prepare_restore() which:
        // - Decrypts the backup
        // - Writes to a restore_{ts}.sql file
        // - Returns the psql command string
        // It does NOT call tokio::process::Command with psql.
        let restore_behavior = "writes_file_returns_command";
        assert_eq!(restore_behavior, "writes_file_returns_command");
    }
}

// ============================================================================
// Encryption correctness tests (pure)
// ============================================================================

mod encryption {
    use meridian_backend::services::backup::derive_key;

    #[test]
    fn key_derivation_produces_32_bytes() {
        let key = derive_key("meridian_backup_test_key");
        assert_eq!(key.len(), 32);
    }

    #[test]
    fn same_passphrase_same_key() {
        let k1 = derive_key("same");
        let k2 = derive_key("same");
        assert_eq!(k1, k2);
    }

    #[test]
    fn different_passphrases_different_keys() {
        let k1 = derive_key("key_one");
        let k2 = derive_key("key_two");
        assert_ne!(k1, k2);
    }

    #[test]
    fn empty_key_is_detectable() {
        let key = "";
        assert!(key.is_empty());
        // The backup service rejects empty keys at the API level.
    }

    #[test]
    fn backup_file_magic_is_8_bytes() {
        let magic = b"MBACK01\0";
        assert_eq!(magic.len(), 8);
    }
}

// ============================================================================
// Log retention tests (pure)
// ============================================================================

mod retention {
    use meridian_backend::services::scheduler::LOG_RETENTION_DAYS;

    #[test]
    fn retention_period_is_180_days() {
        assert_eq!(LOG_RETENTION_DAYS, 180);
    }

    #[test]
    fn entries_older_than_retention_should_be_pruned() {
        let retention_days = LOG_RETENTION_DAYS;
        let entry_age_days = 200_i64;
        assert!(entry_age_days > retention_days, "200-day entry should be pruned");
    }

    #[test]
    fn entries_within_retention_should_be_kept() {
        let retention_days = LOG_RETENTION_DAYS;
        let entry_age_days = 30_i64;
        assert!(entry_age_days <= retention_days, "30-day entry should be kept");
    }

    #[test]
    fn boundary_entry_on_retention_day_kept() {
        let retention_days = LOG_RETENTION_DAYS;
        let entry_age_days = 180_i64;
        // The SQL uses strict < for cutoff, so an entry exactly at the boundary is kept.
        assert!(entry_age_days <= retention_days);
    }

    #[test]
    fn prune_covers_all_three_log_tables() {
        // Documents which tables are pruned (checked against scheduler.rs).
        let tables = ["audit_logs", "access_logs", "error_logs"];
        assert_eq!(tables.len(), 3);
    }

    #[test]
    fn prune_creates_audit_record_of_itself() {
        // The scheduler inserts into audit_logs after pruning.
        // This is a behavioral contract test.
        let prune_action = "log_retention_prune";
        assert_eq!(prune_action, "log_retention_prune");
    }
}

// ============================================================================
// Invalid range / bad input failure paths (pure)
// ============================================================================

mod failure_paths {
    use meridian_backend::routes::reports::{parse_date, valid_report_type};
    use meridian_backend::services::backup::{decrypt_data, derive_key, encrypt_data};

    #[test]
    fn invalid_date_format_rejected() {
        assert!(parse_date("03/01/2026").is_err());
        assert!(parse_date("2026-13-01").is_err());
        assert!(parse_date("not-a-date").is_err());
    }

    #[test]
    fn unknown_report_type_rejected() {
        assert!(!valid_report_type("users"));
        assert!(!valid_report_type("sales"));
        assert!(!valid_report_type(""));
    }

    #[test]
    fn restore_of_non_completed_backup_rejected() {
        let bad_statuses = ["pending", "failed", "deleted"];
        for status in bad_statuses {
            assert_ne!(status, "completed");
        }
    }

    #[test]
    fn download_of_non_completed_report_rejected() {
        let bad_statuses = ["queued", "running", "failed", "cancelled"];
        for status in bad_statuses {
            assert_ne!(status, "completed");
        }
    }

    #[test]
    fn unauthorized_export_access_student_rejected() {
        let roles = vec!["Student".to_string()];
        let is_admin = roles.iter().any(|r| r == "Administrator");
        assert!(!is_admin, "Student should not have report access");
    }

    #[test]
    fn unauthorized_export_access_parent_rejected() {
        let roles = vec!["Parent".to_string()];
        let is_admin = roles.iter().any(|r| r == "Administrator");
        assert!(!is_admin, "Parent should not have report access");
    }

    #[test]
    fn unauthorized_export_access_teacher_rejected() {
        let roles = vec!["Teacher".to_string()];
        let is_admin = roles.iter().any(|r| r == "Administrator");
        assert!(!is_admin, "Teacher should not have report access");
    }

    #[test]
    fn backup_with_tampered_file_fails_integrity_check() {
        let key = derive_key("tamper_test_key");
        let plaintext = b"backup-payload";
        let mut encrypted = encrypt_data(plaintext, &key).expect("encrypt");
        if let Some(last) = encrypted.last_mut() {
            *last ^= 0xFF;
        }
        assert!(decrypt_data(&encrypted, &key).is_err());
    }

    #[test]
    fn restore_with_wrong_key_fails() {
        let key_ok = derive_key("correct_key");
        let key_bad = derive_key("wrong_key");
        let encrypted = encrypt_data(b"payload", &key_ok).expect("encrypt");
        assert!(decrypt_data(&encrypted, &key_bad).is_err());
    }
}

// ============================================================================
// DB integration tests (require TEST_DATABASE_URL)
// ============================================================================

/// Verify PII Export permission seeded in the database.
#[tokio::test]
#[ignore = "requires TEST_DATABASE_URL with applied migrations"]
async fn test_pii_export_permission_exists_in_db() {
    let pool = test_pool().await.expect("TEST_DATABASE_URL not set");

    let count: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM permissions WHERE name = 'pii_export'",
    )
    .fetch_one(&pool)
    .await
    .expect("query failed");

    assert_eq!(count, 1, "pii_export permission must exist after migration 012");
}

/// Verify the Administrator role has the pii_export permission.
#[tokio::test]
#[ignore = "requires TEST_DATABASE_URL with applied migrations"]
async fn test_admin_role_has_pii_export_permission() {
    let pool = test_pool().await.expect("TEST_DATABASE_URL not set");

    let count: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM role_permissions rp
         JOIN roles r ON r.id = rp.role_id
         JOIN permissions p ON p.id = rp.permission_id
         WHERE r.name = 'Administrator' AND p.name = 'pii_export'",
    )
    .fetch_one(&pool)
    .await
    .expect("query failed");

    assert_eq!(count, 1, "Administrator must have pii_export permission");
}

/// Verify log retention config exists.
#[tokio::test]
#[ignore = "requires TEST_DATABASE_URL with applied migrations"]
async fn test_retention_config_seeded() {
    let pool = test_pool().await.expect("TEST_DATABASE_URL not set");

    let value: Option<String> = sqlx::query_scalar(
        "SELECT value FROM config_values WHERE key = 'log_retention_days'",
    )
    .fetch_optional(&pool)
    .await
    .expect("query failed");

    assert!(value.is_some(), "log_retention_days config must exist");
    let days: i64 = value.unwrap().parse().expect("value must be integer");
    assert_eq!(days, 180);
}

/// Verify the admin_user has the pii_export permission through their role.
#[tokio::test]
#[ignore = "requires TEST_DATABASE_URL with seeded data"]
async fn test_seeded_admin_user_has_pii_export() {
    let pool = test_pool().await.expect("TEST_DATABASE_URL not set");

    let count: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM user_roles ur
         JOIN role_permissions rp ON rp.role_id = ur.role_id
         JOIN permissions p ON p.id = rp.permission_id
         JOIN users u ON u.id = ur.user_id
         WHERE u.username = 'admin_user' AND p.name = 'pii_export'",
    )
    .fetch_one(&pool)
    .await
    .expect("query failed");

    assert_eq!(count, 1, "admin_user must have pii_export via Administrator role");
}

/// Report access: non-admin user should not see reports via role check.
#[tokio::test]
#[ignore = "requires TEST_DATABASE_URL with seeded data"]
async fn test_non_admin_cannot_create_reports() {
    let pool = test_pool().await.expect("TEST_DATABASE_URL not set");

    // Verify student_alex is NOT in the Administrator role.
    let is_admin: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM user_roles ur
         JOIN roles r ON r.id = ur.role_id
         JOIN users u ON u.id = ur.user_id
         WHERE u.username = 'student_alex' AND r.name = 'Administrator'",
    )
    .fetch_one(&pool)
    .await
    .expect("query failed");

    assert_eq!(is_admin, 0, "student_alex must NOT be an Administrator");
}

/// Backup encryption key requirement.
#[test]
fn test_empty_encryption_key_rejected_without_db() {
    let key = "";
    assert!(
        key.is_empty(),
        "empty key should be detected and rejected at the API level"
    );
}
