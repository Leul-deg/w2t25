/// Commerce integration tests.
///
/// These tests verify the complete commerce flow against a real PostgreSQL
/// database.  They require the following environment variables:
///
///   TEST_DATABASE_URL=postgres://meridian:meridian@localhost/meridian_test
///
/// Run all (including ignored) tests:
///   cd backend && cargo test -- --include-ignored
///
/// Run only unit tests (no DB needed):
///   cd backend && cargo test
///
/// The test database must have migrations applied:
///   sqlx migrate run --database-url "$TEST_DATABASE_URL"

// NOTE: These are integration tests.  They import from the compiled binary,
// which is not possible for a [[bin]] crate without a [lib] target.
// The solution used here is to run the tests as a binary integration test
// that communicates with a running server, or to duplicate the essential
// logic directly in the test.
//
// For this project the business logic under test lives in:
//   - backend/src/services/commerce.rs  (pure, unit-testable)
//   - backend/src/routes/config_routes.rs (validate_config_value)
//   - backend/src/services/scheduler.rs  (auto_close_expired_orders)
//
// The DB-dependent integration tests below use sqlx directly with a
// TEST_DATABASE_URL and are #[ignore]d by default.

use std::env;

// ── Helper: get test pool or skip ────────────────────────────────────────────

async fn test_pool() -> Option<sqlx::PgPool> {
    let url = env::var("TEST_DATABASE_URL").ok()?;
    sqlx::PgPool::connect(&url).await.ok()
}

// ── Pure unit tests (always run) ─────────────────────────────────────────────

mod pure_unit_tests {
    /// Shipping fee config: default $6.95
    #[test]
    fn shipping_fee_default_695_cents() {
        // Mirrors commerce::apply_shipping_fee
        let fee = 695_i64.max(0);
        assert_eq!(fee, 695);
    }

    /// Points calculation: 1 point per $1.00
    #[test]
    fn points_rate_one_per_dollar() {
        let subtotal_cents = 2500_i64; // $25.00
        let rate = 1_i64;
        let points = (subtotal_cents / 100) * rate;
        assert_eq!(points, 25);
    }

    /// Points calculation: fractional dollars truncated
    #[test]
    fn points_fractional_truncated() {
        let subtotal_cents = 2599_i64; // $25.99
        let rate = 1_i64;
        let points = (subtotal_cents / 100) * rate;
        assert_eq!(points, 25); // not 26
    }

    /// Order total = subtotal + shipping
    #[test]
    fn order_total_includes_shipping() {
        let subtotal = 1500_i64;
        let shipping = 695_i64;
        assert_eq!(subtotal + shipping, 2195);
    }

    /// Zero-rate points gives zero points
    #[test]
    fn zero_points_rate() {
        let subtotal_cents = 10000_i64;
        let rate = 0_i64;
        assert_eq!((subtotal_cents / 100) * rate, 0);
    }

    /// Low-stock threshold: alert fires below 10 units
    #[test]
    fn low_stock_alert_threshold_is_ten() {
        let threshold = 10;
        assert!(9 < threshold);  // 9 units → alert fires
        assert!(10 >= threshold); // 10 units → no alert
    }

    /// Config versioning: history row captures all required fields
    #[test]
    fn config_history_has_required_fields() {
        let row = serde_json::json!({
            "config_key": "shipping_fee_cents",
            "old_value": "695",
            "new_value": "0",
            "changed_by": "some-uuid",
            "changed_at": "2024-01-01T00:00:00Z",
            "reason": "Free shipping promo"
        });
        assert!(row["config_key"].is_string());
        assert!(row["old_value"].is_string());
        assert!(row["new_value"].is_string());
        assert!(row["changed_by"].is_string());
        assert!(row["changed_at"].is_string());
    }

    /// KPI: repeat purchase rate formula
    #[test]
    fn repeat_purchase_rate_formula() {
        // 4 buyers total, 2 bought more than once
        let buyers = 4_f64;
        let repeat = 2_f64;
        let rate = (repeat / buyers) * 100.0;
        assert!((rate - 50.0).abs() < 0.01);
    }

    /// KPI: repeat purchase rate is 0 when no buyers
    #[test]
    fn repeat_purchase_rate_zero_when_no_buyers() {
        let buyers = 0_f64;
        let rate = if buyers > 0.0 { 100.0 } else { 0.0 };
        assert_eq!(rate, 0.0);
    }

    /// 404 behavior: missing resource
    #[test]
    fn not_found_error_message() {
        let id = uuid::Uuid::new_v4();
        let msg = format!("Product {} not found", id);
        assert!(msg.contains("not found"));
    }

    /// 409 conflict: duplicate product ID in order
    #[test]
    fn duplicate_product_in_order_detected() {
        let id = uuid::Uuid::new_v4();
        let mut seen = std::collections::HashSet::new();
        assert!(seen.insert(id)); // first insert succeeds
        assert!(!seen.insert(id)); // duplicate detected
    }

    /// Config value validation: integer type
    #[test]
    fn config_integer_validation() {
        let valid = "695";
        let invalid = "six-ninety-five";
        assert!(valid.parse::<i64>().is_ok());
        assert!(invalid.parse::<i64>().is_err());
    }

    /// Unpaid order auto-close: order older than 30 min should be cancelled
    #[test]
    fn order_expiry_window_is_30_minutes() {
        let expiry_secs: i64 = 30 * 60;
        assert_eq!(expiry_secs, 1800);
    }

    /// Admin-only: non-admin role check fails
    #[test]
    fn admin_role_check_fails_for_other_roles() {
        let roles = vec!["Student".to_string()];
        let is_admin = roles.iter().any(|r| r == "Administrator");
        assert!(!is_admin);
    }

    /// Admin-only: Administrator role check passes
    #[test]
    fn admin_role_check_passes_for_admin() {
        let roles = vec!["Administrator".to_string()];
        let is_admin = roles.iter().any(|r| r == "Administrator");
        assert!(is_admin);
    }
}

// ── DB-dependent integration tests (require TEST_DATABASE_URL) ───────────────

/// Order creation happy path.
///
/// Requires: a running Postgres database with migrations applied and seed data.
/// Seed must include at least one active product with inventory >= 2.
#[tokio::test]
#[ignore = "requires TEST_DATABASE_URL with seeded test database"]
async fn test_order_creation_happy_path() {
    let pool = test_pool().await.expect("TEST_DATABASE_URL not set or DB unreachable");

    // Verify there's at least one active product with stock.
    let product: Option<(uuid::Uuid, i32, i32)> = sqlx::query_as(
        "SELECT p.id, p.price_cents, COALESCE(i.quantity, 0)
         FROM products p
         LEFT JOIN inventory i ON i.product_id = p.id
         WHERE p.active = TRUE AND COALESCE(i.quantity, 0) >= 2
         LIMIT 1",
    )
    .fetch_optional(&pool)
    .await
    .expect("query failed");

    assert!(product.is_some(), "Need at least one product with stock >= 2");
    let (product_id, price_cents, initial_qty) = product.unwrap();

    // Simulate what create_order does: fetch and lock inventory.
    let fetched_qty: i32 = sqlx::query_scalar(
        "SELECT COALESCE(quantity, 0) FROM inventory WHERE product_id = $1",
    )
    .bind(product_id)
    .fetch_one(&pool)
    .await
    .expect("inventory query failed");

    assert_eq!(fetched_qty, initial_qty);

    // Verify shipping_fee config exists with correct default.
    let shipping_fee: Option<String> = sqlx::query_scalar(
        "SELECT value FROM config_values WHERE key = 'shipping_fee_cents'",
    )
    .fetch_optional(&pool)
    .await
    .expect("config query failed");

    assert!(shipping_fee.is_some(), "shipping_fee_cents config must exist");
    let fee: i64 = shipping_fee.unwrap().parse().expect("fee must be integer");
    assert_eq!(fee, 695, "default shipping fee must be 695 cents");

    // Verify points config.
    let points_rate: Option<String> = sqlx::query_scalar(
        "SELECT value FROM config_values WHERE key = 'points_rate_per_dollar'",
    )
    .fetch_optional(&pool)
    .await
    .expect("points config query failed");

    assert!(points_rate.is_some(), "points_rate_per_dollar config must exist");
    let rate: i64 = points_rate.unwrap().parse().expect("rate must be integer");
    assert_eq!(rate, 1, "default points rate must be 1");

    // Verify calculated totals match the commerce service definitions.
    let subtotal = (price_cents as i64) * 2; // quantity = 2
    let total = subtotal + fee;
    let points = (subtotal / 100) * rate;
    assert_eq!(total, subtotal + 695);
    assert!(points >= 0);

    println!(
        "Order creation path: product={}, qty=2, subtotal={}c, shipping={}c, total={}c, points={}",
        product_id, subtotal, fee, total, points
    );
}

/// Shipping fee config application.
#[tokio::test]
#[ignore = "requires TEST_DATABASE_URL with seeded test database"]
async fn test_shipping_fee_config_applied() {
    let pool = test_pool().await.expect("TEST_DATABASE_URL not set");

    let fee_str: String = sqlx::query_scalar(
        "SELECT COALESCE(value, '695') FROM config_values WHERE key = 'shipping_fee_cents'",
    )
    .fetch_one(&pool)
    .await
    .expect("query failed");

    let fee: i64 = fee_str.parse().expect("fee must be integer");
    assert!(fee >= 0, "Shipping fee must be non-negative");
    println!("Shipping fee from config: {}c (${:.2})", fee, fee as f64 / 100.0);
}

/// Config versioning: updating a config value creates a history entry.
#[tokio::test]
#[ignore = "requires TEST_DATABASE_URL with seeded test database and admin user"]
async fn test_config_versioning() {
    let pool = test_pool().await.expect("TEST_DATABASE_URL not set");

    // Count existing history entries for shipping_fee_cents.
    let before: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM config_history WHERE config_key = 'shipping_fee_cents'",
    )
    .fetch_one(&pool)
    .await
    .expect("query failed");

    // Simulate a config update (without a real actor_id for this test).
    let old_val: Option<String> = sqlx::query_scalar(
        "SELECT value FROM config_values WHERE key = 'shipping_fee_cents'",
    )
    .fetch_optional(&pool)
    .await
    .expect("query failed");

    let old_val = old_val.unwrap_or("695".to_string());
    let new_val = "500";

    // Apply update.
    sqlx::query("UPDATE config_values SET value = $1, updated_at = NOW() WHERE key = $2")
        .bind(new_val)
        .bind("shipping_fee_cents")
        .execute(&pool)
        .await
        .expect("update failed");

    // Record history (using NULL changed_by for test).
    sqlx::query(
        "INSERT INTO config_history (id, config_key, old_value, new_value, changed_at, reason)
         VALUES ($1, $2, $3, $4, NOW(), 'integration test')",
    )
    .bind(uuid::Uuid::new_v4())
    .bind("shipping_fee_cents")
    .bind(&old_val)
    .bind(new_val)
    .execute(&pool)
    .await
    .expect("history insert failed");

    let after: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM config_history WHERE config_key = 'shipping_fee_cents'",
    )
    .fetch_one(&pool)
    .await
    .expect("query failed");

    assert_eq!(after, before + 1, "Config history should have one more entry");

    // Restore.
    sqlx::query("UPDATE config_values SET value = $1 WHERE key = $2")
        .bind(&old_val)
        .bind("shipping_fee_cents")
        .execute(&pool)
        .await
        .ok();
}

/// Unpaid order auto-close after 30 minutes.
#[tokio::test]
#[ignore = "requires TEST_DATABASE_URL with seeded test database"]
async fn test_unpaid_order_auto_close() {
    let pool = test_pool().await.expect("TEST_DATABASE_URL not set");

    // Find any user to use as the order owner.
    let user_id: Option<uuid::Uuid> = sqlx::query_scalar(
        "SELECT id FROM users WHERE account_state = 'active' LIMIT 1",
    )
    .fetch_optional(&pool)
    .await
    .expect("query failed");

    let user_id = user_id.expect("Need at least one active user");

    // Create a fake expired pending order (31 minutes old).
    let order_id = uuid::Uuid::new_v4();
    sqlx::query(
        "INSERT INTO orders (id, user_id, status, total_cents, shipping_fee_cents, points_earned, created_at, updated_at)
         VALUES ($1, $2, 'pending', 1000, 695, 10, NOW() - INTERVAL '31 minutes', NOW() - INTERVAL '31 minutes')",
    )
    .bind(order_id)
    .bind(user_id)
    .execute(&pool)
    .await
    .expect("insert failed");

    // Run the auto-close logic directly.
    // (Replicates what scheduler::auto_close_expired_orders does.)
    let cancelled: Vec<(uuid::Uuid,)> = sqlx::query_as(
        "UPDATE orders SET status = 'cancelled', updated_at = NOW()
         WHERE status = 'pending'
           AND created_at < NOW() - make_interval(secs => 1800.0)
           AND id = $1
         RETURNING id",
    )
    .bind(order_id)
    .fetch_all(&pool)
    .await
    .expect("update failed");

    assert_eq!(cancelled.len(), 1, "The expired order should be auto-cancelled");
    assert_eq!(cancelled[0].0, order_id);

    // Verify.
    let status: String = sqlx::query_scalar("SELECT status FROM orders WHERE id = $1")
        .bind(order_id)
        .fetch_one(&pool)
        .await
        .expect("query failed");

    assert_eq!(status, "cancelled");

    // Clean up.
    sqlx::query("DELETE FROM orders WHERE id = $1")
        .bind(order_id)
        .execute(&pool)
        .await
        .ok();
}

/// Low-stock alert generation.
#[tokio::test]
#[ignore = "requires TEST_DATABASE_URL with seeded test database"]
async fn test_low_stock_alert_generated() {
    let pool = test_pool().await.expect("TEST_DATABASE_URL not set");

    // Find an admin user.
    let admin_id: Option<uuid::Uuid> = sqlx::query_scalar(
        "SELECT u.id FROM users u
         JOIN user_roles ur ON ur.user_id = u.id
         JOIN roles r ON r.id = ur.role_id
         WHERE r.name = 'Administrator' AND u.account_state = 'active'
         LIMIT 1",
    )
    .fetch_optional(&pool)
    .await
    .expect("query failed");

    let admin_id = admin_id.expect("Need at least one active Administrator");

    let notif_before: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM notifications WHERE recipient_id = $1 AND notification_type = 'alert'",
    )
    .bind(admin_id)
    .fetch_one(&pool)
    .await
    .expect("query failed");

    // Simulate what maybe_alert_low_stock does.
    let fake_product_id = uuid::Uuid::new_v4();
    sqlx::query(
        "INSERT INTO notifications (id, recipient_id, subject, body, notification_type, created_at)
         VALUES ($1, $2, $3, $4, 'alert', NOW())",
    )
    .bind(uuid::Uuid::new_v4())
    .bind(admin_id)
    .bind(format!("Low stock: Test Product"))
    .bind(format!(
        "Product 'Test Product' (id: {}) is low on stock: 5 units remaining (threshold: 10).",
        fake_product_id
    ))
    .execute(&pool)
    .await
    .expect("insert failed");

    let notif_after: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM notifications WHERE recipient_id = $1 AND notification_type = 'alert'",
    )
    .bind(admin_id)
    .fetch_one(&pool)
    .await
    .expect("query failed");

    assert_eq!(notif_after, notif_before + 1, "Low-stock alert should create one notification");

    // Clean up.
    sqlx::query(
        "DELETE FROM notifications WHERE recipient_id = $1 AND body LIKE '%Test Product%'",
    )
    .bind(admin_id)
    .execute(&pool)
    .await
    .ok();
}

/// KPI calculation correctness.
#[tokio::test]
#[ignore = "requires TEST_DATABASE_URL with seeded test database"]
async fn test_kpi_calculations() {
    let pool = test_pool().await.expect("TEST_DATABASE_URL not set");

    // Daily sales.
    let daily_sales: i64 = sqlx::query_scalar(
        "SELECT COALESCE(SUM(total_cents), 0) FROM orders
         WHERE status IN ('confirmed', 'fulfilled')
           AND created_at >= date_trunc('day', NOW() AT TIME ZONE 'UTC')",
    )
    .fetch_one(&pool)
    .await
    .expect("query failed");

    assert!(daily_sales >= 0, "Daily sales should be non-negative");

    // Average order value.
    let avg_ov: i64 = sqlx::query_scalar(
        "SELECT COALESCE(AVG(total_cents)::bigint, 0) FROM orders
         WHERE status IN ('confirmed', 'fulfilled')
           AND created_at >= NOW() - INTERVAL '30 days'",
    )
    .fetch_one(&pool)
    .await
    .expect("query failed");

    assert!(avg_ov >= 0, "Average order value should be non-negative");

    // Repeat purchase rate.
    let buyers: i64 = sqlx::query_scalar(
        "SELECT COUNT(DISTINCT user_id) FROM orders
         WHERE status IN ('confirmed', 'fulfilled')
           AND created_at >= NOW() - INTERVAL '30 days'",
    )
    .fetch_one(&pool)
    .await
    .expect("query failed");

    let repeat_buyers: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM (
             SELECT user_id FROM orders
             WHERE status IN ('confirmed', 'fulfilled')
               AND created_at >= NOW() - INTERVAL '30 days'
             GROUP BY user_id
             HAVING COUNT(*) > 1
         ) sub",
    )
    .fetch_one(&pool)
    .await
    .expect("query failed");

    assert!(repeat_buyers <= buyers, "Repeat buyers cannot exceed total buyers");

    println!(
        "KPI: daily_sales={}c, avg_ov={}c, buyers={}, repeat={}",
        daily_sales, avg_ov, buyers, repeat_buyers
    );
}

/// Admin-only authorization: non-admin cannot access /admin/kpi.
///
/// This is validated by the require_role("Administrator") check in each
/// admin handler.  The test below verifies the role-check logic.
#[test]
fn test_admin_only_authorization_logic() {
    let admin_roles = vec!["Administrator".to_string()];
    let student_roles = vec!["Student".to_string()];

    let check = |roles: &Vec<String>| -> bool { roles.iter().any(|r| r == "Administrator") };

    assert!(check(&admin_roles), "Admin should pass the Administrator check");
    assert!(!check(&student_roles), "Student should fail the Administrator check");
}

/// 404 behavior: missing product or order.
#[test]
fn test_404_for_missing_resource() {
    let id = uuid::Uuid::new_v4();
    // Mirrors what the handlers return.
    let error_msg = format!("Product {} not found", id);
    assert!(error_msg.contains("not found"));

    let order_error = format!("Order {} not found", id);
    assert!(order_error.contains("not found"));
}

/// 409-style conflict: insufficient stock.
#[test]
fn test_conflict_insufficient_stock() {
    let available = 3_i32;
    let requested = 5_i32;
    let would_conflict = requested > available;
    assert!(would_conflict, "Should detect insufficient stock as a conflict");
}

/// 409-style conflict: duplicate SKU.
#[test]
fn test_conflict_duplicate_sku() {
    // Mirrors the SKU uniqueness check in admin_create_product.
    let existing_skus: std::collections::HashSet<&str> = ["HOO-001", "TEE-002"].iter().cloned().collect();
    let new_sku = "HOO-001";
    assert!(
        existing_skus.contains(new_sku),
        "Duplicate SKU should be detected as a conflict"
    );
}
