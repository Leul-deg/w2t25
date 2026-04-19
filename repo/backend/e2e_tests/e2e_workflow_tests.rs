use actix_web::test::{call_service, init_service, read_body_json, TestRequest};
use actix_web::{web, App};
use meridian_backend::config::Config;
use meridian_backend::routes::configure_routes;
use meridian_backend::services::auth::hash_password;
use sqlx::postgres::PgPoolOptions;
use sqlx::PgPool;
use uuid::Uuid;

fn test_config() -> Config {
    let db_url = std::env::var("TEST_DATABASE_URL")
        .unwrap_or_else(|_| "postgres://meridian:meridian@127.0.0.1:55432/meridian_seeded?sslmode=disable".into());
    let tmp = std::env::temp_dir().to_string_lossy().to_string();
    Config {
        database_url: db_url,
        host: "127.0.0.1".into(),
        port: 8080,
        session_secret: "x".repeat(64),
        session_max_age_seconds: 3600,
        log_level: "info".into(),
        backup_encryption_key: "test_backup_key_must_be_32chars_!".into(),
        exports_dir: tmp.clone(),
        backups_dir: tmp,
    }
}

async fn test_pool() -> PgPool {
    let url = std::env::var("TEST_DATABASE_URL")
        .expect("TEST_DATABASE_URL must be set to run e2e_workflow_tests");
    let pool = PgPoolOptions::new()
        .max_connections(5)
        .connect(&url)
        .await
        .expect("Failed to connect to test database");
    sqlx::migrate!("../migrations")
        .run(&pool)
        .await
        .expect("Failed to run migrations");
    pool
}

async fn seed_user(pool: &PgPool, username: &str, role: &str) -> Uuid {
    let hash = hash_password("TestPass2024!!").expect("hash failed");
    sqlx::query(
        "INSERT INTO users (id, username, email, password_hash, account_state, created_at, updated_at)
         VALUES (gen_random_uuid(), $1, $2, $3, 'active', NOW(), NOW())
         ON CONFLICT (username) DO UPDATE
           SET password_hash = EXCLUDED.password_hash, account_state = 'active', updated_at = NOW()",
    )
    .bind(username)
    .bind(format!("{}@test.local", username))
    .bind(&hash)
    .execute(pool)
    .await
    .expect("seed user failed");

    let role_id: i32 = sqlx::query_scalar("SELECT id FROM roles WHERE name = $1")
        .bind(role)
        .fetch_one(pool)
        .await
        .expect("role not found");
    let uid: Uuid = sqlx::query_scalar("SELECT id FROM users WHERE username = $1")
        .bind(username)
        .fetch_one(pool)
        .await
        .expect("seeded user missing");
    sqlx::query("INSERT INTO user_roles (user_id, role_id) VALUES ($1, $2) ON CONFLICT DO NOTHING")
        .bind(uid)
        .bind(role_id)
        .execute(pool)
        .await
        .expect("assign role failed");

    uid
}

async fn make_super_admin(pool: &PgPool, user_id: Uuid) {
    sqlx::query("UPDATE users SET is_super_admin = TRUE WHERE id = $1")
        .bind(user_id)
        .execute(pool)
        .await
        .expect("failed to set super admin");
}

async fn login_token(
    app: &impl actix_web::dev::Service<
        actix_http::Request,
        Response = actix_web::dev::ServiceResponse,
        Error = actix_web::Error,
    >,
    username: &str,
) -> String {
    let req = TestRequest::post()
        .uri("/api/v1/auth/login")
        .set_json(serde_json::json!({"username": username, "password": "TestPass2024!!"}))
        .to_request();
    let resp = call_service(app, req).await;
    let body: serde_json::Value = read_body_json(resp).await;
    body["token"].as_str().expect("token missing").to_string()
}

#[actix_web::test]
#[ignore = "requires TEST_DATABASE_URL"]
async fn test_student_deletion_request_roundtrip_disables_future_login() {
    let pool = test_pool().await;
    let suffix = &Uuid::new_v4().to_string()[..8];
    let student_name = format!("e2e_student_{}", suffix);
    let admin_name = format!("e2e_admin_{}", suffix);

    seed_user(&pool, &student_name, "Student").await;
    let admin_id = seed_user(&pool, &admin_name, "Administrator").await;
    make_super_admin(&pool, admin_id).await;

    let app = init_service(
        App::new()
            .app_data(web::Data::new(pool.clone()))
            .configure(configure_routes),
    )
    .await;

    let student_token = login_token(&app, &student_name).await;
    let req = TestRequest::post()
        .uri("/api/v1/users/me/request-deletion")
        .insert_header(("Authorization", format!("Bearer {}", student_token)))
        .set_json(serde_json::json!({"reason": "Please remove my account"}))
        .to_request();
    let resp = call_service(&app, req).await;
    assert_eq!(resp.status(), 201, "student should be able to request deletion");

    let admin_token = login_token(&app, &admin_name).await;
    let req = TestRequest::get()
        .uri("/api/v1/admin/deletion-requests")
        .insert_header(("Authorization", format!("Bearer {}", admin_token)))
        .to_request();
    let resp = call_service(&app, req).await;
    assert_eq!(resp.status(), 200);
    let body: serde_json::Value = read_body_json(resp).await;
    let request_id = body
        .as_array()
        .and_then(|rows| {
            rows.iter().find_map(|row| {
                (row["username"].as_str() == Some(student_name.as_str()))
                    .then(|| row["id"].as_str().map(str::to_string))
                    .flatten()
            })
        })
        .expect("deletion request missing");

    let req = TestRequest::post()
        .uri(&format!("/api/v1/admin/deletion-requests/{}/approve", request_id))
        .insert_header(("Authorization", format!("Bearer {}", admin_token)))
        .to_request();
    let resp = call_service(&app, req).await;
    assert_eq!(resp.status(), 200, "admin should approve deletion");

    let req = TestRequest::post()
        .uri("/api/v1/auth/login")
        .set_json(serde_json::json!({"username": student_name, "password": "TestPass2024!!"}))
        .to_request();
    let resp = call_service(&app, req).await;
    assert_eq!(resp.status(), 403, "disabled account should not be able to log in");
}

#[actix_web::test]
#[ignore = "requires TEST_DATABASE_URL"]
async fn test_student_preferences_and_notification_read_flow() {
    let pool = test_pool().await;
    let suffix = &Uuid::new_v4().to_string()[..8];
    let student_name = format!("e2e_notify_{}", suffix);
    let student_id = seed_user(&pool, &student_name, "Student").await;

    let app = init_service(
        App::new()
            .app_data(web::Data::new(pool.clone()))
            .configure(configure_routes),
    )
    .await;

    let token = login_token(&app, &student_name).await;

    let req = TestRequest::get()
        .uri("/api/v1/preferences")
        .insert_header(("Authorization", format!("Bearer {}", token)))
        .to_request();
    let resp = call_service(&app, req).await;
    assert_eq!(resp.status(), 200);
    let body: serde_json::Value = read_body_json(resp).await;
    assert_eq!(body["inbox_frequency"], "immediate");

    let req = TestRequest::patch()
        .uri("/api/v1/preferences")
        .insert_header(("Authorization", format!("Bearer {}", token)))
        .set_json(serde_json::json!({
            "inbox_frequency": "daily",
            "dnd_enabled": true,
            "dnd_start": "22:00",
            "dnd_end": "07:00"
        }))
        .to_request();
    let resp = call_service(&app, req).await;
    assert_eq!(resp.status(), 200);

    let notification_id = Uuid::new_v4();
    sqlx::query(
        "INSERT INTO notifications (id, recipient_id, subject, body, notification_type, created_at)
         VALUES ($1, $2, $3, $4, 'general', NOW())",
    )
    .bind(notification_id)
    .bind(student_id)
    .bind("Policy Update")
    .bind("There is a new handbook update.")
    .execute(&pool)
    .await
    .expect("insert notification failed");

    let req = TestRequest::get()
        .uri("/api/v1/notifications")
        .insert_header(("Authorization", format!("Bearer {}", token)))
        .to_request();
    let resp = call_service(&app, req).await;
    assert_eq!(resp.status(), 200);
    let body: serde_json::Value = read_body_json(resp).await;
    assert!(
        body.as_array()
            .expect("notifications array")
            .iter()
            .any(|row| row["id"].as_str() == Some(notification_id.to_string().as_str())),
        "new notification should be visible to the student"
    );

    let req = TestRequest::post()
        .uri(&format!("/api/v1/notifications/{}/read", notification_id))
        .insert_header(("Authorization", format!("Bearer {}", token)))
        .to_request();
    let resp = call_service(&app, req).await;
    assert_eq!(resp.status(), 200);

    let req = TestRequest::get()
        .uri("/api/v1/notifications/unread-count")
        .insert_header(("Authorization", format!("Bearer {}", token)))
        .to_request();
    let resp = call_service(&app, req).await;
    assert_eq!(resp.status(), 200);
    let body: serde_json::Value = read_body_json(resp).await;
    assert_eq!(body["unread"], 0);
}

// ---------------------------------------------------------------------------
// E2E: check-in submit → admin decide → student notification
// ---------------------------------------------------------------------------

#[actix_web::test]
#[ignore = "requires TEST_DATABASE_URL"]
async fn test_checkin_submit_and_decide_workflow() {
    let pool = test_pool().await;
    let suffix = &Uuid::new_v4().to_string()[..8];

    // Build school infrastructure.
    let district_id = Uuid::new_v4();
    let campus_id = Uuid::new_v4();
    let school_id = Uuid::new_v4();

    sqlx::query(
        "INSERT INTO districts (id, name, state, created_at) VALUES ($1, $2, 'TX', NOW())",
    )
    .bind(district_id)
    .bind(format!("E2E District {}", suffix))
    .execute(&pool)
    .await
    .expect("seed district");

    sqlx::query(
        "INSERT INTO campuses (id, district_id, name, created_at) VALUES ($1, $2, $3, NOW())",
    )
    .bind(campus_id)
    .bind(district_id)
    .bind(format!("E2E Campus {}", suffix))
    .execute(&pool)
    .await
    .expect("seed campus");

    sqlx::query(
        "INSERT INTO schools (id, campus_id, name, school_type, created_at)
         VALUES ($1, $2, $3, 'general', NOW())",
    )
    .bind(school_id)
    .bind(campus_id)
    .bind(format!("E2E School {}", suffix))
    .execute(&pool)
    .await
    .expect("seed school");

    // Seed users before the window so we can set created_by.
    let student_name = format!("e2e_ci_student_{}", suffix);
    let admin_name = format!("e2e_ci_admin_{}", suffix);
    let student_id = seed_user(&pool, &student_name, "Student").await;
    let admin_id = seed_user(&pool, &admin_name, "Administrator").await;
    make_super_admin(&pool, admin_id).await;

    // Open check-in window.
    let window_id = Uuid::new_v4();
    sqlx::query(
        "INSERT INTO checkin_windows
             (id, school_id, created_by, title, opens_at, closes_at, allow_late, active, created_at)
         VALUES ($1, $2, $3, $4,
                 NOW() - INTERVAL '1 hour',
                 NOW() + INTERVAL '1 hour',
                 FALSE, TRUE, NOW())",
    )
    .bind(window_id)
    .bind(school_id)
    .bind(admin_id)
    .bind(format!("E2E Window {}", suffix))
    .execute(&pool)
    .await
    .expect("seed window");

    // Assign student to school.
    sqlx::query(
        "INSERT INTO user_school_assignments (user_id, school_id, assignment_type, assigned_at)
         VALUES ($1, $2, 'student', NOW()) ON CONFLICT DO NOTHING",
    )
    .bind(student_id)
    .bind(school_id)
    .execute(&pool)
    .await
    .expect("assign student to school");

    let app = init_service(
        App::new()
            .app_data(web::Data::new(pool.clone()))
            .configure(configure_routes),
    )
    .await;

    // Student submits check-in.
    let student_token = login_token(&app, &student_name).await;
    let req = TestRequest::post()
        .uri(&format!("/api/v1/check-ins/windows/{}/submit", window_id))
        .insert_header(("Authorization", format!("Bearer {}", student_token)))
        .set_json(serde_json::json!({}))
        .to_request();
    let resp = call_service(&app, req).await;
    assert_eq!(resp.status(), 201, "student submit must return 201");
    let sub_body: serde_json::Value = read_body_json(resp).await;
    let submission_id = sub_body["submission_id"].as_str().expect("submission_id missing");
    assert_eq!(sub_body["status"], "pending", "initial status must be pending");

    // Admin approves the submission.
    let admin_token = login_token(&app, &admin_name).await;
    let req = TestRequest::post()
        .uri(&format!(
            "/api/v1/check-ins/windows/{}/submissions/{}/decide",
            window_id, submission_id
        ))
        .insert_header(("Authorization", format!("Bearer {}", admin_token)))
        .set_json(serde_json::json!({ "decision": "approved" }))
        .to_request();
    let resp = call_service(&app, req).await;
    assert_eq!(resp.status(), 200, "admin approve must return 200");
    let dec_body: serde_json::Value = read_body_json(resp).await;
    assert_eq!(dec_body["decision"], "approved");

    // Verify decision recorded in DB.
    let decision: String = sqlx::query_scalar(
        "SELECT decision FROM checkin_approval_decisions WHERE submission_id = $1",
    )
    .bind(Uuid::parse_str(submission_id).unwrap())
    .fetch_one(&pool)
    .await
    .expect("decision row missing");
    assert_eq!(decision, "approved", "DB must record decision as 'approved'");

    // Verify student received a notification.
    let notif_count: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM notifications WHERE recipient_id = $1 AND notification_type = 'checkin'",
    )
    .bind(student_id)
    .fetch_one(&pool)
    .await
    .expect("notification count query failed");
    assert!(notif_count >= 1, "student must receive at least one checkin notification");
}

// ---------------------------------------------------------------------------
// E2E: order placement → admin fulfilment → student notification
// ---------------------------------------------------------------------------

#[actix_web::test]
#[ignore = "requires TEST_DATABASE_URL"]
async fn test_order_placement_and_fulfillment_chain() {
    let pool = test_pool().await;
    let suffix = &Uuid::new_v4().to_string()[..8];

    let student_name = format!("e2e_ord_student_{}", suffix);
    let admin_name = format!("e2e_ord_admin_{}", suffix);

    let student_id = seed_user(&pool, &student_name, "Student").await;
    let admin_id = seed_user(&pool, &admin_name, "Administrator").await;
    make_super_admin(&pool, admin_id).await;

    let app = init_service(
        App::new()
            .app_data(web::Data::new(pool.clone()))
            .configure(configure_routes),
    )
    .await;

    let admin_token = login_token(&app, &admin_name).await;
    let student_token = login_token(&app, &student_name).await;

    // Admin creates a product.
    let req = TestRequest::post()
        .uri("/api/v1/admin/products")
        .insert_header(("Authorization", format!("Bearer {}", admin_token)))
        .set_json(serde_json::json!({
            "name": format!("Test Product {}", suffix),
            "description": "E2E test product",
            "price_cents": 1000,
            "initial_quantity": 50
        }))
        .to_request();
    let resp = call_service(&app, req).await;
    assert_eq!(resp.status(), 201, "admin create product must return 201");
    let prod_body: serde_json::Value = read_body_json(resp).await;
    let product_id = prod_body["id"].as_str().expect("product id missing");

    // Student places an order.
    let req = TestRequest::post()
        .uri("/api/v1/orders")
        .insert_header(("Authorization", format!("Bearer {}", student_token)))
        .set_json(serde_json::json!({
            "items": [{ "product_id": product_id, "quantity": 1 }]
        }))
        .to_request();
    let resp = call_service(&app, req).await;
    assert_eq!(resp.status(), 201, "student place order must return 201");
    let ord_body: serde_json::Value = read_body_json(resp).await;
    // create_order returns OrderDetailResponse which flattens OrderSummaryRow — field is "id"
    let order_id = ord_body["id"].as_str().expect("order id missing");
    assert_eq!(ord_body["status"], "pending", "new order must be pending");

    // Admin updates order to 'fulfilled'.
    let req = TestRequest::post()
        .uri(&format!("/api/v1/admin/orders/{}/status", order_id))
        .insert_header(("Authorization", format!("Bearer {}", admin_token)))
        .set_json(serde_json::json!({ "status": "fulfilled" }))
        .to_request();
    let resp = call_service(&app, req).await;
    assert_eq!(resp.status(), 200, "admin fulfill order must return 200");
    let update_body: serde_json::Value = read_body_json(resp).await;
    assert_eq!(update_body["new_status"], "fulfilled");
    assert_eq!(update_body["old_status"], "pending");

    // Verify student received an order notification.
    let notif_count: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM notifications WHERE recipient_id = $1 AND notification_type = 'order'",
    )
    .bind(student_id)
    .fetch_one(&pool)
    .await
    .expect("notification count query failed");
    assert!(notif_count >= 1, "student must receive at least one order notification after fulfillment");
}

// ---------------------------------------------------------------------------
// E2E: config value update is reflected in audit history
// ---------------------------------------------------------------------------

#[actix_web::test]
#[ignore = "requires TEST_DATABASE_URL"]
async fn test_config_value_update_appears_in_audit_log() {
    let pool = test_pool().await;
    let suffix = &Uuid::new_v4().to_string()[..8];

    let admin_name = format!("e2e_cfg_admin_{}", suffix);
    let admin_id = seed_user(&pool, &admin_name, "Administrator").await;
    make_super_admin(&pool, admin_id).await;

    let app = init_service(
        App::new()
            .app_data(web::Data::new(pool.clone()))
            .app_data(web::Data::new(test_config()))
            .configure(configure_routes),
    )
    .await;

    let admin_token = login_token(&app, &admin_name).await;

    // Read the current config to find an existing key.
    let req = TestRequest::get()
        .uri("/api/v1/admin/config")
        .insert_header(("Authorization", format!("Bearer {}", admin_token)))
        .to_request();
    let resp = call_service(&app, req).await;
    assert_eq!(resp.status(), 200, "admin config list must return 200");

    // Update the `log_retention_days` config value.
    let req = TestRequest::post()
        .uri("/api/v1/admin/config/values/log_retention_days")
        .insert_header(("Authorization", format!("Bearer {}", admin_token)))
        .set_json(serde_json::json!({ "value": "200" }))
        .to_request();
    let resp = call_service(&app, req).await;
    assert_eq!(resp.status(), 200, "config value update must return 200");

    // Read config history — the update should appear there.
    let req = TestRequest::get()
        .uri("/api/v1/admin/config/history")
        .insert_header(("Authorization", format!("Bearer {}", admin_token)))
        .to_request();
    let resp = call_service(&app, req).await;
    assert_eq!(resp.status(), 200, "config history must return 200");
    let body: serde_json::Value = read_body_json(resp).await;
    let history = body.as_array().expect("history must be an array");
    assert!(
        history.iter().any(|row| row["config_key"].as_str() == Some("log_retention_days")),
        "config history must include the recently updated key"
    );

    // Verify the audit log also captured the change.
    let req = TestRequest::get()
        .uri("/api/v1/logs/audit")
        .insert_header(("Authorization", format!("Bearer {}", admin_token)))
        .to_request();
    let resp = call_service(&app, req).await;
    assert_eq!(resp.status(), 200, "audit log endpoint must return 200");
    let body: serde_json::Value = read_body_json(resp).await;
    let logs = body["rows"].as_array().expect("audit logs must be array");
    assert!(
        logs.iter().any(|entry| entry["action"].as_str() == Some("update_config")),
        "audit log must contain an update_config entry after config change"
    );

    // Restore the original value so other tests are not affected.
    let req = TestRequest::post()
        .uri("/api/v1/admin/config/values/log_retention_days")
        .insert_header(("Authorization", format!("Bearer {}", admin_token)))
        .set_json(serde_json::json!({ "value": "180" }))
        .to_request();
    let _ = call_service(&app, req).await;
}

// ---------------------------------------------------------------------------
// E2E: admin changes user state from blocked to active; login behaviour follows
// ---------------------------------------------------------------------------

#[actix_web::test]
#[ignore = "requires TEST_DATABASE_URL"]
async fn test_admin_user_state_management_affects_login() {
    let pool = test_pool().await;
    let suffix = &Uuid::new_v4().to_string()[..8];

    let student_name = format!("e2e_state_student_{}", suffix);
    let admin_name = format!("e2e_state_admin_{}", suffix);

    let student_id = seed_user(&pool, &student_name, "Student").await;
    let admin_id = seed_user(&pool, &admin_name, "Administrator").await;
    make_super_admin(&pool, admin_id).await;

    let app = init_service(
        App::new()
            .app_data(web::Data::new(pool.clone()))
            .configure(configure_routes),
    )
    .await;

    let admin_token = login_token(&app, &admin_name).await;

    // Step 1: Admin suspends the student account.
    let req = TestRequest::post()
        .uri(&format!("/api/v1/admin/users/{}/set-state", student_id))
        .insert_header(("Authorization", format!("Bearer {}", admin_token)))
        .set_json(serde_json::json!({ "state": "disabled" }))
        .to_request();
    let resp = call_service(&app, req).await;
    assert_eq!(resp.status(), 200, "admin set-state must return 200");

    // Step 2: Suspended student cannot log in.
    let req = TestRequest::post()
        .uri("/api/v1/auth/login")
        .set_json(serde_json::json!({
            "username": student_name,
            "password": "TestPass2024!!"
        }))
        .to_request();
    let resp = call_service(&app, req).await;
    assert_eq!(resp.status(), 403, "suspended account must get 403 on login");

    // Step 3: Admin re-activates the student account.
    let req = TestRequest::post()
        .uri(&format!("/api/v1/admin/users/{}/set-state", student_id))
        .insert_header(("Authorization", format!("Bearer {}", admin_token)))
        .set_json(serde_json::json!({ "state": "active" }))
        .to_request();
    let resp = call_service(&app, req).await;
    assert_eq!(resp.status(), 200, "re-activating account must return 200");

    // Step 4: Student can log in again.
    let req = TestRequest::post()
        .uri("/api/v1/auth/login")
        .set_json(serde_json::json!({
            "username": student_name,
            "password": "TestPass2024!!"
        }))
        .to_request();
    let resp = call_service(&app, req).await;
    assert_eq!(resp.status(), 200, "re-activated account must be able to log in");
    let body: serde_json::Value = read_body_json(resp).await;
    assert!(body["token"].as_str().is_some(), "login after re-activation must return a token");
}

// ---------------------------------------------------------------------------
// E2E: campaign toggle affects order shipping fee
// ---------------------------------------------------------------------------

#[actix_web::test]
#[ignore = "requires TEST_DATABASE_URL"]
async fn test_campaign_toggle_affects_order_shipping() {
    let pool = test_pool().await;
    let suffix = &Uuid::new_v4().to_string()[..8];

    let student_name = format!("e2e_camp_student_{}", suffix);
    let admin_name = format!("e2e_camp_admin_{}", suffix);

    seed_user(&pool, &student_name, "Student").await;
    let admin_id = seed_user(&pool, &admin_name, "Administrator").await;
    make_super_admin(&pool, admin_id).await;

    let app = init_service(
        App::new()
            .app_data(web::Data::new(pool.clone()))
            .app_data(web::Data::new(test_config()))
            .configure(configure_routes),
    )
    .await;

    let admin_token = login_token(&app, &admin_name).await;
    let student_token = login_token(&app, &student_name).await;

    // Admin creates a product for ordering.
    let req = TestRequest::post()
        .uri("/api/v1/admin/products")
        .insert_header(("Authorization", format!("Bearer {}", admin_token)))
        .set_json(serde_json::json!({
            "name": format!("Camp Product {}", suffix),
            "description": "campaign test product",
            "price_cents": 500,
            "initial_quantity": 100
        }))
        .to_request();
    let resp = call_service(&app, req).await;
    assert_eq!(resp.status(), 201, "product creation must succeed");
    let prod_body: serde_json::Value = read_body_json(resp).await;
    let product_id = prod_body["id"].as_str().expect("product id missing");

    // --- Phase 1: enable free_shipping ---
    let req = TestRequest::post()
        .uri("/api/v1/admin/config/campaigns/free_shipping")
        .insert_header(("Authorization", format!("Bearer {}", admin_token)))
        .set_json(serde_json::json!({ "enabled": true }))
        .to_request();
    let resp = call_service(&app, req).await;
    assert_eq!(resp.status(), 200, "enable free_shipping must return 200");

    // Student places order while free_shipping is active.
    let req = TestRequest::post()
        .uri("/api/v1/orders")
        .insert_header(("Authorization", format!("Bearer {}", student_token)))
        .set_json(serde_json::json!({
            "items": [{ "product_id": product_id, "quantity": 1 }]
        }))
        .to_request();
    let resp = call_service(&app, req).await;
    assert_eq!(resp.status(), 201, "order with free_shipping must return 201");
    let free_order: serde_json::Value = read_body_json(resp).await;
    assert_eq!(
        free_order["shipping_fee_cents"], 0,
        "shipping_fee_cents must be 0 when free_shipping campaign is enabled"
    );

    // --- Phase 2: disable free_shipping ---
    let req = TestRequest::post()
        .uri("/api/v1/admin/config/campaigns/free_shipping")
        .insert_header(("Authorization", format!("Bearer {}", admin_token)))
        .set_json(serde_json::json!({ "enabled": false }))
        .to_request();
    let resp = call_service(&app, req).await;
    assert_eq!(resp.status(), 200, "disable free_shipping must return 200");

    // Student places another order — shipping fee must be non-zero.
    let req = TestRequest::post()
        .uri("/api/v1/orders")
        .insert_header(("Authorization", format!("Bearer {}", student_token)))
        .set_json(serde_json::json!({
            "items": [{ "product_id": product_id, "quantity": 1 }]
        }))
        .to_request();
    let resp = call_service(&app, req).await;
    assert_eq!(resp.status(), 201, "order without free_shipping must return 201");
    let paid_order: serde_json::Value = read_body_json(resp).await;
    assert!(
        paid_order["shipping_fee_cents"].as_i64().unwrap_or(0) > 0,
        "shipping_fee_cents must be > 0 when free_shipping campaign is disabled"
    );
}
