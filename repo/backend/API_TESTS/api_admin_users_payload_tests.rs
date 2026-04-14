/// Admin user-management endpoint payload tests.
///
/// Verifies that admin user listing, state changes, and deletion-request
/// management return correct JSON shapes and validate inputs.
///
/// Requires a live PostgreSQL database:
///   TEST_DATABASE_URL=postgres://meridian:meridian@127.0.0.1:5433/meridian_seeded?sslmode=disable \
///     cargo test --test api_admin_users_payload_tests -- --include-ignored
use actix_web::test::{call_service, init_service, read_body_json, TestRequest};
use actix_web::{web, App};
use meridian_backend::routes::configure_routes;
use meridian_backend::services::auth::hash_password;
use serde_json::Value;
use sqlx::postgres::PgPoolOptions;
use sqlx::PgPool;
use uuid::Uuid;

async fn test_pool() -> PgPool {
    let url = std::env::var("TEST_DATABASE_URL")
        .expect("TEST_DATABASE_URL must be set to run api_admin_users_payload_tests");
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
        .expect("user missing");
    sqlx::query(
        "INSERT INTO user_roles (user_id, role_id) VALUES ($1, $2) ON CONFLICT DO NOTHING",
    )
    .bind(uid)
    .bind(role_id)
    .execute(pool)
    .await
    .expect("assign role failed");
    uid
}

async fn seed_super_admin(pool: &PgPool, username: &str) -> Uuid {
    let hash = hash_password("TestPass2024!!").expect("hash failed");
    sqlx::query(
        "INSERT INTO users (id, username, email, password_hash, account_state, is_super_admin, created_at, updated_at)
         VALUES (gen_random_uuid(), $1, $2, $3, 'active', TRUE, NOW(), NOW())
         ON CONFLICT (username) DO UPDATE
           SET password_hash = EXCLUDED.password_hash, account_state = 'active',
               is_super_admin = TRUE, updated_at = NOW()",
    )
    .bind(username)
    .bind(format!("{}@test.local", username))
    .bind(&hash)
    .execute(pool)
    .await
    .expect("seed super admin failed");

    let role_id: i32 = sqlx::query_scalar("SELECT id FROM roles WHERE name = 'Administrator'")
        .fetch_one(pool)
        .await
        .expect("admin role not found");
    let uid: Uuid = sqlx::query_scalar("SELECT id FROM users WHERE username = $1")
        .bind(username)
        .fetch_one(pool)
        .await
        .unwrap();
    sqlx::query(
        "INSERT INTO user_roles (user_id, role_id) VALUES ($1, $2) ON CONFLICT DO NOTHING",
    )
    .bind(uid)
    .bind(role_id)
    .execute(pool)
    .await
    .unwrap();
    uid
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
        .set_json(serde_json::json!({ "username": username, "password": "TestPass2024!!" }))
        .to_request();
    let body: Value = read_body_json(call_service(app, req).await).await;
    body["token"].as_str().expect("token missing").to_string()
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

/// GET /api/v1/admin/users must return a JSON array where each item has the
/// required fields: id, username, email, account_state, and roles.
#[actix_web::test]
#[ignore = "requires TEST_DATABASE_URL"]
async fn test_list_admin_users_returns_required_fields() {
    let pool = test_pool().await;
    let suffix = &Uuid::new_v4().to_string()[..8];
    let admin_name = format!("admusr_admin_{}", suffix);
    seed_super_admin(&pool, &admin_name).await;
    // Seed a target user so the list is non-empty
    seed_user(&pool, &format!("admusr_target_{}", suffix), "Student").await;

    let app = init_service(
        App::new()
            .app_data(web::Data::new(pool.clone()))
            .configure(configure_routes),
    )
    .await;
    let token = login_token(&app, &admin_name).await;

    let req = TestRequest::get()
        .uri("/api/v1/admin/users")
        .insert_header(("Authorization", format!("Bearer {}", token)))
        .to_request();
    let resp = call_service(&app, req).await;
    assert_eq!(resp.status(), 200);
    let body: Value = read_body_json(resp).await;
    let items = body.as_array().expect("admin users response must be a JSON array");
    assert!(!items.is_empty(), "user list must be non-empty after seeding");

    for item in items {
        assert!(item["id"].is_string(), "user.id must be a string");
        assert!(item["username"].is_string(), "user.username must be a string");
        assert!(item["email"].is_string(), "user.email must be a string");
        assert!(
            item["account_state"].is_string(),
            "user.account_state must be a string"
        );
        assert!(
            item["roles"].is_array(),
            "user.roles must be an array"
        );
    }
}

/// POST /api/v1/admin/users/{id}/set-state must return a response object
/// with user_id, username, old_state, new_state, and message fields.
#[actix_web::test]
#[ignore = "requires TEST_DATABASE_URL"]
async fn test_set_user_state_response_contains_state_change_fields() {
    let pool = test_pool().await;
    let suffix = &Uuid::new_v4().to_string()[..8];
    let admin_name = format!("setst_admin_{}", suffix);
    let target_name = format!("setst_target_{}", suffix);
    seed_super_admin(&pool, &admin_name).await;
    let target_id = seed_user(&pool, &target_name, "Student").await;

    let app = init_service(
        App::new()
            .app_data(web::Data::new(pool.clone()))
            .configure(configure_routes),
    )
    .await;
    let token = login_token(&app, &admin_name).await;

    let req = TestRequest::post()
        .uri(&format!("/api/v1/admin/users/{}/set-state", target_id))
        .insert_header(("Authorization", format!("Bearer {}", token)))
        .set_json(serde_json::json!({ "state": "disabled", "reason": "test deactivation" }))
        .to_request();
    let resp = call_service(&app, req).await;
    assert_eq!(resp.status(), 200);
    let body: Value = read_body_json(resp).await;

    assert_eq!(body["user_id"].as_str().unwrap_or(""), target_id.to_string(),
        "response must include the target user_id");
    assert!(body["username"].is_string(), "response must include username");
    assert_eq!(body["old_state"], "active", "old_state must be 'active'");
    assert_eq!(body["new_state"], "disabled", "new_state must be 'disabled'");
    assert!(body["message"].is_string(), "response must include a message field");
}

/// POST /api/v1/admin/users/{id}/set-state with an invalid state value must
/// return 422 with an error field.
#[actix_web::test]
#[ignore = "requires TEST_DATABASE_URL"]
async fn test_set_user_state_invalid_value_returns_422() {
    let pool = test_pool().await;
    let suffix = &Uuid::new_v4().to_string()[..8];
    let admin_name = format!("setst_val_admin_{}", suffix);
    let target_name = format!("setst_val_target_{}", suffix);
    seed_super_admin(&pool, &admin_name).await;
    let target_id = seed_user(&pool, &target_name, "Student").await;

    let app = init_service(
        App::new()
            .app_data(web::Data::new(pool.clone()))
            .configure(configure_routes),
    )
    .await;
    let token = login_token(&app, &admin_name).await;

    let req = TestRequest::post()
        .uri(&format!("/api/v1/admin/users/{}/set-state", target_id))
        .insert_header(("Authorization", format!("Bearer {}", token)))
        .set_json(serde_json::json!({ "state": "suspended" })) // not a valid state
        .to_request();
    let resp = call_service(&app, req).await;
    assert_eq!(resp.status(), 422, "invalid state must return 422");
    let body: Value = read_body_json(resp).await;
    assert!(body["error"].is_string(), "422 must include an error field");
}

/// POST /api/v1/admin/users/{id}/set-state for a non-existent user must
/// return 404 with an error field.
#[actix_web::test]
#[ignore = "requires TEST_DATABASE_URL"]
async fn test_set_nonexistent_user_state_returns_404() {
    let pool = test_pool().await;
    let suffix = &Uuid::new_v4().to_string()[..8];
    let admin_name = format!("setst_404_admin_{}", suffix);
    seed_super_admin(&pool, &admin_name).await;

    let app = init_service(
        App::new()
            .app_data(web::Data::new(pool.clone()))
            .configure(configure_routes),
    )
    .await;
    let token = login_token(&app, &admin_name).await;

    let ghost_id = Uuid::new_v4();
    let req = TestRequest::post()
        .uri(&format!("/api/v1/admin/users/{}/set-state", ghost_id))
        .insert_header(("Authorization", format!("Bearer {}", token)))
        .set_json(serde_json::json!({ "state": "disabled" }))
        .to_request();
    let resp = call_service(&app, req).await;
    assert_eq!(resp.status(), 404);
    let body: Value = read_body_json(resp).await;
    assert!(body["error"].is_string(), "404 must include an error field");
}

/// GET /api/v1/admin/deletion-requests must return a JSON array where each
/// item has the required fields.
#[actix_web::test]
#[ignore = "requires TEST_DATABASE_URL"]
async fn test_list_account_deletion_requests_returns_array_with_required_fields() {
    let pool = test_pool().await;
    let suffix = &Uuid::new_v4().to_string()[..8];
    let admin_name = format!("delreq_admin_{}", suffix);
    let requester_name = format!("delreq_user_{}", suffix);
    seed_super_admin(&pool, &admin_name).await;
    let requester_id = seed_user(&pool, &requester_name, "Student").await;

    // Seed a pending deletion request
    sqlx::query(
        "INSERT INTO account_deletion_requests (id, user_id, reason, status, requested_at)
         VALUES (gen_random_uuid(), $1, 'test reason', 'pending', NOW())",
    )
    .bind(requester_id)
    .execute(&pool)
    .await
    .expect("seed deletion request failed");

    let app = init_service(
        App::new()
            .app_data(web::Data::new(pool.clone()))
            .configure(configure_routes),
    )
    .await;
    let token = login_token(&app, &admin_name).await;

    let req = TestRequest::get()
        .uri("/api/v1/admin/deletion-requests")
        .insert_header(("Authorization", format!("Bearer {}", token)))
        .to_request();
    let resp = call_service(&app, req).await;
    assert_eq!(resp.status(), 200);
    let body: Value = read_body_json(resp).await;
    let items = body.as_array().expect("deletion requests must be a JSON array");
    assert!(!items.is_empty(), "deletion request list must be non-empty after seeding");

    // Find the seeded request
    let seeded = items
        .iter()
        .find(|r| r["user_id"].as_str() == Some(&requester_id.to_string()))
        .expect("seeded deletion request must appear in list");

    assert!(seeded["id"].is_string(), "request.id must be a string");
    assert!(seeded["user_id"].is_string(), "request.user_id must be a string");
    assert!(seeded["username"].is_string(), "request.username must be a string");
    assert!(seeded["status"].is_string(), "request.status must be a string");
    assert_eq!(seeded["status"], "pending", "seeded request must have status 'pending'");
    assert!(seeded["requested_at"].is_string(), "request.requested_at must be a string");
}

/// Approving a deletion request must return 200 with a confirmation message
/// and set the user's account state to 'disabled'.
#[actix_web::test]
#[ignore = "requires TEST_DATABASE_URL"]
async fn test_approve_deletion_request_disables_account_and_returns_message() {
    let pool = test_pool().await;
    let suffix = &Uuid::new_v4().to_string()[..8];
    let admin_name = format!("delreq_appr_admin_{}", suffix);
    let requester_name = format!("delreq_appr_user_{}", suffix);
    seed_super_admin(&pool, &admin_name).await;
    let requester_id = seed_user(&pool, &requester_name, "Student").await;

    let request_id = Uuid::new_v4();
    sqlx::query(
        "INSERT INTO account_deletion_requests (id, user_id, reason, status, requested_at)
         VALUES ($1, $2, 'want to leave', 'pending', NOW())",
    )
    .bind(request_id)
    .bind(requester_id)
    .execute(&pool)
    .await
    .expect("seed deletion request failed");

    let app = init_service(
        App::new()
            .app_data(web::Data::new(pool.clone()))
            .configure(configure_routes),
    )
    .await;
    let token = login_token(&app, &admin_name).await;

    let req = TestRequest::post()
        .uri(&format!("/api/v1/admin/deletion-requests/{}/approve", request_id))
        .insert_header(("Authorization", format!("Bearer {}", token)))
        .to_request();
    let resp = call_service(&app, req).await;
    assert_eq!(resp.status(), 200);
    let body: Value = read_body_json(resp).await;
    assert!(
        body["message"].is_string(),
        "approve must return a message field"
    );

    // Verify the account is now disabled in the DB
    let state: String =
        sqlx::query_scalar("SELECT account_state FROM users WHERE id = $1")
            .bind(requester_id)
            .fetch_one(&pool)
            .await
            .unwrap();
    assert_eq!(state, "disabled", "approved user account must be disabled");
}

/// Rejecting a deletion request must return 200 with a message and leave the
/// user's account state unchanged.
#[actix_web::test]
#[ignore = "requires TEST_DATABASE_URL"]
async fn test_reject_deletion_request_returns_message_and_leaves_account_active() {
    let pool = test_pool().await;
    let suffix = &Uuid::new_v4().to_string()[..8];
    let admin_name = format!("delreq_rej_admin_{}", suffix);
    let requester_name = format!("delreq_rej_user_{}", suffix);
    seed_super_admin(&pool, &admin_name).await;
    let requester_id = seed_user(&pool, &requester_name, "Student").await;

    let request_id = Uuid::new_v4();
    sqlx::query(
        "INSERT INTO account_deletion_requests (id, user_id, reason, status, requested_at)
         VALUES ($1, $2, 'changed my mind', 'pending', NOW())",
    )
    .bind(request_id)
    .bind(requester_id)
    .execute(&pool)
    .await
    .expect("seed deletion request failed");

    let app = init_service(
        App::new()
            .app_data(web::Data::new(pool.clone()))
            .configure(configure_routes),
    )
    .await;
    let token = login_token(&app, &admin_name).await;

    let req = TestRequest::post()
        .uri(&format!(
            "/api/v1/admin/deletion-requests/{}/reject",
            request_id
        ))
        .insert_header(("Authorization", format!("Bearer {}", token)))
        .set_json(serde_json::json!({ "reason": "request denied for testing" }))
        .to_request();
    let resp = call_service(&app, req).await;
    assert_eq!(resp.status(), 200);
    let body: Value = read_body_json(resp).await;
    assert!(
        body["message"].is_string(),
        "reject must return a message field"
    );

    // Account must still be active after rejection
    let state: String =
        sqlx::query_scalar("SELECT account_state FROM users WHERE id = $1")
            .bind(requester_id)
            .fetch_one(&pool)
            .await
            .unwrap();
    assert_eq!(state, "active", "rejected user account must remain active");

    // Request status must be 'rejected'
    let status: String =
        sqlx::query_scalar("SELECT status FROM account_deletion_requests WHERE id = $1")
            .bind(request_id)
            .fetch_one(&pool)
            .await
            .unwrap();
    assert_eq!(status, "rejected", "request status must be 'rejected'");
}
