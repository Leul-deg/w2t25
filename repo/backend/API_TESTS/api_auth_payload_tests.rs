/// Auth endpoint payload tests.
///
/// Verifies that authentication endpoints return the correct JSON shapes —
/// not just status codes, but field presence, types, and values.
///
/// Requires a live PostgreSQL database:
///   TEST_DATABASE_URL=postgres://meridian:meridian@127.0.0.1:5433/meridian_seeded?sslmode=disable \
///     cargo test --test api_auth_payload_tests -- --include-ignored
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
        .expect("TEST_DATABASE_URL must be set to run api_auth_payload_tests");
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

async fn seed_user(pool: &PgPool, username: &str, role: &str) {
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
    sqlx::query(
        "INSERT INTO user_roles (user_id, role_id) VALUES ($1, $2) ON CONFLICT DO NOTHING",
    )
    .bind(uid)
    .bind(role_id)
    .execute(pool)
    .await
    .expect("assign role failed");
}

async fn seed_user_with_state(pool: &PgPool, username: &str, role: &str, state: &str) {
    let hash = hash_password("TestPass2024!!").expect("hash failed");
    sqlx::query(
        "INSERT INTO users (id, username, email, password_hash, account_state, created_at, updated_at)
         VALUES (gen_random_uuid(), $1, $2, $3, $4, NOW(), NOW())
         ON CONFLICT (username) DO UPDATE
           SET password_hash = EXCLUDED.password_hash, account_state = $4, updated_at = NOW()",
    )
    .bind(username)
    .bind(format!("{}@test.local", username))
    .bind(&hash)
    .bind(state)
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
    sqlx::query(
        "INSERT INTO user_roles (user_id, role_id) VALUES ($1, $2) ON CONFLICT DO NOTHING",
    )
    .bind(uid)
    .bind(role_id)
    .execute(pool)
    .await
    .expect("assign role failed");
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

/// GET /api/v1/health must return { "status": "ok", "version": "0.1.0" }
/// and requires no authentication.
#[actix_web::test]
#[ignore = "requires TEST_DATABASE_URL"]
async fn test_health_endpoint_payload() {
    let pool = test_pool().await;
    let app = init_service(
        App::new()
            .app_data(web::Data::new(pool))
            .configure(configure_routes),
    )
    .await;

    let req = TestRequest::get().uri("/api/v1/health").to_request();
    let resp = call_service(&app, req).await;
    assert_eq!(resp.status(), 200);
    let body: Value = read_body_json(resp).await;
    assert_eq!(body["status"], "ok", "health status must be 'ok'");
    assert_eq!(body["version"], "0.1.0", "version must match");
}

/// Successful login must return a non-empty token and a user object with
/// the correct username, account_state, and at least one role.
#[actix_web::test]
#[ignore = "requires TEST_DATABASE_URL"]
async fn test_login_success_payload_shape() {
    let pool = test_pool().await;
    let suffix = &Uuid::new_v4().to_string()[..8];
    let username = format!("auth_login_{}", suffix);
    seed_user(&pool, &username, "Student").await;

    let app = init_service(
        App::new()
            .app_data(web::Data::new(pool))
            .configure(configure_routes),
    )
    .await;

    let req = TestRequest::post()
        .uri("/api/v1/auth/login")
        .set_json(serde_json::json!({ "username": &username, "password": "TestPass2024!!" }))
        .to_request();
    let resp = call_service(&app, req).await;
    assert_eq!(resp.status(), 200);
    let body: Value = read_body_json(resp).await;

    // Token must be a non-empty string
    let token = body["token"].as_str().expect("token must be a string");
    assert!(!token.is_empty(), "token must not be empty");

    // User object must be present with required fields
    let user = &body["user"];
    assert!(user.is_object(), "user must be an object");
    assert!(user["id"].is_string(), "user.id must be a string");
    assert_eq!(user["username"], username, "user.username must match");
    assert!(user["email"].is_string(), "user.email must be a string");
    assert_eq!(user["account_state"], "active", "account_state must be 'active'");

    // Roles must be a non-empty array containing "Student"
    let roles = user["roles"].as_array().expect("roles must be an array");
    assert!(!roles.is_empty(), "roles must not be empty");
    assert!(
        roles.iter().any(|r| r == "Student"),
        "roles must contain 'Student'"
    );
}

/// Wrong password must return 401 with an `error` field (not a token).
#[actix_web::test]
#[ignore = "requires TEST_DATABASE_URL"]
async fn test_login_wrong_password_returns_error_body() {
    let pool = test_pool().await;
    let suffix = &Uuid::new_v4().to_string()[..8];
    let username = format!("auth_badpw_{}", suffix);
    seed_user(&pool, &username, "Student").await;

    let app = init_service(
        App::new()
            .app_data(web::Data::new(pool))
            .configure(configure_routes),
    )
    .await;

    let req = TestRequest::post()
        .uri("/api/v1/auth/login")
        .set_json(serde_json::json!({ "username": &username, "password": "WrongPassword!" }))
        .to_request();
    let resp = call_service(&app, req).await;
    assert_eq!(resp.status(), 401);
    let body: Value = read_body_json(resp).await;
    assert!(
        body["error"].is_string(),
        "401 response must have an `error` string field"
    );
    assert!(body["token"].is_null(), "error response must not contain a token");
}

/// A non-existent username must return 401 with the same generic message as a
/// wrong password — the response must not reveal whether the user exists.
#[actix_web::test]
#[ignore = "requires TEST_DATABASE_URL"]
async fn test_login_nonexistent_user_returns_same_error_as_wrong_password() {
    let pool = test_pool().await;
    let suffix = &Uuid::new_v4().to_string()[..8];
    let real_username = format!("auth_real_{}", suffix);
    let fake_username = format!("auth_ghost_{}", suffix);
    seed_user(&pool, &real_username, "Student").await;

    let app = init_service(
        App::new()
            .app_data(web::Data::new(pool))
            .configure(configure_routes),
    )
    .await;

    let wrong_pw_req = TestRequest::post()
        .uri("/api/v1/auth/login")
        .set_json(serde_json::json!({ "username": &real_username, "password": "Wrong!" }))
        .to_request();
    let no_user_req = TestRequest::post()
        .uri("/api/v1/auth/login")
        .set_json(serde_json::json!({ "username": &fake_username, "password": "anything" }))
        .to_request();

    let resp_wrong = call_service(&app, wrong_pw_req).await;
    let resp_no_user = call_service(&app, no_user_req).await;

    assert_eq!(resp_wrong.status(), 401);
    assert_eq!(resp_no_user.status(), 401);

    let body_wrong: Value = read_body_json(resp_wrong).await;
    let body_no_user: Value = read_body_json(resp_no_user).await;

    // Same error message regardless of whether the user exists
    assert_eq!(
        body_wrong["error"], body_no_user["error"],
        "error message must be identical for wrong-password and non-existent user"
    );
}

/// A disabled account must return 403 with an error message describing the
/// disabled state — not a generic 401.
#[actix_web::test]
#[ignore = "requires TEST_DATABASE_URL"]
async fn test_login_disabled_account_returns_403_with_error() {
    let pool = test_pool().await;
    let suffix = &Uuid::new_v4().to_string()[..8];
    let username = format!("auth_disabled_{}", suffix);
    seed_user_with_state(&pool, &username, "Student", "disabled").await;

    let app = init_service(
        App::new()
            .app_data(web::Data::new(pool))
            .configure(configure_routes),
    )
    .await;

    let req = TestRequest::post()
        .uri("/api/v1/auth/login")
        .set_json(serde_json::json!({ "username": &username, "password": "TestPass2024!!" }))
        .to_request();
    let resp = call_service(&app, req).await;
    assert_eq!(resp.status(), 403, "disabled account must return 403, not 401");
    let body: Value = read_body_json(resp).await;
    let msg = body["error"].as_str().expect("error field must be a string");
    assert!(
        msg.to_lowercase().contains("disabled"),
        "error message should mention 'disabled', got: {}",
        msg
    );
}

/// GET /api/v1/auth/me must return the same user object as the login response.
#[actix_web::test]
#[ignore = "requires TEST_DATABASE_URL"]
async fn test_me_endpoint_returns_logged_in_user() {
    let pool = test_pool().await;
    let suffix = &Uuid::new_v4().to_string()[..8];
    let username = format!("auth_me_{}", suffix);
    seed_user(&pool, &username, "Teacher").await;

    let app = init_service(
        App::new()
            .app_data(web::Data::new(pool))
            .configure(configure_routes),
    )
    .await;

    // Login
    let login_req = TestRequest::post()
        .uri("/api/v1/auth/login")
        .set_json(serde_json::json!({ "username": &username, "password": "TestPass2024!!" }))
        .to_request();
    let login_resp = call_service(&app, login_req).await;
    let login_body: Value = read_body_json(login_resp).await;
    let token = login_body["token"].as_str().unwrap().to_string();

    // GET /me
    let me_req = TestRequest::get()
        .uri("/api/v1/auth/me")
        .insert_header(("Authorization", format!("Bearer {}", token)))
        .to_request();
    let me_resp = call_service(&app, me_req).await;
    assert_eq!(me_resp.status(), 200);
    let me_body: Value = read_body_json(me_resp).await;

    assert_eq!(me_body["username"], username);
    assert_eq!(me_body["account_state"], "active");
    let roles = me_body["roles"].as_array().expect("roles must be an array");
    assert!(roles.iter().any(|r| r == "Teacher"));
}

/// GET /api/v1/auth/me without a token must return 401 with an error body.
#[actix_web::test]
#[ignore = "requires TEST_DATABASE_URL"]
async fn test_me_without_auth_returns_401_with_error() {
    let pool = test_pool().await;
    let app = init_service(
        App::new()
            .app_data(web::Data::new(pool))
            .configure(configure_routes),
    )
    .await;

    let req = TestRequest::get().uri("/api/v1/auth/me").to_request();
    let resp = call_service(&app, req).await;
    assert_eq!(resp.status(), 401);
    let body: Value = read_body_json(resp).await;
    assert!(body["error"].is_string(), "401 must include an error field");
}

/// POST /api/v1/auth/logout must return 200 with a success message, and
/// the token must be invalidated (subsequent /me returns 401).
#[actix_web::test]
#[ignore = "requires TEST_DATABASE_URL"]
async fn test_logout_returns_message_and_invalidates_token() {
    let pool = test_pool().await;
    let suffix = &Uuid::new_v4().to_string()[..8];
    let username = format!("auth_logout_{}", suffix);
    seed_user(&pool, &username, "Student").await;

    let app = init_service(
        App::new()
            .app_data(web::Data::new(pool))
            .configure(configure_routes),
    )
    .await;

    // Login to get token
    let login_req = TestRequest::post()
        .uri("/api/v1/auth/login")
        .set_json(serde_json::json!({ "username": &username, "password": "TestPass2024!!" }))
        .to_request();
    let login_body: Value = read_body_json(call_service(&app, login_req).await).await;
    let token = login_body["token"].as_str().unwrap().to_string();

    // Logout
    let logout_req = TestRequest::post()
        .uri("/api/v1/auth/logout")
        .insert_header(("Authorization", format!("Bearer {}", token)))
        .to_request();
    let logout_resp = call_service(&app, logout_req).await;
    assert_eq!(logout_resp.status(), 200);
    let logout_body: Value = read_body_json(logout_resp).await;
    assert_eq!(
        logout_body["message"], "Logged out successfully",
        "logout must return success message"
    );

    // Token is now invalid — /me must return 401
    let me_req = TestRequest::get()
        .uri("/api/v1/auth/me")
        .insert_header(("Authorization", format!("Bearer {}", token)))
        .to_request();
    let me_resp = call_service(&app, me_req).await;
    assert_eq!(
        me_resp.status(),
        401,
        "token must be invalid after logout"
    );
}

/// POST /api/v1/auth/verify with the correct password must return
/// { "verified": true }.
#[actix_web::test]
#[ignore = "requires TEST_DATABASE_URL"]
async fn test_verify_password_correct_returns_verified_true() {
    let pool = test_pool().await;
    let suffix = &Uuid::new_v4().to_string()[..8];
    let username = format!("auth_verify_{}", suffix);
    seed_user(&pool, &username, "Student").await;

    let app = init_service(
        App::new()
            .app_data(web::Data::new(pool))
            .configure(configure_routes),
    )
    .await;

    let login_req = TestRequest::post()
        .uri("/api/v1/auth/login")
        .set_json(serde_json::json!({ "username": &username, "password": "TestPass2024!!" }))
        .to_request();
    let login_body: Value = read_body_json(call_service(&app, login_req).await).await;
    let token = login_body["token"].as_str().unwrap().to_string();

    let verify_req = TestRequest::post()
        .uri("/api/v1/auth/verify")
        .insert_header(("Authorization", format!("Bearer {}", token)))
        .set_json(serde_json::json!({ "password": "TestPass2024!!" }))
        .to_request();
    let verify_resp = call_service(&app, verify_req).await;
    assert_eq!(verify_resp.status(), 200);
    let verify_body: Value = read_body_json(verify_resp).await;
    assert_eq!(
        verify_body["verified"], true,
        "verify with correct password must return verified: true"
    );
}

/// POST /api/v1/auth/verify with the wrong password must return 401
/// with an error field.
#[actix_web::test]
#[ignore = "requires TEST_DATABASE_URL"]
async fn test_verify_password_wrong_returns_401() {
    let pool = test_pool().await;
    let suffix = &Uuid::new_v4().to_string()[..8];
    let username = format!("auth_verify_bad_{}", suffix);
    seed_user(&pool, &username, "Student").await;

    let app = init_service(
        App::new()
            .app_data(web::Data::new(pool))
            .configure(configure_routes),
    )
    .await;

    let login_req = TestRequest::post()
        .uri("/api/v1/auth/login")
        .set_json(serde_json::json!({ "username": &username, "password": "TestPass2024!!" }))
        .to_request();
    let login_body: Value = read_body_json(call_service(&app, login_req).await).await;
    let token = login_body["token"].as_str().unwrap().to_string();

    let verify_req = TestRequest::post()
        .uri("/api/v1/auth/verify")
        .insert_header(("Authorization", format!("Bearer {}", token)))
        .set_json(serde_json::json!({ "password": "WrongPassword!" }))
        .to_request();
    let verify_resp = call_service(&app, verify_req).await;
    assert_eq!(verify_resp.status(), 401);
    let body: Value = read_body_json(verify_resp).await;
    assert!(body["error"].is_string(), "wrong verify must return error field");
}

// ---------------------------------------------------------------------------
// POST /auth/request-deletion
// ---------------------------------------------------------------------------

/// POST /api/v1/auth/request-deletion without auth must return 401.
#[actix_web::test]
#[ignore = "requires TEST_DATABASE_URL"]
async fn test_auth_request_deletion_requires_auth() {
    let pool = test_pool().await;
    let app = init_service(
        App::new()
            .app_data(web::Data::new(pool))
            .configure(configure_routes),
    )
    .await;

    let req = TestRequest::post()
        .uri("/api/v1/auth/request-deletion")
        .set_json(serde_json::json!({ "reason": "no longer needed" }))
        .to_request();
    let resp = call_service(&app, req).await;
    assert_eq!(resp.status(), 401, "/auth/request-deletion must require auth");
    let body: Value = read_body_json(resp).await;
    assert!(body["error"].is_string(), "401 must include an error field");
}

/// POST /api/v1/auth/request-deletion with a valid auth token submits the deletion
/// request and returns a 201 with a message field.
#[actix_web::test]
#[ignore = "requires TEST_DATABASE_URL"]
async fn test_auth_request_deletion_happy_path() {
    let pool = test_pool().await;
    let suffix = &Uuid::new_v4().to_string()[..8];
    let username = format!("auth_del_req_{}", suffix);
    seed_user(&pool, &username, "Student").await;

    let app = init_service(
        App::new()
            .app_data(web::Data::new(pool.clone()))
            .configure(configure_routes),
    )
    .await;

    let login_req = TestRequest::post()
        .uri("/api/v1/auth/login")
        .set_json(serde_json::json!({ "username": &username, "password": "TestPass2024!!" }))
        .to_request();
    let login_body: Value = read_body_json(call_service(&app, login_req).await).await;
    let token = login_body["token"].as_str().expect("token missing").to_string();

    let req = TestRequest::post()
        .uri("/api/v1/auth/request-deletion")
        .insert_header(("Authorization", format!("Bearer {}", token)))
        .set_json(serde_json::json!({ "reason": "I no longer need this account" }))
        .to_request();
    let resp = call_service(&app, req).await;
    assert_eq!(resp.status(), 201, "/auth/request-deletion must return 201 on success");
    let body: Value = read_body_json(resp).await;
    assert!(body["message"].is_string(), "201 response must include a message field");

    // Verify the deletion request was persisted.
    let uid: Uuid = sqlx::query_scalar("SELECT id FROM users WHERE username = $1")
        .bind(&username)
        .fetch_one(&pool)
        .await
        .expect("user must exist");
    let count: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM account_deletion_requests WHERE user_id = $1 AND status = 'pending'",
    )
    .bind(uid)
    .fetch_one(&pool)
    .await
    .expect("query failed");
    assert_eq!(count, 1, "one pending deletion request must exist in the DB");
}

/// POST /api/v1/auth/request-deletion a second time returns 409 (duplicate).
#[actix_web::test]
#[ignore = "requires TEST_DATABASE_URL"]
async fn test_auth_request_deletion_duplicate_returns_409() {
    let pool = test_pool().await;
    let suffix = &Uuid::new_v4().to_string()[..8];
    let username = format!("auth_del_dup_{}", suffix);
    seed_user(&pool, &username, "Student").await;

    let app = init_service(
        App::new()
            .app_data(web::Data::new(pool))
            .configure(configure_routes),
    )
    .await;

    let login_req = TestRequest::post()
        .uri("/api/v1/auth/login")
        .set_json(serde_json::json!({ "username": &username, "password": "TestPass2024!!" }))
        .to_request();
    let login_body: Value = read_body_json(call_service(&app, login_req).await).await;
    let token = login_body["token"].as_str().expect("token missing").to_string();

    // First request must succeed.
    let req = TestRequest::post()
        .uri("/api/v1/auth/request-deletion")
        .insert_header(("Authorization", format!("Bearer {}", token.clone())))
        .set_json(serde_json::json!({ "reason": "leaving" }))
        .to_request();
    let resp = call_service(&app, req).await;
    assert_eq!(resp.status(), 201, "first deletion request must return 201");

    // Second request must be rejected.
    let req = TestRequest::post()
        .uri("/api/v1/auth/request-deletion")
        .insert_header(("Authorization", format!("Bearer {}", token)))
        .set_json(serde_json::json!({ "reason": "again" }))
        .to_request();
    let resp = call_service(&app, req).await;
    assert_eq!(resp.status(), 409, "duplicate deletion request must return 409");
    let body: Value = read_body_json(resp).await;
    assert!(body["error"].is_string(), "409 must include an error field");
}
