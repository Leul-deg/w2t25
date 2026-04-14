/// Configuration endpoint payload tests.
///
/// Covers the two public config endpoints and all admin config endpoints:
/// list, update, history, campaigns list, and campaign update.
///
/// Requires a live PostgreSQL database:
///   TEST_DATABASE_URL=postgres://meridian:meridian@127.0.0.1:5433/meridian_seeded?sslmode=disable \
///     cargo test --test api_config_tests -- --include-ignored
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
        .expect("TEST_DATABASE_URL must be set to run api_config_tests");
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
// Public config endpoints
// ---------------------------------------------------------------------------

/// GET /api/v1/config/commerce without auth must return 401.
#[actix_web::test]
#[ignore = "requires TEST_DATABASE_URL"]
async fn test_commerce_summary_requires_auth() {
    let pool = test_pool().await;
    let app = init_service(
        App::new()
            .app_data(web::Data::new(pool))
            .configure(configure_routes),
    )
    .await;

    let req = TestRequest::get()
        .uri("/api/v1/config/commerce")
        .to_request();
    let resp = call_service(&app, req).await;
    assert_eq!(resp.status(), 401);
    let body: Value = read_body_json(resp).await;
    assert!(body["error"].is_string(), "401 must include an error field");
}

/// GET /api/v1/config/commerce returns shape with required fields.
#[actix_web::test]
#[ignore = "requires TEST_DATABASE_URL"]
async fn test_commerce_summary_returns_correct_shape() {
    let pool = test_pool().await;
    let suffix = &Uuid::new_v4().to_string()[..8];
    seed_user(&pool, &format!("cfg_com_{}", suffix), "Student").await;

    let app = init_service(
        App::new()
            .app_data(web::Data::new(pool.clone()))
            .configure(configure_routes),
    )
    .await;
    let token = login_token(&app, &format!("cfg_com_{}", suffix)).await;

    let req = TestRequest::get()
        .uri("/api/v1/config/commerce")
        .insert_header(("Authorization", format!("Bearer {}", token)))
        .to_request();
    let resp = call_service(&app, req).await;
    assert_eq!(resp.status(), 200);
    let body: Value = read_body_json(resp).await;

    assert!(
        body["shipping_fee_cents"].is_number(),
        "commerce.shipping_fee_cents must be number"
    );
    assert!(
        body["shipping_fee_display"].is_string(),
        "commerce.shipping_fee_display must be string"
    );
    assert!(
        body["points_rate_per_dollar"].is_number(),
        "commerce.points_rate_per_dollar must be number"
    );
    assert!(
        body["campaigns"].is_array(),
        "commerce.campaigns must be array"
    );
}

/// GET /api/v1/config/campaigns/{name}/status for an existing campaign returns
/// the name and enabled fields.
#[actix_web::test]
#[ignore = "requires TEST_DATABASE_URL"]
async fn test_campaign_status_returns_name_and_enabled() {
    let pool = test_pool().await;
    let suffix = &Uuid::new_v4().to_string()[..8];
    seed_user(&pool, &format!("cfg_camp_{}", suffix), "Student").await;

    let app = init_service(
        App::new()
            .app_data(web::Data::new(pool.clone()))
            .configure(configure_routes),
    )
    .await;
    let token = login_token(&app, &format!("cfg_camp_{}", suffix)).await;

    // The seeded database must have a "free_shipping" campaign (from migrations).
    let req = TestRequest::get()
        .uri("/api/v1/config/campaigns/free_shipping/status")
        .insert_header(("Authorization", format!("Bearer {}", token)))
        .to_request();
    let resp = call_service(&app, req).await;
    assert_eq!(resp.status(), 200);
    let body: Value = read_body_json(resp).await;

    assert_eq!(body["name"], "free_shipping", "status.name must match requested name");
    assert!(body["enabled"].is_boolean(), "status.enabled must be boolean");
}

/// GET /api/v1/config/campaigns/{name}/status for a nonexistent campaign
/// must return 404 with an error field.
#[actix_web::test]
#[ignore = "requires TEST_DATABASE_URL"]
async fn test_campaign_status_nonexistent_returns_404() {
    let pool = test_pool().await;
    let suffix = &Uuid::new_v4().to_string()[..8];
    seed_user(&pool, &format!("cfg_404_{}", suffix), "Student").await;

    let app = init_service(
        App::new()
            .app_data(web::Data::new(pool.clone()))
            .configure(configure_routes),
    )
    .await;
    let token = login_token(&app, &format!("cfg_404_{}", suffix)).await;

    let req = TestRequest::get()
        .uri("/api/v1/config/campaigns/nonexistent_campaign_xyz/status")
        .insert_header(("Authorization", format!("Bearer {}", token)))
        .to_request();
    let resp = call_service(&app, req).await;
    assert_eq!(resp.status(), 404);
    let body: Value = read_body_json(resp).await;
    assert!(body["error"].is_string(), "404 must include an error field");
}

// ---------------------------------------------------------------------------
// Admin config endpoints
// ---------------------------------------------------------------------------

/// GET /api/v1/admin/config without auth must return 401.
#[actix_web::test]
#[ignore = "requires TEST_DATABASE_URL"]
async fn test_admin_config_list_requires_auth() {
    let pool = test_pool().await;
    let app = init_service(
        App::new()
            .app_data(web::Data::new(pool))
            .configure(configure_routes),
    )
    .await;

    let req = TestRequest::get()
        .uri("/api/v1/admin/config")
        .to_request();
    let resp = call_service(&app, req).await;
    assert_eq!(resp.status(), 401);
    let body: Value = read_body_json(resp).await;
    assert!(body["error"].is_string(), "401 must include an error field");
}

/// GET /api/v1/admin/config by a non-admin must return 403.
#[actix_web::test]
#[ignore = "requires TEST_DATABASE_URL"]
async fn test_admin_config_list_requires_admin_role() {
    let pool = test_pool().await;
    let suffix = &Uuid::new_v4().to_string()[..8];
    seed_user(&pool, &format!("cfg_nadmin_{}", suffix), "Student").await;

    let app = init_service(
        App::new()
            .app_data(web::Data::new(pool.clone()))
            .configure(configure_routes),
    )
    .await;
    let token = login_token(&app, &format!("cfg_nadmin_{}", suffix)).await;

    let req = TestRequest::get()
        .uri("/api/v1/admin/config")
        .insert_header(("Authorization", format!("Bearer {}", token)))
        .to_request();
    let resp = call_service(&app, req).await;
    assert_eq!(resp.status(), 403, "non-admin must receive 403");
    let body: Value = read_body_json(resp).await;
    assert!(body["error"].is_string(), "403 must include an error field");
}

/// GET /api/v1/admin/config returns an array where each item has the required
/// config value fields.
#[actix_web::test]
#[ignore = "requires TEST_DATABASE_URL"]
async fn test_admin_config_list_returns_array_with_required_fields() {
    let pool = test_pool().await;
    let suffix = &Uuid::new_v4().to_string()[..8];
    let admin_name = format!("cfg_admin_{}", suffix);
    seed_super_admin(&pool, &admin_name).await;

    let app = init_service(
        App::new()
            .app_data(web::Data::new(pool.clone()))
            .configure(configure_routes),
    )
    .await;
    let token = login_token(&app, &admin_name).await;

    let req = TestRequest::get()
        .uri("/api/v1/admin/config")
        .insert_header(("Authorization", format!("Bearer {}", token)))
        .to_request();
    let resp = call_service(&app, req).await;
    assert_eq!(resp.status(), 200);
    let body: Value = read_body_json(resp).await;
    let items = body.as_array().expect("admin config must be a JSON array");
    assert!(!items.is_empty(), "config list must be non-empty after migrations");

    for item in items {
        assert!(item["id"].is_string(), "config.id must be string");
        assert!(item["key"].is_string(), "config.key must be string");
        assert!(item["value_type"].is_string(), "config.value_type must be string");
        assert!(item["scope"].is_string(), "config.scope must be string");
    }
}

/// POST /api/v1/admin/config/values/{key} updates a value and returns old/new shape.
#[actix_web::test]
#[ignore = "requires TEST_DATABASE_URL"]
async fn test_admin_config_update_value_returns_old_and_new() {
    let pool = test_pool().await;
    let suffix = &Uuid::new_v4().to_string()[..8];
    let admin_name = format!("cfg_upd_admin_{}", suffix);
    seed_super_admin(&pool, &admin_name).await;

    let app = init_service(
        App::new()
            .app_data(web::Data::new(pool.clone()))
            .configure(configure_routes),
    )
    .await;
    let token = login_token(&app, &admin_name).await;

    // shipping_fee_cents is a seeded integer config key
    let req = TestRequest::post()
        .uri("/api/v1/admin/config/values/shipping_fee_cents")
        .insert_header(("Authorization", format!("Bearer {}", token)))
        .set_json(serde_json::json!({ "value": "699", "reason": "test update" }))
        .to_request();
    let resp = call_service(&app, req).await;
    assert_eq!(resp.status(), 200);
    let body: Value = read_body_json(resp).await;

    assert_eq!(body["key"], "shipping_fee_cents", "response must echo the key");
    assert!(body["old_value"].is_string() || body["old_value"].is_null(),
        "response must include old_value");
    assert_eq!(body["new_value"], "699", "response must include new_value");
    assert!(body["changed_by"].is_string(), "response must include changed_by");
    assert!(body["message"].is_string(), "response must include message");
}

/// POST /api/v1/admin/config/values/{key} with a wrong-type value must return 422.
#[actix_web::test]
#[ignore = "requires TEST_DATABASE_URL"]
async fn test_admin_config_update_value_wrong_type_returns_422() {
    let pool = test_pool().await;
    let suffix = &Uuid::new_v4().to_string()[..8];
    let admin_name = format!("cfg_422_admin_{}", suffix);
    seed_super_admin(&pool, &admin_name).await;

    let app = init_service(
        App::new()
            .app_data(web::Data::new(pool.clone()))
            .configure(configure_routes),
    )
    .await;
    let token = login_token(&app, &admin_name).await;

    // shipping_fee_cents is declared as integer; sending "not_a_number" is invalid
    let req = TestRequest::post()
        .uri("/api/v1/admin/config/values/shipping_fee_cents")
        .insert_header(("Authorization", format!("Bearer {}", token)))
        .set_json(serde_json::json!({ "value": "not_a_number" }))
        .to_request();
    let resp = call_service(&app, req).await;
    assert_eq!(resp.status(), 422, "wrong type must return 422");
    let body: Value = read_body_json(resp).await;
    assert!(body["error"].is_string(), "422 must include an error field");
}

/// POST /api/v1/admin/config/values/{key} for a nonexistent key must return 404.
#[actix_web::test]
#[ignore = "requires TEST_DATABASE_URL"]
async fn test_admin_config_update_nonexistent_key_returns_404() {
    let pool = test_pool().await;
    let suffix = &Uuid::new_v4().to_string()[..8];
    let admin_name = format!("cfg_nokey_admin_{}", suffix);
    seed_super_admin(&pool, &admin_name).await;

    let app = init_service(
        App::new()
            .app_data(web::Data::new(pool.clone()))
            .configure(configure_routes),
    )
    .await;
    let token = login_token(&app, &admin_name).await;

    let req = TestRequest::post()
        .uri("/api/v1/admin/config/values/nonexistent_key_xyz")
        .insert_header(("Authorization", format!("Bearer {}", token)))
        .set_json(serde_json::json!({ "value": "123" }))
        .to_request();
    let resp = call_service(&app, req).await;
    assert_eq!(resp.status(), 404);
    let body: Value = read_body_json(resp).await;
    assert!(body["error"].is_string(), "404 must include an error field");
}

/// GET /api/v1/admin/config/history returns a JSON array with history entries.
#[actix_web::test]
#[ignore = "requires TEST_DATABASE_URL"]
async fn test_admin_config_history_returns_array() {
    let pool = test_pool().await;
    let suffix = &Uuid::new_v4().to_string()[..8];
    let admin_name = format!("cfg_hist_admin_{}", suffix);
    seed_super_admin(&pool, &admin_name).await;

    let app = init_service(
        App::new()
            .app_data(web::Data::new(pool.clone()))
            .configure(configure_routes),
    )
    .await;
    let token = login_token(&app, &admin_name).await;

    // Create a history entry by updating a config key
    let _ = call_service(
        &app,
        TestRequest::post()
            .uri("/api/v1/admin/config/values/shipping_fee_cents")
            .insert_header(("Authorization", format!("Bearer {}", token)))
            .set_json(serde_json::json!({ "value": "695", "reason": "history test" }))
            .to_request(),
    )
    .await;

    let req = TestRequest::get()
        .uri("/api/v1/admin/config/history")
        .insert_header(("Authorization", format!("Bearer {}", token)))
        .to_request();
    let resp = call_service(&app, req).await;
    assert_eq!(resp.status(), 200);
    let body: Value = read_body_json(resp).await;
    let items = body.as_array().expect("config history must be a JSON array");
    assert!(!items.is_empty(), "history must be non-empty after an update");

    let first = &items[0];
    assert!(first["id"].is_string(), "history.id must be string");
    assert!(first["config_key"].is_string(), "history.config_key must be string");
    assert!(first["changed_at"].is_string(), "history.changed_at must be string");
}

/// GET /api/v1/admin/config/campaigns returns a JSON array of campaign toggles.
#[actix_web::test]
#[ignore = "requires TEST_DATABASE_URL"]
async fn test_admin_list_campaigns_returns_array_with_required_fields() {
    let pool = test_pool().await;
    let suffix = &Uuid::new_v4().to_string()[..8];
    let admin_name = format!("cfg_camps_admin_{}", suffix);
    seed_super_admin(&pool, &admin_name).await;

    let app = init_service(
        App::new()
            .app_data(web::Data::new(pool.clone()))
            .configure(configure_routes),
    )
    .await;
    let token = login_token(&app, &admin_name).await;

    let req = TestRequest::get()
        .uri("/api/v1/admin/config/campaigns")
        .insert_header(("Authorization", format!("Bearer {}", token)))
        .to_request();
    let resp = call_service(&app, req).await;
    assert_eq!(resp.status(), 200);
    let body: Value = read_body_json(resp).await;
    let items = body.as_array().expect("campaigns must be a JSON array");
    assert!(!items.is_empty(), "seeded DB must have campaigns");

    for item in items {
        assert!(item["id"].is_string(), "campaign.id must be string");
        assert!(item["name"].is_string(), "campaign.name must be string");
        assert!(item["enabled"].is_boolean(), "campaign.enabled must be boolean");
    }
}

/// POST /api/v1/admin/config/campaigns/{name} updates campaign and returns shape.
#[actix_web::test]
#[ignore = "requires TEST_DATABASE_URL"]
async fn test_admin_update_campaign_returns_correct_shape() {
    let pool = test_pool().await;
    let suffix = &Uuid::new_v4().to_string()[..8];
    let admin_name = format!("cfg_updcamp_admin_{}", suffix);
    seed_super_admin(&pool, &admin_name).await;

    let app = init_service(
        App::new()
            .app_data(web::Data::new(pool.clone()))
            .configure(configure_routes),
    )
    .await;
    let token = login_token(&app, &admin_name).await;

    let req = TestRequest::post()
        .uri("/api/v1/admin/config/campaigns/free_shipping")
        .insert_header(("Authorization", format!("Bearer {}", token)))
        .set_json(serde_json::json!({ "enabled": false }))
        .to_request();
    let resp = call_service(&app, req).await;
    assert_eq!(resp.status(), 200);
    let body: Value = read_body_json(resp).await;

    assert_eq!(body["name"], "free_shipping", "response must echo name");
    assert_eq!(body["enabled"], false, "response must reflect new enabled state");
    assert!(body["changed_by"].is_string(), "response must include changed_by");
    assert!(body["message"].is_string(), "response must include message");
}

/// POST /api/v1/admin/config/campaigns/{name} for nonexistent campaign → 404.
#[actix_web::test]
#[ignore = "requires TEST_DATABASE_URL"]
async fn test_admin_update_nonexistent_campaign_returns_404() {
    let pool = test_pool().await;
    let suffix = &Uuid::new_v4().to_string()[..8];
    let admin_name = format!("cfg_nocamp_admin_{}", suffix);
    seed_super_admin(&pool, &admin_name).await;

    let app = init_service(
        App::new()
            .app_data(web::Data::new(pool.clone()))
            .configure(configure_routes),
    )
    .await;
    let token = login_token(&app, &admin_name).await;

    let req = TestRequest::post()
        .uri("/api/v1/admin/config/campaigns/campaign_does_not_exist")
        .insert_header(("Authorization", format!("Bearer {}", token)))
        .set_json(serde_json::json!({ "enabled": true }))
        .to_request();
    let resp = call_service(&app, req).await;
    assert_eq!(resp.status(), 404);
    let body: Value = read_body_json(resp).await;
    assert!(body["error"].is_string(), "404 must include an error field");
}
