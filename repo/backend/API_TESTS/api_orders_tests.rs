/// Order endpoint payload tests.
///
/// Covers customer-facing order flow (create, list, detail) and all admin order
/// endpoints: list, dashboard, detail, status update, and KPI.
///
/// Requires a live PostgreSQL database:
///   TEST_DATABASE_URL=postgres://meridian:meridian@127.0.0.1:5433/meridian_seeded?sslmode=disable \
///     cargo test --test api_orders_tests -- --include-ignored
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
        .expect("TEST_DATABASE_URL must be set to run api_orders_tests");
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

/// Seed a product with inventory, returns the product UUID as a string.
async fn seed_product(
    app: &impl actix_web::dev::Service<
        actix_http::Request,
        Response = actix_web::dev::ServiceResponse,
        Error = actix_web::Error,
    >,
    admin_token: &str,
    suffix: &str,
) -> String {
    let sku = format!("SKU-ORD-{}", suffix);
    let req = TestRequest::post()
        .uri("/api/v1/admin/products")
        .insert_header(("Authorization", format!("Bearer {}", admin_token)))
        .set_json(serde_json::json!({
            "name": format!("Order Test Product {}", suffix),
            "price_cents": 999,
            "sku": sku,
            "initial_quantity": 50
        }))
        .to_request();
    let body: Value = read_body_json(call_service(app, req).await).await;
    body["id"].as_str().expect("product id missing").to_string()
}

// ---------------------------------------------------------------------------
// Tests — unauthenticated access
// ---------------------------------------------------------------------------

/// GET /api/v1/orders without a token must return 401 with an error body.
#[actix_web::test]
#[ignore = "requires TEST_DATABASE_URL"]
async fn test_list_orders_requires_auth() {
    let pool = test_pool().await;
    let app = init_service(
        App::new()
            .app_data(web::Data::new(pool))
            .configure(configure_routes),
    )
    .await;

    let req = TestRequest::get().uri("/api/v1/orders").to_request();
    let resp = call_service(&app, req).await;
    assert_eq!(resp.status(), 401);
    let body: Value = read_body_json(resp).await;
    assert!(body["error"].is_string(), "401 must include an error field");
}

/// POST /api/v1/orders without a token must return 401 with an error body.
#[actix_web::test]
#[ignore = "requires TEST_DATABASE_URL"]
async fn test_create_order_requires_auth() {
    let pool = test_pool().await;
    let app = init_service(
        App::new()
            .app_data(web::Data::new(pool))
            .configure(configure_routes),
    )
    .await;

    let req = TestRequest::post()
        .uri("/api/v1/orders")
        .set_json(serde_json::json!({ "items": [] }))
        .to_request();
    let resp = call_service(&app, req).await;
    assert_eq!(resp.status(), 401);
    let body: Value = read_body_json(resp).await;
    assert!(body["error"].is_string(), "401 must include an error field");
}

// ---------------------------------------------------------------------------
// Tests — customer order flow
// ---------------------------------------------------------------------------

/// GET /api/v1/orders returns an empty array for a newly seeded user with no orders.
#[actix_web::test]
#[ignore = "requires TEST_DATABASE_URL"]
async fn test_list_orders_returns_empty_array_for_new_user() {
    let pool = test_pool().await;
    let suffix = &Uuid::new_v4().to_string()[..8];
    seed_user(&pool, &format!("ord_list_{}", suffix), "Student").await;

    let app = init_service(
        App::new()
            .app_data(web::Data::new(pool.clone()))
            .configure(configure_routes),
    )
    .await;
    let token = login_token(&app, &format!("ord_list_{}", suffix)).await;

    let req = TestRequest::get()
        .uri("/api/v1/orders")
        .insert_header(("Authorization", format!("Bearer {}", token)))
        .to_request();
    let resp = call_service(&app, req).await;
    assert_eq!(resp.status(), 200);
    let body: Value = read_body_json(resp).await;
    let items = body.as_array().expect("orders response must be a JSON array");
    assert!(items.is_empty(), "new user must have no orders");
}

/// POST /api/v1/orders with empty items returns 422 with error body.
#[actix_web::test]
#[ignore = "requires TEST_DATABASE_URL"]
async fn test_create_order_empty_items_returns_422() {
    let pool = test_pool().await;
    let suffix = &Uuid::new_v4().to_string()[..8];
    seed_user(&pool, &format!("ord_empty_{}", suffix), "Student").await;

    let app = init_service(
        App::new()
            .app_data(web::Data::new(pool.clone()))
            .configure(configure_routes),
    )
    .await;
    let token = login_token(&app, &format!("ord_empty_{}", suffix)).await;

    let req = TestRequest::post()
        .uri("/api/v1/orders")
        .insert_header(("Authorization", format!("Bearer {}", token)))
        .set_json(serde_json::json!({ "items": [] }))
        .to_request();
    let resp = call_service(&app, req).await;
    assert_eq!(resp.status(), 422, "empty items must return 422");
    let body: Value = read_body_json(resp).await;
    assert!(body["error"].is_string(), "422 must include an error field");
}

/// Full order creation flow: POST /orders returns 201 with correct shape, then
/// GET /orders returns the new order, and GET /orders/{id} returns the detail.
#[actix_web::test]
#[ignore = "requires TEST_DATABASE_URL"]
async fn test_create_order_returns_correct_shape_and_appears_in_list() {
    let pool = test_pool().await;
    let suffix = &Uuid::new_v4().to_string()[..8];
    let admin_name = format!("ord_cr_admin_{}", suffix);
    let student_name = format!("ord_cr_student_{}", suffix);
    seed_super_admin(&pool, &admin_name).await;
    seed_user(&pool, &student_name, "Student").await;

    let app = init_service(
        App::new()
            .app_data(web::Data::new(pool.clone()))
            .configure(configure_routes),
    )
    .await;

    let admin_token = login_token(&app, &admin_name).await;
    let product_id = seed_product(&app, &admin_token, suffix).await;
    let student_token = login_token(&app, &student_name).await;

    // Create order
    let req = TestRequest::post()
        .uri("/api/v1/orders")
        .insert_header(("Authorization", format!("Bearer {}", student_token)))
        .set_json(serde_json::json!({
            "items": [{ "product_id": product_id, "quantity": 2 }],
            "notes": "test order note"
        }))
        .to_request();
    let resp = call_service(&app, req).await;
    assert_eq!(resp.status(), 201, "create order must return 201");
    let body: Value = read_body_json(resp).await;

    // Verify order shape
    assert!(body["id"].is_string(), "order.id must be a string");
    assert_eq!(body["status"], "pending", "new order status must be 'pending'");
    assert!(
        body["total_cents"].is_number(),
        "order.total_cents must be a number"
    );
    assert!(
        body["shipping_fee_cents"].is_number(),
        "order.shipping_fee_cents must be a number"
    );
    assert!(
        body["points_earned"].is_number(),
        "order.points_earned must be a number"
    );
    assert!(body["items"].is_array(), "order.items must be an array");
    let items = body["items"].as_array().unwrap();
    assert_eq!(items.len(), 1, "order must contain 1 item");
    assert!(items[0]["product_id"].is_string(), "item.product_id must be string");
    assert_eq!(items[0]["quantity"], 2, "item.quantity must match");
    assert!(items[0]["unit_price_cents"].is_number(), "item.unit_price_cents must be number");

    let order_id = body["id"].as_str().unwrap().to_string();

    // GET /orders must now include this order
    let list_req = TestRequest::get()
        .uri("/api/v1/orders")
        .insert_header(("Authorization", format!("Bearer {}", student_token)))
        .to_request();
    let list_body: Value = read_body_json(call_service(&app, list_req).await).await;
    let list_items = list_body.as_array().unwrap();
    assert!(
        list_items.iter().any(|o| o["id"] == order_id),
        "created order must appear in GET /orders"
    );

    // GET /orders/{id} returns detail
    let detail_req = TestRequest::get()
        .uri(&format!("/api/v1/orders/{}", order_id))
        .insert_header(("Authorization", format!("Bearer {}", student_token)))
        .to_request();
    let detail_resp = call_service(&app, detail_req).await;
    assert_eq!(detail_resp.status(), 200, "GET /orders/{{id}} must return 200");
    let detail_body: Value = read_body_json(detail_resp).await;
    assert_eq!(detail_body["id"], order_id, "detail must return correct order id");
    assert!(detail_body["items"].is_array(), "detail.items must be array");
}

/// GET /api/v1/orders/{id} owned by another user must return 403 with error body.
#[actix_web::test]
#[ignore = "requires TEST_DATABASE_URL"]
async fn test_get_order_by_other_user_returns_403() {
    let pool = test_pool().await;
    let suffix = &Uuid::new_v4().to_string()[..8];
    let admin_name = format!("ord_403_admin_{}", suffix);
    let owner_name = format!("ord_403_owner_{}", suffix);
    let other_name = format!("ord_403_other_{}", suffix);
    seed_super_admin(&pool, &admin_name).await;
    seed_user(&pool, &owner_name, "Student").await;
    seed_user(&pool, &other_name, "Student").await;

    let app = init_service(
        App::new()
            .app_data(web::Data::new(pool.clone()))
            .configure(configure_routes),
    )
    .await;

    let admin_token = login_token(&app, &admin_name).await;
    let product_id = seed_product(&app, &admin_token, suffix).await;
    let owner_token = login_token(&app, &owner_name).await;
    let other_token = login_token(&app, &other_name).await;

    // Owner places an order
    let req = TestRequest::post()
        .uri("/api/v1/orders")
        .insert_header(("Authorization", format!("Bearer {}", owner_token)))
        .set_json(serde_json::json!({
            "items": [{ "product_id": product_id, "quantity": 1 }]
        }))
        .to_request();
    let body: Value = read_body_json(call_service(&app, req).await).await;
    let order_id = body["id"].as_str().expect("order id missing");

    // Other user tries to access it
    let req = TestRequest::get()
        .uri(&format!("/api/v1/orders/{}", order_id))
        .insert_header(("Authorization", format!("Bearer {}", other_token)))
        .to_request();
    let resp = call_service(&app, req).await;
    assert_eq!(resp.status(), 403, "another user must not access someone else's order");
    let body: Value = read_body_json(resp).await;
    assert!(body["error"].is_string(), "403 must include an error field");
}

/// GET /api/v1/orders/{id} for a nonexistent order must return 404 with error body.
#[actix_web::test]
#[ignore = "requires TEST_DATABASE_URL"]
async fn test_get_nonexistent_order_returns_404() {
    let pool = test_pool().await;
    let suffix = &Uuid::new_v4().to_string()[..8];
    seed_user(&pool, &format!("ord_404_{}", suffix), "Student").await;

    let app = init_service(
        App::new()
            .app_data(web::Data::new(pool.clone()))
            .configure(configure_routes),
    )
    .await;
    let token = login_token(&app, &format!("ord_404_{}", suffix)).await;

    let ghost_id = Uuid::new_v4();
    let req = TestRequest::get()
        .uri(&format!("/api/v1/orders/{}", ghost_id))
        .insert_header(("Authorization", format!("Bearer {}", token)))
        .to_request();
    let resp = call_service(&app, req).await;
    assert_eq!(resp.status(), 404);
    let body: Value = read_body_json(resp).await;
    assert!(body["error"].is_string(), "404 must include an error field");
}

// ---------------------------------------------------------------------------
// Tests — admin order management
// ---------------------------------------------------------------------------

/// GET /api/v1/admin/orders without auth must return 401.
#[actix_web::test]
#[ignore = "requires TEST_DATABASE_URL"]
async fn test_admin_list_orders_requires_auth() {
    let pool = test_pool().await;
    let app = init_service(
        App::new()
            .app_data(web::Data::new(pool))
            .configure(configure_routes),
    )
    .await;

    let req = TestRequest::get().uri("/api/v1/admin/orders").to_request();
    let resp = call_service(&app, req).await;
    assert_eq!(resp.status(), 401);
    let body: Value = read_body_json(resp).await;
    assert!(body["error"].is_string(), "401 must include an error field");
}

/// GET /api/v1/admin/orders by non-admin must return 403.
#[actix_web::test]
#[ignore = "requires TEST_DATABASE_URL"]
async fn test_admin_list_orders_requires_admin_role() {
    let pool = test_pool().await;
    let suffix = &Uuid::new_v4().to_string()[..8];
    seed_user(&pool, &format!("ord_nonadmin_{}", suffix), "Student").await;

    let app = init_service(
        App::new()
            .app_data(web::Data::new(pool.clone()))
            .configure(configure_routes),
    )
    .await;
    let token = login_token(&app, &format!("ord_nonadmin_{}", suffix)).await;

    let req = TestRequest::get()
        .uri("/api/v1/admin/orders")
        .insert_header(("Authorization", format!("Bearer {}", token)))
        .to_request();
    let resp = call_service(&app, req).await;
    assert_eq!(resp.status(), 403, "non-admin must receive 403");
    let body: Value = read_body_json(resp).await;
    assert!(body["error"].is_string(), "403 must include an error field");
}

/// GET /api/v1/admin/orders returns array where each item has the required fields.
#[actix_web::test]
#[ignore = "requires TEST_DATABASE_URL"]
async fn test_admin_list_orders_returns_array_with_required_fields() {
    let pool = test_pool().await;
    let suffix = &Uuid::new_v4().to_string()[..8];
    let admin_name = format!("ord_admlist_admin_{}", suffix);
    let student_name = format!("ord_admlist_student_{}", suffix);
    seed_super_admin(&pool, &admin_name).await;
    seed_user(&pool, &student_name, "Student").await;

    let app = init_service(
        App::new()
            .app_data(web::Data::new(pool.clone()))
            .configure(configure_routes),
    )
    .await;

    let admin_token = login_token(&app, &admin_name).await;
    let product_id = seed_product(&app, &admin_token, suffix).await;
    let student_token = login_token(&app, &student_name).await;

    // Place an order so the list is non-empty
    let _ = call_service(
        &app,
        TestRequest::post()
            .uri("/api/v1/orders")
            .insert_header(("Authorization", format!("Bearer {}", student_token)))
            .set_json(serde_json::json!({
                "items": [{ "product_id": product_id, "quantity": 1 }]
            }))
            .to_request(),
    )
    .await;

    let req = TestRequest::get()
        .uri("/api/v1/admin/orders")
        .insert_header(("Authorization", format!("Bearer {}", admin_token)))
        .to_request();
    let resp = call_service(&app, req).await;
    assert_eq!(resp.status(), 200);
    let body: Value = read_body_json(resp).await;
    let orders = body.as_array().expect("admin orders must be a JSON array");
    assert!(!orders.is_empty(), "must have at least one order after seeding");

    for order in orders {
        assert!(order["id"].is_string(), "order.id must be string");
        assert!(order["user_id"].is_string(), "order.user_id must be string");
        assert!(order["username"].is_string(), "order.username must be string");
        assert!(order["status"].is_string(), "order.status must be string");
        assert!(order["total_cents"].is_number(), "order.total_cents must be number");
        assert!(order["item_count"].is_number(), "order.item_count must be number");
    }
}

/// GET /api/v1/admin/orders/dashboard returns a shape with order counts and
/// recent_orders array.
#[actix_web::test]
#[ignore = "requires TEST_DATABASE_URL"]
async fn test_admin_orders_dashboard_returns_correct_shape() {
    let pool = test_pool().await;
    let suffix = &Uuid::new_v4().to_string()[..8];
    let admin_name = format!("ord_dash_admin_{}", suffix);
    seed_super_admin(&pool, &admin_name).await;

    let app = init_service(
        App::new()
            .app_data(web::Data::new(pool.clone()))
            .configure(configure_routes),
    )
    .await;
    let token = login_token(&app, &admin_name).await;

    let req = TestRequest::get()
        .uri("/api/v1/admin/orders/dashboard")
        .insert_header(("Authorization", format!("Bearer {}", token)))
        .to_request();
    let resp = call_service(&app, req).await;
    assert_eq!(resp.status(), 200);
    let body: Value = read_body_json(resp).await;

    assert!(body["pending_orders"].is_number(), "dashboard.pending_orders must be number");
    assert!(body["confirmed_orders"].is_number(), "dashboard.confirmed_orders must be number");
    assert!(body["fulfilled_orders"].is_number(), "dashboard.fulfilled_orders must be number");
    assert!(body["cancelled_orders"].is_number(), "dashboard.cancelled_orders must be number");
    assert!(body["pending_over_30_min"].is_number(), "dashboard.pending_over_30_min must be number");
    assert!(body["low_stock_products"].is_array(), "dashboard.low_stock_products must be array");
    assert!(body["recent_orders"].is_array(), "dashboard.recent_orders must be array");
}

/// POST /api/v1/admin/orders/{id}/status updates status and returns old/new.
#[actix_web::test]
#[ignore = "requires TEST_DATABASE_URL"]
async fn test_admin_update_order_status_returns_old_and_new_status() {
    let pool = test_pool().await;
    let suffix = &Uuid::new_v4().to_string()[..8];
    let admin_name = format!("ord_upst_admin_{}", suffix);
    let student_name = format!("ord_upst_student_{}", suffix);
    seed_super_admin(&pool, &admin_name).await;
    seed_user(&pool, &student_name, "Student").await;

    let app = init_service(
        App::new()
            .app_data(web::Data::new(pool.clone()))
            .configure(configure_routes),
    )
    .await;

    let admin_token = login_token(&app, &admin_name).await;
    let product_id = seed_product(&app, &admin_token, suffix).await;
    let student_token = login_token(&app, &student_name).await;

    // Place an order
    let create_req = TestRequest::post()
        .uri("/api/v1/orders")
        .insert_header(("Authorization", format!("Bearer {}", student_token)))
        .set_json(serde_json::json!({
            "items": [{ "product_id": product_id, "quantity": 1 }]
        }))
        .to_request();
    let create_body: Value = read_body_json(call_service(&app, create_req).await).await;
    let order_id = create_body["id"].as_str().expect("order id missing");

    // Admin updates status to 'confirmed'
    let req = TestRequest::post()
        .uri(&format!("/api/v1/admin/orders/{}/status", order_id))
        .insert_header(("Authorization", format!("Bearer {}", admin_token)))
        .set_json(serde_json::json!({ "status": "confirmed" }))
        .to_request();
    let resp = call_service(&app, req).await;
    assert_eq!(resp.status(), 200);
    let body: Value = read_body_json(resp).await;

    assert_eq!(body["old_status"], "pending", "old_status must be 'pending'");
    assert_eq!(body["new_status"], "confirmed", "new_status must be 'confirmed'");
    assert!(body["order_id"].is_string(), "response must include order_id");
    assert!(body["message"].is_string(), "response must include message");
}

/// POST /api/v1/admin/orders/{id}/status with invalid status must return 422.
#[actix_web::test]
#[ignore = "requires TEST_DATABASE_URL"]
async fn test_admin_update_order_invalid_status_returns_422() {
    let pool = test_pool().await;
    let suffix = &Uuid::new_v4().to_string()[..8];
    let admin_name = format!("ord_inv_admin_{}", suffix);
    let student_name = format!("ord_inv_student_{}", suffix);
    seed_super_admin(&pool, &admin_name).await;
    seed_user(&pool, &student_name, "Student").await;

    let app = init_service(
        App::new()
            .app_data(web::Data::new(pool.clone()))
            .configure(configure_routes),
    )
    .await;

    let admin_token = login_token(&app, &admin_name).await;
    let product_id = seed_product(&app, &admin_token, suffix).await;
    let student_token = login_token(&app, &student_name).await;

    let create_req = TestRequest::post()
        .uri("/api/v1/orders")
        .insert_header(("Authorization", format!("Bearer {}", student_token)))
        .set_json(serde_json::json!({
            "items": [{ "product_id": product_id, "quantity": 1 }]
        }))
        .to_request();
    let create_body: Value = read_body_json(call_service(&app, create_req).await).await;
    let order_id = create_body["id"].as_str().expect("order id missing");

    let req = TestRequest::post()
        .uri(&format!("/api/v1/admin/orders/{}/status", order_id))
        .insert_header(("Authorization", format!("Bearer {}", admin_token)))
        .set_json(serde_json::json!({ "status": "shipped" })) // invalid
        .to_request();
    let resp = call_service(&app, req).await;
    assert_eq!(resp.status(), 422, "invalid status must return 422");
    let body: Value = read_body_json(resp).await;
    assert!(body["error"].is_string(), "422 must include an error field");
}

/// GET /api/v1/admin/kpi returns all required KPI numeric fields.
#[actix_web::test]
#[ignore = "requires TEST_DATABASE_URL"]
async fn test_admin_kpi_returns_required_numeric_fields() {
    let pool = test_pool().await;
    let suffix = &Uuid::new_v4().to_string()[..8];
    let admin_name = format!("kpi_admin_{}", suffix);
    seed_super_admin(&pool, &admin_name).await;

    let app = init_service(
        App::new()
            .app_data(web::Data::new(pool.clone()))
            .configure(configure_routes),
    )
    .await;
    let token = login_token(&app, &admin_name).await;

    let req = TestRequest::get()
        .uri("/api/v1/admin/kpi")
        .insert_header(("Authorization", format!("Bearer {}", token)))
        .to_request();
    let resp = call_service(&app, req).await;
    assert_eq!(resp.status(), 200);
    let body: Value = read_body_json(resp).await;

    assert!(body["daily_sales_cents"].is_number(), "kpi.daily_sales_cents must be number");
    assert!(
        body["average_order_value_cents"].is_number(),
        "kpi.average_order_value_cents must be number"
    );
    assert!(
        body["repeat_purchase_rate_pct"].is_number(),
        "kpi.repeat_purchase_rate_pct must be number"
    );
    assert!(body["orders_last_30d"].is_number(), "kpi.orders_last_30d must be number");
    assert!(body["buyers_last_30d"].is_number(), "kpi.buyers_last_30d must be number");
    assert!(body["repeat_buyers_last_30d"].is_number(), "kpi.repeat_buyers_last_30d must be number");
}

/// GET /api/v1/admin/kpi without auth must return 401.
#[actix_web::test]
#[ignore = "requires TEST_DATABASE_URL"]
async fn test_admin_kpi_requires_auth() {
    let pool = test_pool().await;
    let app = init_service(
        App::new()
            .app_data(web::Data::new(pool))
            .configure(configure_routes),
    )
    .await;

    let req = TestRequest::get().uri("/api/v1/admin/kpi").to_request();
    let resp = call_service(&app, req).await;
    assert_eq!(resp.status(), 401);
    let body: Value = read_body_json(resp).await;
    assert!(body["error"].is_string(), "401 must include an error field");
}
