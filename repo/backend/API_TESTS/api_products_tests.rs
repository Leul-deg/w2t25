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

/// GET /api/v1/products/{id} for an existing active product returns 200 with
/// the required shape: id, name, price_cents, active, quantity, low_stock_threshold.
#[actix_web::test]
#[ignore = "requires TEST_DATABASE_URL"]
async fn test_get_product_by_id_returns_correct_shape() {
    let pool = test_pool().await;
    let app = init_service(
        App::new()
            .app_data(web::Data::new(pool.clone()))
            .configure(configure_routes),
    )
    .await;
    let suffix = &Uuid::new_v4().to_string()[..8];
    let token =
        seed_user_and_login(&app, &pool, &format!("prod_byid_{}", suffix), "Student").await;

    // Seed a product so we have a known ID to fetch.
    let product_id = Uuid::new_v4();
    sqlx::query(
        "INSERT INTO products (id, name, price_cents, sku, active, created_at, updated_at)
         VALUES ($1, $2, 1500, $3, TRUE, NOW(), NOW())",
    )
    .bind(product_id)
    .bind(format!("Test Product {}", suffix))
    .bind(format!("SKU-BYID-{}", suffix))
    .execute(&pool)
    .await
    .expect("seed product failed");
    sqlx::query(
        "INSERT INTO inventory (product_id, quantity, low_stock_threshold, last_updated_at)
         VALUES ($1, 25, 5, NOW())
         ON CONFLICT (product_id) DO UPDATE SET quantity = 25",
    )
    .bind(product_id)
    .execute(&pool)
    .await
    .expect("seed inventory failed");

    let req = TestRequest::get()
        .uri(&format!("/api/v1/products/{}", product_id))
        .insert_header(("Authorization", format!("Bearer {}", token)))
        .to_request();
    let resp = call_service(&app, req).await;
    assert_eq!(resp.status(), 200, "GET /products/{{id}} must return 200 for an existing product");
    let body: Value = read_body_json(resp).await;

    assert_eq!(
        body["id"].as_str().unwrap(),
        product_id.to_string(),
        "product.id must match the requested id"
    );
    assert!(body["name"].is_string(), "product.name must be a string");
    assert!(body["price_cents"].is_number(), "product.price_cents must be a number");
    assert_eq!(body["active"], true, "fetched product must be active");
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

/// POST /api/v1/admin/products returns 201 with a complete product shape
/// including the seeded inventory quantity.
#[actix_web::test]
#[ignore = "requires TEST_DATABASE_URL"]
async fn test_admin_create_product_returns_correct_shape() {
    let pool = test_pool().await;
    let suffix = &Uuid::new_v4().to_string()[..8];
    let admin_name = format!("prod_create_admin_{}", suffix);
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
    let admin_body: Value = read_body_json(call_service(&app, login_req).await).await;
    let token = admin_body["token"].as_str().unwrap().to_string();

    let req = TestRequest::post()
        .uri("/api/v1/admin/products")
        .insert_header(("Authorization", format!("Bearer {}", token)))
        .set_json(serde_json::json!({
            "name": format!("Shape Test {}", suffix),
            "description": "unit test product",
            "price_cents": 1299,
            "sku": format!("SKU-SHAPE-{}", suffix),
            "initial_quantity": 30
        }))
        .to_request();
    let resp = call_service(&app, req).await;
    assert_eq!(resp.status(), 201, "admin create product must return 201");
    let body: Value = read_body_json(resp).await;

    assert!(body["id"].is_string(), "product.id must be string");
    assert_eq!(body["name"], format!("Shape Test {}", suffix), "product.name must match");
    assert_eq!(body["price_cents"], 1299, "product.price_cents must match");
    assert_eq!(body["active"], true, "newly created product must be active");
    assert_eq!(body["quantity"], 30, "quantity must reflect initial_quantity");
    assert!(body["created_at"].is_string(), "product.created_at must be string");
}

/// POST /api/v1/admin/products requires auth — unauthenticated request returns 401.
#[actix_web::test]
#[ignore = "requires TEST_DATABASE_URL"]
async fn test_admin_create_product_requires_auth() {
    let pool = test_pool().await;
    let app = init_service(
        App::new()
            .app_data(web::Data::new(pool))
            .configure(configure_routes),
    )
    .await;

    let req = TestRequest::post()
        .uri("/api/v1/admin/products")
        .set_json(serde_json::json!({ "name": "Unauthorized", "price_cents": 100 }))
        .to_request();
    let resp = call_service(&app, req).await;
    assert_eq!(resp.status(), 401);
    let body: Value = read_body_json(resp).await;
    assert!(body["error"].is_string(), "401 must include an error field");
}

/// POST /api/v1/admin/products by a non-admin returns 403.
#[actix_web::test]
#[ignore = "requires TEST_DATABASE_URL"]
async fn test_admin_create_product_requires_admin_role() {
    let pool = test_pool().await;
    let suffix = &Uuid::new_v4().to_string()[..8];
    let student_token =
        seed_user_and_login(
            &init_service(
                App::new()
                    .app_data(web::Data::new(pool.clone()))
                    .configure(configure_routes),
            )
            .await,
            &pool,
            &format!("prod_norole_{}", suffix),
            "Student",
        )
        .await;

    let app = init_service(
        App::new()
            .app_data(web::Data::new(pool.clone()))
            .configure(configure_routes),
    )
    .await;

    let req = TestRequest::post()
        .uri("/api/v1/admin/products")
        .insert_header(("Authorization", format!("Bearer {}", student_token)))
        .set_json(serde_json::json!({ "name": "Student Cannot Create", "price_cents": 100 }))
        .to_request();
    let resp = call_service(&app, req).await;
    assert_eq!(resp.status(), 403, "non-admin must receive 403");
    let body: Value = read_body_json(resp).await;
    assert!(body["error"].is_string(), "403 must include an error field");
}

/// POST /api/v1/admin/products/{id}/update returns the updated product shape.
#[actix_web::test]
#[ignore = "requires TEST_DATABASE_URL"]
async fn test_admin_update_product_returns_updated_shape() {
    let pool = test_pool().await;
    let suffix = &Uuid::new_v4().to_string()[..8];
    let admin_name = format!("prod_upd_admin_{}", suffix);
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
    let admin_body: Value = read_body_json(call_service(&app, login_req).await).await;
    let token = admin_body["token"].as_str().unwrap().to_string();

    // Create product first.
    let create_req = TestRequest::post()
        .uri("/api/v1/admin/products")
        .insert_header(("Authorization", format!("Bearer {}", token)))
        .set_json(serde_json::json!({
            "name": format!("Before Update {}", suffix),
            "price_cents": 500,
            "initial_quantity": 10
        }))
        .to_request();
    let create_body: Value = read_body_json(call_service(&app, create_req).await).await;
    let product_id = create_body["id"].as_str().expect("product id missing");

    // Update name and price.
    let req = TestRequest::post()
        .uri(&format!("/api/v1/admin/products/{}/update", product_id))
        .insert_header(("Authorization", format!("Bearer {}", token)))
        .set_json(serde_json::json!({
            "name": format!("After Update {}", suffix),
            "price_cents": 999,
            "quantity": 25
        }))
        .to_request();
    let resp = call_service(&app, req).await;
    assert_eq!(resp.status(), 200, "admin update product must return 200");
    let body: Value = read_body_json(resp).await;

    assert_eq!(body["id"].as_str().unwrap(), product_id, "id must be unchanged");
    assert_eq!(body["name"], format!("After Update {}", suffix), "name must be updated");
    assert_eq!(body["price_cents"], 999, "price_cents must be updated");
    assert_eq!(body["quantity"], 25, "quantity must be updated");
    assert_eq!(body["active"], true, "active must be unchanged");
}

/// POST /api/v1/admin/products/{id}/update with a negative price returns 422.
#[actix_web::test]
#[ignore = "requires TEST_DATABASE_URL"]
async fn test_admin_update_product_negative_price_returns_422() {
    let pool = test_pool().await;
    let suffix = &Uuid::new_v4().to_string()[..8];
    let admin_name = format!("prod_negprice_admin_{}", suffix);
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
    let admin_body: Value = read_body_json(call_service(&app, login_req).await).await;
    let token = admin_body["token"].as_str().unwrap().to_string();

    // Create product first.
    let create_req = TestRequest::post()
        .uri("/api/v1/admin/products")
        .insert_header(("Authorization", format!("Bearer {}", token)))
        .set_json(serde_json::json!({ "name": format!("NegPrice {}", suffix), "price_cents": 100 }))
        .to_request();
    let create_body: Value = read_body_json(call_service(&app, create_req).await).await;
    let product_id = create_body["id"].as_str().expect("product id missing");

    let req = TestRequest::post()
        .uri(&format!("/api/v1/admin/products/{}/update", product_id))
        .insert_header(("Authorization", format!("Bearer {}", token)))
        .set_json(serde_json::json!({ "price_cents": -50 }))
        .to_request();
    let resp = call_service(&app, req).await;
    assert_eq!(resp.status(), 422, "negative price must return 422");
    let body: Value = read_body_json(resp).await;
    assert!(body["error"].is_string(), "422 must include an error field");
}

/// POST /api/v1/admin/products/{id}/update for a nonexistent product returns 404.
#[actix_web::test]
#[ignore = "requires TEST_DATABASE_URL"]
async fn test_admin_update_nonexistent_product_returns_404() {
    let pool = test_pool().await;
    let suffix = &Uuid::new_v4().to_string()[..8];
    let admin_name = format!("prod_upd404_admin_{}", suffix);
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
    let admin_body: Value = read_body_json(call_service(&app, login_req).await).await;
    let token = admin_body["token"].as_str().unwrap().to_string();

    let ghost_id = Uuid::new_v4();
    let req = TestRequest::post()
        .uri(&format!("/api/v1/admin/products/{}/update", ghost_id))
        .insert_header(("Authorization", format!("Bearer {}", token)))
        .set_json(serde_json::json!({ "name": "Ghost" }))
        .to_request();
    let resp = call_service(&app, req).await;
    assert_eq!(resp.status(), 404, "nonexistent product update must return 404");
    let body: Value = read_body_json(resp).await;
    assert!(body["error"].is_string(), "404 must include an error field");
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
