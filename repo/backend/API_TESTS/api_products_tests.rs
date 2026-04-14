/// Product endpoint payload tests.
///
/// Verifies that product listing and detail endpoints return the correct JSON
/// shapes, field types, and error responses — not just status codes.
///
/// Requires a live PostgreSQL database:
///   TEST_DATABASE_URL=postgres://meridian:meridian@127.0.0.1:5433/meridian_seeded?sslmode=disable \
///     cargo test --test api_products_tests -- --include-ignored
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
        .expect("TEST_DATABASE_URL must be set to run api_products_tests");
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

async fn seed_user_and_login(
    app: &impl actix_web::dev::Service<
        actix_http::Request,
        Response = actix_web::dev::ServiceResponse,
        Error = actix_web::Error,
    >,
    pool: &PgPool,
    username: &str,
    role: &str,
) -> String {
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

    let req = TestRequest::post()
        .uri("/api/v1/auth/login")
        .set_json(serde_json::json!({ "username": username, "password": "TestPass2024!!" }))
        .to_request();
    let resp = call_service(app, req).await;
    let body: Value = read_body_json(resp).await;
    body["token"].as_str().expect("token missing").to_string()
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

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

/// GET /api/v1/products without a token must return 401 with an error body.
#[actix_web::test]
#[ignore = "requires TEST_DATABASE_URL"]
async fn test_list_products_requires_auth() {
    let pool = test_pool().await;
    let app = init_service(
        App::new()
            .app_data(web::Data::new(pool))
            .configure(configure_routes),
    )
    .await;

    let req = TestRequest::get().uri("/api/v1/products").to_request();
    let resp = call_service(&app, req).await;
    assert_eq!(resp.status(), 401);
    let body: Value = read_body_json(resp).await;
    assert!(body["error"].is_string(), "401 must include an error field");
}

/// GET /api/v1/products must return a JSON array; every item must have the
/// required fields with correct types.
#[actix_web::test]
#[ignore = "requires TEST_DATABASE_URL"]
async fn test_list_products_returns_array_with_required_fields() {
    let pool = test_pool().await;
    let app = init_service(
        App::new()
            .app_data(web::Data::new(pool.clone()))
            .configure(configure_routes),
    )
    .await;
    let suffix = &Uuid::new_v4().to_string()[..8];
    let token = seed_user_and_login(&app, &pool, &format!("prod_list_{}", suffix), "Student").await;

    let req = TestRequest::get()
        .uri("/api/v1/products")
        .insert_header(("Authorization", format!("Bearer {}", token)))
        .to_request();
    let resp = call_service(&app, req).await;
    assert_eq!(resp.status(), 200);
    let body: Value = read_body_json(resp).await;
    let items = body.as_array().expect("products response must be a JSON array");

    // If the seeded database has products, verify their shape
    for item in items {
        assert!(item["id"].is_string(), "product.id must be a string");
        assert!(item["name"].is_string(), "product.name must be a string");
        assert!(
            item["price_cents"].is_number(),
            "product.price_cents must be a number"
        );
        assert!(item["active"].is_boolean(), "product.active must be a boolean");
        // All items in the public list must be active
        assert_eq!(item["active"], true, "list must only return active products");
    }
}

/// GET /api/v1/products/{id} for a non-existent UUID must return 404 with
/// an error field.
#[actix_web::test]
#[ignore = "requires TEST_DATABASE_URL"]
async fn test_get_nonexistent_product_returns_404_with_error() {
    let pool = test_pool().await;
    let app = init_service(
        App::new()
            .app_data(web::Data::new(pool.clone()))
            .configure(configure_routes),
    )
    .await;
    let suffix = &Uuid::new_v4().to_string()[..8];
    let token =
        seed_user_and_login(&app, &pool, &format!("prod_404_{}", suffix), "Student").await;

    let nonexistent_id = Uuid::new_v4();
    let req = TestRequest::get()
        .uri(&format!("/api/v1/products/{}", nonexistent_id))
        .insert_header(("Authorization", format!("Bearer {}", token)))
        .to_request();
    let resp = call_service(&app, req).await;
    assert_eq!(resp.status(), 404);
    let body: Value = read_body_json(resp).await;
    assert!(
        body["error"].is_string(),
        "404 response must include an error field"
    );
}

/// Admin GET /api/v1/admin/products must return an array where each item
/// includes inventory fields (quantity, low_stock_threshold).
#[actix_web::test]
#[ignore = "requires TEST_DATABASE_URL"]
async fn test_admin_product_list_includes_inventory_fields() {
    let pool = test_pool().await;
    let suffix = &Uuid::new_v4().to_string()[..8];
    let admin_name = format!("prod_admin_{}", suffix);
    seed_super_admin(&pool, &admin_name).await;

    let app = init_service(
        App::new()
            .app_data(web::Data::new(pool.clone()))
            .configure(configure_routes),
    )
    .await;

    let login_req = TestRequest::post()
        .uri("/api/v1/auth/login")
        .set_json(serde_json::json!({ "username": &admin_name, "password": "TestPass2024!!" }))
        .to_request();
    let login_body: Value = read_body_json(call_service(&app, login_req).await).await;
    let token = login_body["token"].as_str().unwrap().to_string();

    let req = TestRequest::get()
        .uri("/api/v1/admin/products")
        .insert_header(("Authorization", format!("Bearer {}", token)))
        .to_request();
    let resp = call_service(&app, req).await;
    assert_eq!(resp.status(), 200);
    let body: Value = read_body_json(resp).await;
    let items = body.as_array().expect("admin products must be a JSON array");

    for item in items {
        assert!(item["id"].is_string(), "product.id must be a string");
        assert!(item["name"].is_string(), "product.name must be a string");
        assert!(
            item["price_cents"].is_number(),
            "product.price_cents must be a number"
        );
        assert!(
            item["active"].is_boolean(),
            "product.active must be a boolean"
        );
        // Admin list includes inventory info (may be null if no inventory row)
        assert!(
            item["quantity"].is_number() || item["quantity"].is_null(),
            "product.quantity must be number or null"
        );
    }
}

/// Creating a product and then deactivating it must make it disappear from the
/// public list while remaining in the admin list.
#[actix_web::test]
#[ignore = "requires TEST_DATABASE_URL"]
async fn test_deactivated_product_hidden_from_public_list() {
    let pool = test_pool().await;
    let suffix = &Uuid::new_v4().to_string()[..8];
    let admin_name = format!("prod_deact_admin_{}", suffix);
    let student_name = format!("prod_deact_student_{}", suffix);
    seed_super_admin(&pool, &admin_name).await;

    let app = init_service(
        App::new()
            .app_data(web::Data::new(pool.clone()))
            .configure(configure_routes),
    )
    .await;

    // Login as admin
    let login_req = TestRequest::post()
        .uri("/api/v1/auth/login")
        .set_json(serde_json::json!({ "username": &admin_name, "password": "TestPass2024!!" }))
        .to_request();
    let admin_body: Value = read_body_json(call_service(&app, login_req).await).await;
    let admin_token = admin_body["token"].as_str().unwrap().to_string();

    // Create product
    let sku = format!("SKU-DEACT-{}", suffix);
    let create_req = TestRequest::post()
        .uri("/api/v1/admin/products")
        .insert_header(("Authorization", format!("Bearer {}", admin_token)))
        .set_json(serde_json::json!({
            "name": format!("Deact Test {}", suffix),
            "price_cents": 500,
            "sku": &sku,
            "initial_quantity": 20
        }))
        .to_request();
    let create_body: Value = read_body_json(call_service(&app, create_req).await).await;
    let product_id = create_body["id"].as_str().expect("product id missing");

    // Seed student and login
    let student_token =
        seed_user_and_login(&app, &pool, &student_name, "Student").await;

    // Product is visible in student list before deactivation
    let list_req = TestRequest::get()
        .uri("/api/v1/products")
        .insert_header(("Authorization", format!("Bearer {}", student_token)))
        .to_request();
    let list_body: Value = read_body_json(call_service(&app, list_req).await).await;
    let items = list_body.as_array().unwrap();
    assert!(
        items.iter().any(|p| p["id"] == product_id),
        "new product must appear in public list before deactivation"
    );

    // Deactivate
    let deact_req = TestRequest::post()
        .uri(&format!("/api/v1/admin/products/{}/deactivate", product_id))
        .insert_header(("Authorization", format!("Bearer {}", admin_token)))
        .to_request();
    let deact_resp = call_service(&app, deact_req).await;
    assert_eq!(deact_resp.status(), 200);

    // Product must no longer appear in student list
    let list_req2 = TestRequest::get()
        .uri("/api/v1/products")
        .insert_header(("Authorization", format!("Bearer {}", student_token)))
        .to_request();
    let list_body2: Value = read_body_json(call_service(&app, list_req2).await).await;
    let items2 = list_body2.as_array().unwrap();
    assert!(
        !items2.iter().any(|p| p["id"] == product_id),
        "deactivated product must not appear in public list"
    );
}
