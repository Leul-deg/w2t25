/// Notification preference endpoint payload tests.
///
/// Verifies that GET /preferences returns correct defaults and that
/// PATCH /preferences persists changes and rejects invalid values.
///
/// Requires a live PostgreSQL database:
///   TEST_DATABASE_URL=postgres://meridian:meridian@127.0.0.1:5433/meridian_seeded?sslmode=disable \
///     cargo test --test api_preferences_tests -- --include-ignored
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
        .expect("TEST_DATABASE_URL must be set to run api_preferences_tests");
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
// GET /preferences — auth + shape
// ---------------------------------------------------------------------------

/// GET /api/v1/preferences without auth must return 401.
#[actix_web::test]
#[ignore = "requires TEST_DATABASE_URL"]
async fn test_get_preferences_requires_auth() {
    let pool = test_pool().await;
    let app = init_service(
        App::new()
            .app_data(web::Data::new(pool))
            .configure(configure_routes),
    )
    .await;

    let req = TestRequest::get().uri("/api/v1/preferences").to_request();
    let resp = call_service(&app, req).await;
    assert_eq!(resp.status(), 401);
    let body: Value = read_body_json(resp).await;
    assert!(body["error"].is_string(), "401 must include an error field");
}

/// GET /api/v1/preferences for a new user must return 200 with the correct
/// default values: notif_checkin=true, dnd_enabled=false, inbox_frequency="immediate",
/// dnd_start="21:00", dnd_end="06:00".
#[actix_web::test]
#[ignore = "requires TEST_DATABASE_URL"]
async fn test_get_preferences_returns_defaults_for_new_user() {
    let pool = test_pool().await;
    let suffix = &Uuid::new_v4().to_string()[..8];
    let username = format!("prefs_new_{}", suffix);
    seed_user(&pool, &username, "Student").await;

    let app = init_service(
        App::new()
            .app_data(web::Data::new(pool))
            .configure(configure_routes),
    )
    .await;
    let token = login_token(&app, &username).await;

    let req = TestRequest::get()
        .uri("/api/v1/preferences")
        .insert_header(("Authorization", format!("Bearer {}", token)))
        .to_request();
    let resp = call_service(&app, req).await;
    assert_eq!(resp.status(), 200, "GET /preferences must return 200");
    let body: Value = read_body_json(resp).await;

    // Required fields present.
    assert!(body["notif_checkin"].is_boolean(), "notif_checkin must be boolean");
    assert!(body["notif_order"].is_boolean(), "notif_order must be boolean");
    assert!(body["notif_general"].is_boolean(), "notif_general must be boolean");
    assert!(body["dnd_enabled"].is_boolean(), "dnd_enabled must be boolean");
    assert!(body["dnd_start"].is_string(), "dnd_start must be string");
    assert!(body["dnd_end"].is_string(), "dnd_end must be string");
    assert!(body["inbox_frequency"].is_string(), "inbox_frequency must be string");

    // Default values.
    assert_eq!(body["notif_checkin"], true, "default notif_checkin must be true");
    assert_eq!(body["dnd_enabled"], false, "default dnd_enabled must be false");
    assert_eq!(body["inbox_frequency"], "immediate", "default inbox_frequency must be 'immediate'");
    assert_eq!(body["dnd_start"], "21:00", "default dnd_start must be '21:00'");
    assert_eq!(body["dnd_end"], "06:00", "default dnd_end must be '06:00'");
}

// ---------------------------------------------------------------------------
// PATCH /preferences — auth + happy path + invalid value
// ---------------------------------------------------------------------------

/// PATCH /api/v1/preferences without auth must return 401.
#[actix_web::test]
#[ignore = "requires TEST_DATABASE_URL"]
async fn test_patch_preferences_requires_auth() {
    let pool = test_pool().await;
    let app = init_service(
        App::new()
            .app_data(web::Data::new(pool))
            .configure(configure_routes),
    )
    .await;

    let req = TestRequest::patch()
        .uri("/api/v1/preferences")
        .set_json(serde_json::json!({ "inbox_frequency": "daily" }))
        .to_request();
    let resp = call_service(&app, req).await;
    assert_eq!(resp.status(), 401);
    let body: Value = read_body_json(resp).await;
    assert!(body["error"].is_string(), "401 must include an error field");
}

/// PATCH /api/v1/preferences with valid fields returns 200 and reflects the
/// updated values in both the PATCH response and a subsequent GET.
#[actix_web::test]
#[ignore = "requires TEST_DATABASE_URL"]
async fn test_patch_preferences_persists_changes() {
    let pool = test_pool().await;
    let suffix = &Uuid::new_v4().to_string()[..8];
    let username = format!("prefs_upd_{}", suffix);
    seed_user(&pool, &username, "Teacher").await;

    let app = init_service(
        App::new()
            .app_data(web::Data::new(pool))
            .configure(configure_routes),
    )
    .await;
    let token = login_token(&app, &username).await;

    // Apply a PATCH.
    let req = TestRequest::patch()
        .uri("/api/v1/preferences")
        .insert_header(("Authorization", format!("Bearer {}", token.clone())))
        .set_json(serde_json::json!({
            "inbox_frequency": "daily",
            "dnd_enabled": true,
            "dnd_start": "22:00",
            "dnd_end": "07:00",
            "notif_order": false
        }))
        .to_request();
    let resp = call_service(&app, req).await;
    assert_eq!(resp.status(), 200, "PATCH /preferences must return 200");
    let patch_body: Value = read_body_json(resp).await;
    assert_eq!(patch_body["inbox_frequency"], "daily", "PATCH response must reflect updated frequency");
    assert_eq!(patch_body["dnd_enabled"], true, "PATCH response must reflect updated dnd_enabled");

    // Verify with a subsequent GET.
    let req = TestRequest::get()
        .uri("/api/v1/preferences")
        .insert_header(("Authorization", format!("Bearer {}", token)))
        .to_request();
    let get_body: Value = read_body_json(call_service(&app, req).await).await;
    assert_eq!(get_body["inbox_frequency"], "daily", "GET must return the persisted frequency");
    assert_eq!(get_body["dnd_enabled"], true, "GET must return the persisted dnd_enabled");
    assert_eq!(get_body["dnd_start"], "22:00", "GET must return the persisted dnd_start");
    assert_eq!(get_body["dnd_end"], "07:00", "GET must return the persisted dnd_end");
    assert_eq!(get_body["notif_order"], false, "GET must return the persisted notif_order");
    // Untouched field must retain its default.
    assert_eq!(get_body["notif_checkin"], true, "unpatched notif_checkin must stay true");
}

/// PATCH /api/v1/preferences with an invalid inbox_frequency value must return 422.
#[actix_web::test]
#[ignore = "requires TEST_DATABASE_URL"]
async fn test_patch_preferences_invalid_frequency_returns_422() {
    let pool = test_pool().await;
    let suffix = &Uuid::new_v4().to_string()[..8];
    let username = format!("prefs_inv_{}", suffix);
    seed_user(&pool, &username, "Student").await;

    let app = init_service(
        App::new()
            .app_data(web::Data::new(pool))
            .configure(configure_routes),
    )
    .await;
    let token = login_token(&app, &username).await;

    let req = TestRequest::patch()
        .uri("/api/v1/preferences")
        .insert_header(("Authorization", format!("Bearer {}", token)))
        .set_json(serde_json::json!({ "inbox_frequency": "hourly" }))
        .to_request();
    let resp = call_service(&app, req).await;
    assert_eq!(resp.status(), 422, "invalid inbox_frequency must return 422");
    let body: Value = read_body_json(resp).await;
    assert!(body["error"].is_string(), "422 must include an error field");
}
