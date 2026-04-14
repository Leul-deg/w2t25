/// Log endpoint payload tests.
///
/// Covers all four log endpoints (audit, access, errors, prune), checking auth
/// enforcement, response shape, pagination fields, and level validation.
///
/// Requires a live PostgreSQL database:
///   TEST_DATABASE_URL=postgres://meridian:meridian@127.0.0.1:5433/meridian_seeded?sslmode=disable \
///     cargo test --test api_logs_tests -- --include-ignored
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
        .expect("TEST_DATABASE_URL must be set to run api_logs_tests");
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
        .expect("user missing");
    sqlx::query(
        "INSERT INTO user_roles (user_id, role_id) VALUES ($1, $2) ON CONFLICT DO NOTHING",
    )
    .bind(uid)
    .bind(role_id)
    .execute(pool)
    .await
    .expect("assign role failed");
}

async fn seed_super_admin(pool: &PgPool, username: &str) {
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
// Auth enforcement
// ---------------------------------------------------------------------------

/// GET /api/v1/logs/audit without a token must return 401 with an error body.
#[actix_web::test]
#[ignore = "requires TEST_DATABASE_URL"]
async fn test_audit_logs_requires_auth() {
    let pool = test_pool().await;
    let app = init_service(
        App::new()
            .app_data(web::Data::new(pool))
            .configure(configure_routes),
    )
    .await;

    let req = TestRequest::get().uri("/api/v1/logs/audit").to_request();
    let resp = call_service(&app, req).await;
    assert_eq!(resp.status(), 401);
    let body: Value = read_body_json(resp).await;
    assert!(body["error"].is_string(), "401 must include an error field");
}

/// GET /api/v1/logs/audit by a non-admin must return 403 with an error body.
#[actix_web::test]
#[ignore = "requires TEST_DATABASE_URL"]
async fn test_audit_logs_requires_admin_role() {
    let pool = test_pool().await;
    let suffix = &Uuid::new_v4().to_string()[..8];
    seed_user(&pool, &format!("log_nadmin_{}", suffix), "Student").await;

    let app = init_service(
        App::new()
            .app_data(web::Data::new(pool.clone()))
            .configure(configure_routes),
    )
    .await;
    let token = login_token(&app, &format!("log_nadmin_{}", suffix)).await;

    let req = TestRequest::get()
        .uri("/api/v1/logs/audit")
        .insert_header(("Authorization", format!("Bearer {}", token)))
        .to_request();
    let resp = call_service(&app, req).await;
    assert_eq!(resp.status(), 403, "non-admin must receive 403");
    let body: Value = read_body_json(resp).await;
    assert!(body["error"].is_string(), "403 must include an error field");
}

// ---------------------------------------------------------------------------
// Audit logs
// ---------------------------------------------------------------------------

/// GET /api/v1/logs/audit returns the expected paginated shape.
#[actix_web::test]
#[ignore = "requires TEST_DATABASE_URL"]
async fn test_audit_logs_returns_paginated_shape() {
    let pool = test_pool().await;
    let suffix = &Uuid::new_v4().to_string()[..8];
    let admin_name = format!("log_audit_admin_{}", suffix);
    seed_super_admin(&pool, &admin_name).await;

    let app = init_service(
        App::new()
            .app_data(web::Data::new(pool.clone()))
            .configure(configure_routes),
    )
    .await;
    let token = login_token(&app, &admin_name).await;

    let req = TestRequest::get()
        .uri("/api/v1/logs/audit?limit=10&offset=0")
        .insert_header(("Authorization", format!("Bearer {}", token)))
        .to_request();
    let resp = call_service(&app, req).await;
    assert_eq!(resp.status(), 200);
    let body: Value = read_body_json(resp).await;

    assert!(body["count"].is_number(), "audit.count must be number");
    assert!(body["limit"].is_number(), "audit.limit must be number");
    assert!(body["offset"].is_number(), "audit.offset must be number");
    assert!(body["rows"].is_array(), "audit.rows must be array");
    assert_eq!(body["limit"], 10, "limit must match requested value");
    assert_eq!(body["offset"], 0, "offset must match requested value");

    // Each row must have required fields
    for row in body["rows"].as_array().unwrap() {
        assert!(row["id"].is_string(), "audit row.id must be string");
        assert!(row["action"].is_string(), "audit row.action must be string");
        assert!(row["entity_type"].is_string(), "audit row.entity_type must be string");
        assert!(row["created_at"].is_string(), "audit row.created_at must be string");
    }
}

// ---------------------------------------------------------------------------
// Access logs
// ---------------------------------------------------------------------------

/// GET /api/v1/logs/access returns the expected paginated shape.
#[actix_web::test]
#[ignore = "requires TEST_DATABASE_URL"]
async fn test_access_logs_returns_paginated_shape() {
    let pool = test_pool().await;
    let suffix = &Uuid::new_v4().to_string()[..8];
    let admin_name = format!("log_access_admin_{}", suffix);
    seed_super_admin(&pool, &admin_name).await;

    let app = init_service(
        App::new()
            .app_data(web::Data::new(pool.clone()))
            .configure(configure_routes),
    )
    .await;
    let token = login_token(&app, &admin_name).await;

    let req = TestRequest::get()
        .uri("/api/v1/logs/access?limit=5")
        .insert_header(("Authorization", format!("Bearer {}", token)))
        .to_request();
    let resp = call_service(&app, req).await;
    assert_eq!(resp.status(), 200);
    let body: Value = read_body_json(resp).await;

    assert!(body["count"].is_number(), "access.count must be number");
    assert!(body["limit"].is_number(), "access.limit must be number");
    assert!(body["offset"].is_number(), "access.offset must be number");
    assert!(body["rows"].is_array(), "access.rows must be array");
    assert_eq!(body["limit"], 5, "limit must match requested value");

    for row in body["rows"].as_array().unwrap() {
        assert!(row["id"].is_string(), "access row.id must be string");
        assert!(row["action"].is_string(), "access row.action must be string");
        assert!(row["success"].is_boolean(), "access row.success must be boolean");
        assert!(row["created_at"].is_string(), "access row.created_at must be string");
    }
}

/// GET /api/v1/logs/access without auth must return 401.
#[actix_web::test]
#[ignore = "requires TEST_DATABASE_URL"]
async fn test_access_logs_requires_auth() {
    let pool = test_pool().await;
    let app = init_service(
        App::new()
            .app_data(web::Data::new(pool))
            .configure(configure_routes),
    )
    .await;

    let req = TestRequest::get().uri("/api/v1/logs/access").to_request();
    let resp = call_service(&app, req).await;
    assert_eq!(resp.status(), 401);
    let body: Value = read_body_json(resp).await;
    assert!(body["error"].is_string(), "401 must include an error field");
}

// ---------------------------------------------------------------------------
// Error logs
// ---------------------------------------------------------------------------

/// GET /api/v1/logs/errors returns the expected paginated shape.
#[actix_web::test]
#[ignore = "requires TEST_DATABASE_URL"]
async fn test_error_logs_returns_paginated_shape() {
    let pool = test_pool().await;
    let suffix = &Uuid::new_v4().to_string()[..8];
    let admin_name = format!("log_err_admin_{}", suffix);
    seed_super_admin(&pool, &admin_name).await;

    let app = init_service(
        App::new()
            .app_data(web::Data::new(pool.clone()))
            .configure(configure_routes),
    )
    .await;
    let token = login_token(&app, &admin_name).await;

    let req = TestRequest::get()
        .uri("/api/v1/logs/errors?limit=20")
        .insert_header(("Authorization", format!("Bearer {}", token)))
        .to_request();
    let resp = call_service(&app, req).await;
    assert_eq!(resp.status(), 200);
    let body: Value = read_body_json(resp).await;

    assert!(body["count"].is_number(), "errors.count must be number");
    assert!(body["limit"].is_number(), "errors.limit must be number");
    assert!(body["offset"].is_number(), "errors.offset must be number");
    assert!(body["rows"].is_array(), "errors.rows must be array");
    assert_eq!(body["limit"], 20, "limit must match requested value");

    for row in body["rows"].as_array().unwrap() {
        assert!(row["id"].is_string(), "error row.id must be string");
        assert!(row["level"].is_string(), "error row.level must be string");
        assert!(row["message"].is_string(), "error row.message must be string");
        assert!(row["created_at"].is_string(), "error row.created_at must be string");
    }
}

/// GET /api/v1/logs/errors with an invalid level must return 422 with error body.
#[actix_web::test]
#[ignore = "requires TEST_DATABASE_URL"]
async fn test_error_logs_invalid_level_returns_422() {
    let pool = test_pool().await;
    let suffix = &Uuid::new_v4().to_string()[..8];
    let admin_name = format!("log_422_admin_{}", suffix);
    seed_super_admin(&pool, &admin_name).await;

    let app = init_service(
        App::new()
            .app_data(web::Data::new(pool.clone()))
            .configure(configure_routes),
    )
    .await;
    let token = login_token(&app, &admin_name).await;

    let req = TestRequest::get()
        .uri("/api/v1/logs/errors?level=critical") // 'critical' is not a valid level
        .insert_header(("Authorization", format!("Bearer {}", token)))
        .to_request();
    let resp = call_service(&app, req).await;
    assert_eq!(resp.status(), 422, "invalid level must return 422");
    let body: Value = read_body_json(resp).await;
    assert!(body["error"].is_string(), "422 must include an error field");
}

/// GET /api/v1/logs/errors without auth must return 401.
#[actix_web::test]
#[ignore = "requires TEST_DATABASE_URL"]
async fn test_error_logs_requires_auth() {
    let pool = test_pool().await;
    let app = init_service(
        App::new()
            .app_data(web::Data::new(pool))
            .configure(configure_routes),
    )
    .await;

    let req = TestRequest::get().uri("/api/v1/logs/errors").to_request();
    let resp = call_service(&app, req).await;
    assert_eq!(resp.status(), 401);
    let body: Value = read_body_json(resp).await;
    assert!(body["error"].is_string(), "401 must include an error field");
}

// ---------------------------------------------------------------------------
// Prune
// ---------------------------------------------------------------------------

/// POST /api/v1/logs/prune without auth must return 401.
#[actix_web::test]
#[ignore = "requires TEST_DATABASE_URL"]
async fn test_prune_logs_requires_auth() {
    let pool = test_pool().await;
    let app = init_service(
        App::new()
            .app_data(web::Data::new(pool))
            .configure(configure_routes),
    )
    .await;

    let req = TestRequest::post().uri("/api/v1/logs/prune").to_request();
    let resp = call_service(&app, req).await;
    assert_eq!(resp.status(), 401);
    let body: Value = read_body_json(resp).await;
    assert!(body["error"].is_string(), "401 must include an error field");
}

/// POST /api/v1/logs/prune by admin returns 200 with a message field.
#[actix_web::test]
#[ignore = "requires TEST_DATABASE_URL"]
async fn test_prune_logs_returns_message() {
    let pool = test_pool().await;
    let suffix = &Uuid::new_v4().to_string()[..8];
    let admin_name = format!("log_prune_admin_{}", suffix);
    seed_super_admin(&pool, &admin_name).await;

    let app = init_service(
        App::new()
            .app_data(web::Data::new(pool.clone()))
            .configure(configure_routes),
    )
    .await;
    let token = login_token(&app, &admin_name).await;

    let req = TestRequest::post()
        .uri("/api/v1/logs/prune")
        .insert_header(("Authorization", format!("Bearer {}", token)))
        .to_request();
    let resp = call_service(&app, req).await;
    assert_eq!(resp.status(), 200);
    let body: Value = read_body_json(resp).await;
    assert!(body["message"].is_string(), "prune must return a message field");
}
