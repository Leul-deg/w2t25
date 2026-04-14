/// Notification endpoint payload tests.
///
/// Verifies that the notifications inbox, unread-count, and mark-read endpoints
/// return correct JSON shapes, enforce ownership, and keep counts accurate.
///
/// Requires a live PostgreSQL database:
///   TEST_DATABASE_URL=postgres://meridian:meridian@127.0.0.1:5433/meridian_seeded?sslmode=disable \
///     cargo test --test api_notifications_payload_tests -- --include-ignored
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
        .expect("TEST_DATABASE_URL must be set to run api_notifications_payload_tests");
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

async fn insert_notification(pool: &PgPool, recipient_id: Uuid, subject: &str) -> Uuid {
    let id = Uuid::new_v4();
    sqlx::query(
        "INSERT INTO notifications (id, recipient_id, subject, body, notification_type, created_at)
         VALUES ($1, $2, $3, $4, 'general', NOW())",
    )
    .bind(id)
    .bind(recipient_id)
    .bind(subject)
    .bind("Test body text.")
    .execute(pool)
    .await
    .expect("insert notification failed");
    id
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

/// GET /api/v1/notifications without auth must return 401.
#[actix_web::test]
#[ignore = "requires TEST_DATABASE_URL"]
async fn test_list_notifications_requires_auth() {
    let pool = test_pool().await;
    let app = init_service(
        App::new()
            .app_data(web::Data::new(pool))
            .configure(configure_routes),
    )
    .await;

    let req = TestRequest::get().uri("/api/v1/notifications").to_request();
    let resp = call_service(&app, req).await;
    assert_eq!(resp.status(), 401);
    let body: Value = read_body_json(resp).await;
    assert!(body["error"].is_string());
}

/// GET /api/v1/notifications must return a JSON array where each item has
/// the required fields.
#[actix_web::test]
#[ignore = "requires TEST_DATABASE_URL"]
async fn test_list_notifications_returns_array_with_required_fields() {
    let pool = test_pool().await;
    let suffix = &Uuid::new_v4().to_string()[..8];
    let username = format!("notif_shape_{}", suffix);
    let uid = seed_user(&pool, &username, "Student").await;
    insert_notification(&pool, uid, "Shape Test Subject").await;

    let app = init_service(
        App::new()
            .app_data(web::Data::new(pool.clone()))
            .configure(configure_routes),
    )
    .await;
    let token = login_token(&app, &username).await;

    let req = TestRequest::get()
        .uri("/api/v1/notifications")
        .insert_header(("Authorization", format!("Bearer {}", token)))
        .to_request();
    let resp = call_service(&app, req).await;
    assert_eq!(resp.status(), 200);
    let body: Value = read_body_json(resp).await;
    let items = body.as_array().expect("notifications must be an array");
    assert!(!items.is_empty(), "should have at least one notification");

    let first = &items[0];
    assert!(first["id"].is_string(), "notification.id must be a string");
    assert!(first["subject"].is_string(), "notification.subject must be a string");
    assert!(first["body"].is_string(), "notification.body must be a string");
    assert!(
        first["notification_type"].is_string(),
        "notification.notification_type must be a string"
    );
    assert!(
        first["created_at"].is_string(),
        "notification.created_at must be a string"
    );
    // read_at is null until explicitly marked
    assert!(
        first["read_at"].is_null() || first["read_at"].is_string(),
        "notification.read_at must be null or a string"
    );
}

/// GET /api/v1/notifications/unread-count must return { "unread": <integer> }.
#[actix_web::test]
#[ignore = "requires TEST_DATABASE_URL"]
async fn test_unread_count_returns_integer_field() {
    let pool = test_pool().await;
    let suffix = &Uuid::new_v4().to_string()[..8];
    let username = format!("notif_cnt_{}", suffix);
    let uid = seed_user(&pool, &username, "Student").await;
    // Insert 2 unread notifications
    insert_notification(&pool, uid, "Unread One").await;
    insert_notification(&pool, uid, "Unread Two").await;

    let app = init_service(
        App::new()
            .app_data(web::Data::new(pool.clone()))
            .configure(configure_routes),
    )
    .await;
    let token = login_token(&app, &username).await;

    let req = TestRequest::get()
        .uri("/api/v1/notifications/unread-count")
        .insert_header(("Authorization", format!("Bearer {}", token)))
        .to_request();
    let resp = call_service(&app, req).await;
    assert_eq!(resp.status(), 200);
    let body: Value = read_body_json(resp).await;
    assert!(
        body["unread"].is_number(),
        "unread-count must return {{ \"unread\": <integer> }}"
    );
    let count = body["unread"].as_i64().expect("unread must be an integer");
    assert!(count >= 2, "unread count must reflect seeded notifications, got {}", count);
}

/// POST /api/v1/notifications/{id}/read must return { "message": "Marked as read." }
/// and must decrement the unread count.
#[actix_web::test]
#[ignore = "requires TEST_DATABASE_URL"]
async fn test_mark_read_returns_message_and_decrements_count() {
    let pool = test_pool().await;
    let suffix = &Uuid::new_v4().to_string()[..8];
    let username = format!("notif_read_{}", suffix);
    let uid = seed_user(&pool, &username, "Student").await;
    let notif_id = insert_notification(&pool, uid, "Mark Me Read").await;

    let app = init_service(
        App::new()
            .app_data(web::Data::new(pool.clone()))
            .configure(configure_routes),
    )
    .await;
    let token = login_token(&app, &username).await;

    // Capture initial unread count
    let cnt_req = TestRequest::get()
        .uri("/api/v1/notifications/unread-count")
        .insert_header(("Authorization", format!("Bearer {}", token)))
        .to_request();
    let cnt_before: Value = read_body_json(call_service(&app, cnt_req).await).await;
    let before = cnt_before["unread"].as_i64().unwrap();

    // Mark as read
    let read_req = TestRequest::post()
        .uri(&format!("/api/v1/notifications/{}/read", notif_id))
        .insert_header(("Authorization", format!("Bearer {}", token)))
        .to_request();
    let read_resp = call_service(&app, read_req).await;
    assert_eq!(read_resp.status(), 200);
    let read_body: Value = read_body_json(read_resp).await;
    assert_eq!(
        read_body["message"], "Marked as read.",
        "mark-read must return the expected message"
    );

    // Unread count must have decreased by 1
    let cnt_req2 = TestRequest::get()
        .uri("/api/v1/notifications/unread-count")
        .insert_header(("Authorization", format!("Bearer {}", token)))
        .to_request();
    let cnt_after: Value = read_body_json(call_service(&app, cnt_req2).await).await;
    let after = cnt_after["unread"].as_i64().unwrap();
    assert_eq!(after, before - 1, "unread count must decrease by 1 after mark-read");
}

/// POST /api/v1/notifications/{id}/read for a notification that does not exist
/// must return 404 with an error field.
#[actix_web::test]
#[ignore = "requires TEST_DATABASE_URL"]
async fn test_mark_read_nonexistent_notification_returns_404() {
    let pool = test_pool().await;
    let suffix = &Uuid::new_v4().to_string()[..8];
    let username = format!("notif_404_{}", suffix);
    seed_user(&pool, &username, "Student").await;

    let app = init_service(
        App::new()
            .app_data(web::Data::new(pool.clone()))
            .configure(configure_routes),
    )
    .await;
    let token = login_token(&app, &username).await;

    let ghost_id = Uuid::new_v4();
    let req = TestRequest::post()
        .uri(&format!("/api/v1/notifications/{}/read", ghost_id))
        .insert_header(("Authorization", format!("Bearer {}", token)))
        .to_request();
    let resp = call_service(&app, req).await;
    assert_eq!(resp.status(), 404);
    let body: Value = read_body_json(resp).await;
    assert!(body["error"].is_string(), "404 must include an error field");
}

/// POST /api/v1/notifications/{id}/read for a notification owned by another
/// user must return 403 with an error field.
#[actix_web::test]
#[ignore = "requires TEST_DATABASE_URL"]
async fn test_mark_read_foreign_notification_returns_403_with_error() {
    let pool = test_pool().await;
    let suffix = &Uuid::new_v4().to_string()[..8];
    let owner_name = format!("notif_owner_{}", suffix);
    let attacker_name = format!("notif_attacker_{}", suffix);

    let owner_id = seed_user(&pool, &owner_name, "Student").await;
    seed_user(&pool, &attacker_name, "Student").await;

    let notif_id = insert_notification(&pool, owner_id, "Owners Only").await;

    let app = init_service(
        App::new()
            .app_data(web::Data::new(pool.clone()))
            .configure(configure_routes),
    )
    .await;
    let attacker_token = login_token(&app, &attacker_name).await;

    let req = TestRequest::post()
        .uri(&format!("/api/v1/notifications/{}/read", notif_id))
        .insert_header(("Authorization", format!("Bearer {}", attacker_token)))
        .to_request();
    let resp = call_service(&app, req).await;
    assert_eq!(resp.status(), 403);
    let body: Value = read_body_json(resp).await;
    assert!(
        body["error"].is_string(),
        "403 response must include an error field, got: {}",
        body
    );
}

/// Marking an already-read notification as read again must be idempotent
/// (200, same message, count unchanged).
#[actix_web::test]
#[ignore = "requires TEST_DATABASE_URL"]
async fn test_mark_read_is_idempotent() {
    let pool = test_pool().await;
    let suffix = &Uuid::new_v4().to_string()[..8];
    let username = format!("notif_idem_{}", suffix);
    let uid = seed_user(&pool, &username, "Student").await;
    let notif_id = insert_notification(&pool, uid, "Idempotent Read").await;

    let app = init_service(
        App::new()
            .app_data(web::Data::new(pool.clone()))
            .configure(configure_routes),
    )
    .await;
    let token = login_token(&app, &username).await;

    // Mark read once
    let req1 = TestRequest::post()
        .uri(&format!("/api/v1/notifications/{}/read", notif_id))
        .insert_header(("Authorization", format!("Bearer {}", token)))
        .to_request();
    let resp1 = call_service(&app, req1).await;
    assert_eq!(resp1.status(), 200);

    // Mark read again — still 200
    let req2 = TestRequest::post()
        .uri(&format!("/api/v1/notifications/{}/read", notif_id))
        .insert_header(("Authorization", format!("Bearer {}", token)))
        .to_request();
    let resp2 = call_service(&app, req2).await;
    assert_eq!(resp2.status(), 200, "second mark-read must also be 200");
    let body2: Value = read_body_json(resp2).await;
    assert_eq!(body2["message"], "Marked as read.");
}
