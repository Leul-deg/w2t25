/// User endpoint payload tests.
///
/// Covers:
///   GET  /users/me               – authenticated user's public profile
///   POST /users/me/request-deletion – submit account-deletion request
///   GET  /users/me/linked-students  – parent's linked student list
///
/// Requires a live PostgreSQL database:
///   TEST_DATABASE_URL=postgres://meridian:meridian@127.0.0.1:5433/meridian_seeded?sslmode=disable \
///     cargo test --test api_users_tests -- --include-ignored
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
        .expect("TEST_DATABASE_URL must be set to run api_users_tests");
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

// ---------------------------------------------------------------------------
// GET /users/me
// ---------------------------------------------------------------------------

/// GET /api/v1/users/me without a token must return 401.
#[actix_web::test]
#[ignore = "requires TEST_DATABASE_URL"]
async fn test_get_users_me_requires_auth() {
    let pool = test_pool().await;
    let app = init_service(
        App::new()
            .app_data(web::Data::new(pool))
            .configure(configure_routes),
    )
    .await;

    let req = TestRequest::get().uri("/api/v1/users/me").to_request();
    let resp = call_service(&app, req).await;
    assert_eq!(resp.status(), 401);
    let body: Value = read_body_json(resp).await;
    assert!(body["error"].is_string(), "401 must include an error field");
}

/// GET /api/v1/users/me returns the UserPublic shape:
/// id, username, email, account_state, roles, created_at.
#[actix_web::test]
#[ignore = "requires TEST_DATABASE_URL"]
async fn test_get_users_me_returns_user_public_shape() {
    let pool = test_pool().await;
    let suffix = &Uuid::new_v4().to_string()[..8];
    let username = format!("usr_me_{}", suffix);
    let user_id = seed_user(&pool, &username, "Student").await;

    let app = init_service(
        App::new()
            .app_data(web::Data::new(pool.clone()))
            .configure(configure_routes),
    )
    .await;
    let token = login_token(&app, &username).await;

    let req = TestRequest::get()
        .uri("/api/v1/users/me")
        .insert_header(("Authorization", format!("Bearer {}", token)))
        .to_request();
    let resp = call_service(&app, req).await;
    assert_eq!(resp.status(), 200, "GET /users/me must return 200");
    let body: Value = read_body_json(resp).await;

    assert_eq!(
        body["id"].as_str().unwrap(),
        user_id.to_string(),
        "user.id must match the seeded user"
    );
    assert_eq!(body["username"], username, "user.username must match");
    assert!(body["email"].is_string(), "user.email must be string");
    assert_eq!(body["account_state"], "active", "user.account_state must be 'active'");
    let roles = body["roles"].as_array().expect("user.roles must be array");
    assert!(
        roles.iter().any(|r| r == "Student"),
        "roles must contain 'Student'"
    );
    assert!(body["created_at"].is_string(), "user.created_at must be string");
}

// ---------------------------------------------------------------------------
// POST /users/me/request-deletion
// ---------------------------------------------------------------------------

/// POST /api/v1/users/me/request-deletion without auth must return 401.
#[actix_web::test]
#[ignore = "requires TEST_DATABASE_URL"]
async fn test_request_deletion_requires_auth() {
    let pool = test_pool().await;
    let app = init_service(
        App::new()
            .app_data(web::Data::new(pool))
            .configure(configure_routes),
    )
    .await;

    let req = TestRequest::post()
        .uri("/api/v1/users/me/request-deletion")
        .set_json(serde_json::json!({ "reason": "test" }))
        .to_request();
    let resp = call_service(&app, req).await;
    assert_eq!(resp.status(), 401);
    let body: Value = read_body_json(resp).await;
    assert!(body["error"].is_string(), "401 must include an error field");
}

/// POST /api/v1/users/me/request-deletion happy path returns 201 with a message.
#[actix_web::test]
#[ignore = "requires TEST_DATABASE_URL"]
async fn test_request_deletion_happy_path_returns_201_with_message() {
    let pool = test_pool().await;
    let suffix = &Uuid::new_v4().to_string()[..8];
    let username = format!("del_req_happy_{}", suffix);
    seed_user(&pool, &username, "Student").await;

    let app = init_service(
        App::new()
            .app_data(web::Data::new(pool.clone()))
            .configure(configure_routes),
    )
    .await;
    let token = login_token(&app, &username).await;

    let req = TestRequest::post()
        .uri("/api/v1/users/me/request-deletion")
        .insert_header(("Authorization", format!("Bearer {}", token)))
        .set_json(serde_json::json!({ "reason": "I want to leave the platform." }))
        .to_request();
    let resp = call_service(&app, req).await;
    assert_eq!(resp.status(), 201, "first deletion request must return 201");
    let body: Value = read_body_json(resp).await;
    assert!(
        body["message"].is_string(),
        "201 response must include a message field"
    );
}

/// POST /api/v1/users/me/request-deletion when a pending request already exists
/// returns 409.
#[actix_web::test]
#[ignore = "requires TEST_DATABASE_URL"]
async fn test_request_deletion_duplicate_returns_409() {
    let pool = test_pool().await;
    let suffix = &Uuid::new_v4().to_string()[..8];
    let username = format!("del_req_dup_{}", suffix);
    let user_id = seed_user(&pool, &username, "Student").await;

    // Seed a pre-existing pending deletion request directly in DB.
    sqlx::query(
        "INSERT INTO account_deletion_requests (id, user_id, reason, status, requested_at)
         VALUES (gen_random_uuid(), $1, 'existing request', 'pending', NOW())",
    )
    .bind(user_id)
    .execute(&pool)
    .await
    .expect("seed pending deletion request failed");

    let app = init_service(
        App::new()
            .app_data(web::Data::new(pool.clone()))
            .configure(configure_routes),
    )
    .await;
    let token = login_token(&app, &username).await;

    let req = TestRequest::post()
        .uri("/api/v1/users/me/request-deletion")
        .insert_header(("Authorization", format!("Bearer {}", token)))
        .set_json(serde_json::json!({ "reason": "Duplicate attempt." }))
        .to_request();
    let resp = call_service(&app, req).await;
    assert_eq!(resp.status(), 409, "duplicate deletion request must return 409");
    let body: Value = read_body_json(resp).await;
    assert!(body["error"].is_string(), "409 must include an error field");
}

// ---------------------------------------------------------------------------
// GET /users/me/linked-students
// ---------------------------------------------------------------------------

/// GET /api/v1/users/me/linked-students without auth must return 401.
#[actix_web::test]
#[ignore = "requires TEST_DATABASE_URL"]
async fn test_linked_students_requires_auth() {
    let pool = test_pool().await;
    let app = init_service(
        App::new()
            .app_data(web::Data::new(pool))
            .configure(configure_routes),
    )
    .await;

    let req = TestRequest::get()
        .uri("/api/v1/users/me/linked-students")
        .to_request();
    let resp = call_service(&app, req).await;
    assert_eq!(resp.status(), 401);
    let body: Value = read_body_json(resp).await;
    assert!(body["error"].is_string(), "401 must include an error field");
}

/// GET /api/v1/users/me/linked-students by a Student (non-Parent) must return 403.
#[actix_web::test]
#[ignore = "requires TEST_DATABASE_URL"]
async fn test_linked_students_requires_parent_role() {
    let pool = test_pool().await;
    let suffix = &Uuid::new_v4().to_string()[..8];
    let username = format!("linked_norole_{}", suffix);
    seed_user(&pool, &username, "Student").await;

    let app = init_service(
        App::new()
            .app_data(web::Data::new(pool.clone()))
            .configure(configure_routes),
    )
    .await;
    let token = login_token(&app, &username).await;

    let req = TestRequest::get()
        .uri("/api/v1/users/me/linked-students")
        .insert_header(("Authorization", format!("Bearer {}", token)))
        .to_request();
    let resp = call_service(&app, req).await;
    assert_eq!(resp.status(), 403, "non-parent must receive 403");
    let body: Value = read_body_json(resp).await;
    assert!(body["error"].is_string(), "403 must include an error field");
}

/// GET /api/v1/users/me/linked-students for a Parent with a seeded link returns
/// an array where each item has id, username, display_name.
#[actix_web::test]
#[ignore = "requires TEST_DATABASE_URL"]
async fn test_linked_students_returns_array_with_shape_for_parent() {
    let pool = test_pool().await;
    let suffix = &Uuid::new_v4().to_string()[..8];
    let parent_name = format!("linked_parent_{}", suffix);
    let student_name = format!("linked_student_{}", suffix);

    let parent_id = seed_user(&pool, &parent_name, "Parent").await;
    let student_id = seed_user(&pool, &student_name, "Student").await;

    // Seed the parent–student link.
    sqlx::query(
        "INSERT INTO parent_student_links (parent_id, student_id, relationship)
         VALUES ($1, $2, 'parent')
         ON CONFLICT DO NOTHING",
    )
    .bind(parent_id)
    .bind(student_id)
    .execute(&pool)
    .await
    .expect("seed parent_student_links failed");

    let app = init_service(
        App::new()
            .app_data(web::Data::new(pool.clone()))
            .configure(configure_routes),
    )
    .await;
    let token = login_token(&app, &parent_name).await;

    let req = TestRequest::get()
        .uri("/api/v1/users/me/linked-students")
        .insert_header(("Authorization", format!("Bearer {}", token)))
        .to_request();
    let resp = call_service(&app, req).await;
    assert_eq!(resp.status(), 200, "GET /users/me/linked-students must return 200");
    let body: Value = read_body_json(resp).await;
    let items = body.as_array().expect("linked-students response must be a JSON array");
    assert!(!items.is_empty(), "linked-students list must be non-empty after seeding");

    // Find the seeded student.
    let found = items
        .iter()
        .find(|s| s["id"].as_str() == Some(&student_id.to_string()))
        .expect("seeded student must appear in linked-students list");

    assert!(found["id"].is_string(), "student.id must be string");
    assert_eq!(
        found["username"].as_str().unwrap(),
        student_name,
        "student.username must match"
    );
    // display_name may be null for a freshly seeded user — just assert presence.
    assert!(
        found["display_name"].is_string() || found["display_name"].is_null(),
        "student.display_name must be string or null"
    );
}

/// GET /api/v1/users/me/linked-students for a Parent with no links returns an
/// empty array (not a 404 or error).
#[actix_web::test]
#[ignore = "requires TEST_DATABASE_URL"]
async fn test_linked_students_empty_for_parent_with_no_links() {
    let pool = test_pool().await;
    let suffix = &Uuid::new_v4().to_string()[..8];
    let parent_name = format!("linked_empty_parent_{}", suffix);
    seed_user(&pool, &parent_name, "Parent").await;

    let app = init_service(
        App::new()
            .app_data(web::Data::new(pool.clone()))
            .configure(configure_routes),
    )
    .await;
    let token = login_token(&app, &parent_name).await;

    let req = TestRequest::get()
        .uri("/api/v1/users/me/linked-students")
        .insert_header(("Authorization", format!("Bearer {}", token)))
        .to_request();
    let resp = call_service(&app, req).await;
    assert_eq!(resp.status(), 200, "no-links parent must still get 200");
    let body: Value = read_body_json(resp).await;
    let items = body.as_array().expect("response must be a JSON array");
    assert!(items.is_empty(), "parent with no links must get empty array");
}
