/// API authorization integration tests.
///
/// Verifies HTTP-level authentication (401) and role/scope enforcement (403)
/// for the most security-critical endpoints.
///
/// Requires a live PostgreSQL database:
///   TEST_DATABASE_URL=postgres://meridian:meridian@127.0.0.1:5433/meridian_seeded?sslmode=disable \
///     cargo test --test api_authorization_tests -- --include-ignored
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
        .expect("TEST_DATABASE_URL must be set to run api_authorization_tests");
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
        .expect("seeded user missing");
    sqlx::query("INSERT INTO user_roles (user_id, role_id) VALUES ($1, $2) ON CONFLICT DO NOTHING")
        .bind(uid)
        .bind(role_id)
        .execute(pool)
        .await
        .expect("assign role failed");

    uid
}

async fn make_super_admin(pool: &PgPool, user_id: Uuid) {
    sqlx::query("UPDATE users SET is_super_admin = TRUE WHERE id = $1")
        .bind(user_id)
        .execute(pool)
        .await
        .expect("failed to set super admin");
}

/// Seeds district → campus → school and returns (campus_id, school_id).
/// Uses the actual schema columns (no `code` column — districts/campuses/schools
/// identify uniquely by name within their parent).
async fn seed_campus_and_school(pool: &PgPool, suffix: &str) -> (Uuid, Uuid) {
    let district_id = Uuid::new_v4();
    let campus_id = Uuid::new_v4();
    let school_id = Uuid::new_v4();

    sqlx::query(
        "INSERT INTO districts (id, name, state, created_at)
         VALUES ($1, $2, 'TX', NOW())",
    )
    .bind(district_id)
    .bind(format!("District_{}", suffix))
    .execute(pool)
    .await
    .expect("seed district failed");

    sqlx::query(
        "INSERT INTO campuses (id, district_id, name, created_at)
         VALUES ($1, $2, $3, NOW())",
    )
    .bind(campus_id)
    .bind(district_id)
    .bind(format!("Campus_{}", suffix))
    .execute(pool)
    .await
    .expect("seed campus failed");

    sqlx::query(
        "INSERT INTO schools (id, campus_id, name, school_type, created_at)
         VALUES ($1, $2, $3, 'general', NOW())",
    )
    .bind(school_id)
    .bind(campus_id)
    .bind(format!("School_{}", suffix))
    .execute(pool)
    .await
    .expect("seed school failed");

    (campus_id, school_id)
}

async fn assign_admin_to_campus(pool: &PgPool, admin_id: Uuid, campus_id: Uuid) {
    sqlx::query(
        "INSERT INTO admin_scope_assignments (admin_id, scope_type, scope_id)
         VALUES ($1, 'campus', $2)
         ON CONFLICT DO NOTHING",
    )
    .bind(admin_id)
    .bind(campus_id)
    .execute(pool)
    .await
    .expect("assign admin scope failed");
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
        .set_json(serde_json::json!({"username": username, "password": "TestPass2024!!"}))
        .to_request();
    let resp = call_service(app, req).await;
    let body: serde_json::Value = actix_web::test::read_body_json(resp).await;
    body["token"].as_str().expect("token missing").to_string()
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

/// Reports and backup list endpoints must reject unauthenticated requests
/// with 401 and an `error` field in the body.
#[actix_web::test]
#[ignore = "requires TEST_DATABASE_URL"]
async fn test_reports_and_backups_require_authentication() {
    let pool = test_pool().await;
    let app = init_service(
        App::new()
            .app_data(web::Data::new(pool))
            .configure(configure_routes),
    )
    .await;

    for path in ["/api/v1/reports", "/api/v1/backups"] {
        let req = TestRequest::get().uri(path).to_request();
        let resp = call_service(&app, req).await;
        assert_eq!(resp.status(), 401, "{} should require auth", path);
        let body: Value = read_body_json(resp).await;
        assert!(
            body["error"].is_string(),
            "{} 401 response must have an `error` field",
            path
        );
    }
}

/// Non-admin roles (e.g. Teacher) must receive 403 on admin-only endpoints.
#[actix_web::test]
#[ignore = "requires TEST_DATABASE_URL"]
async fn test_non_admin_forbidden_from_reports_and_backups() {
    let pool = test_pool().await;
    let suffix = &Uuid::new_v4().to_string()[..8];
    let teacher_name = format!("api_teacher_{}", suffix);
    seed_user(&pool, &teacher_name, "Teacher").await;

    let app = init_service(
        App::new()
            .app_data(web::Data::new(pool.clone()))
            .configure(configure_routes),
    )
    .await;
    let token = login_token(&app, &teacher_name).await;

    for path in ["/api/v1/reports", "/api/v1/backups"] {
        let req = TestRequest::get()
            .uri(path)
            .insert_header(("Authorization", format!("Bearer {}", token)))
            .to_request();
        let resp = call_service(&app, req).await;
        assert_eq!(resp.status(), 403, "{} should be admin-only", path);
        let body: Value = read_body_json(resp).await;
        assert!(
            body["error"].is_string(),
            "{} 403 response must have an `error` field",
            path
        );
    }
}

/// A scoped admin (campus-limited, is_super_admin = false) must be denied 403
/// on global operations such as reports and backups.
#[actix_web::test]
#[ignore = "requires TEST_DATABASE_URL"]
async fn test_scoped_admin_forbidden_from_global_reports_and_backups() {
    let pool = test_pool().await;
    let suffix = &Uuid::new_v4().to_string()[..8];
    let admin_name = format!("api_scoped_admin_{}", suffix);
    let admin_id = seed_user(&pool, &admin_name, "Administrator").await;
    let (campus_id, _school_id) = seed_campus_and_school(&pool, suffix).await;
    assign_admin_to_campus(&pool, admin_id, campus_id).await;

    let app = init_service(
        App::new()
            .app_data(web::Data::new(pool.clone()))
            .configure(configure_routes),
    )
    .await;
    let token = login_token(&app, &admin_name).await;

    for path in ["/api/v1/reports", "/api/v1/backups"] {
        let req = TestRequest::get()
            .uri(path)
            .insert_header(("Authorization", format!("Bearer {}", token)))
            .to_request();
        let resp = call_service(&app, req).await;
        assert_eq!(resp.status(), 403, "{} should require global admin scope", path);
    }
}

/// A super-admin (is_super_admin = true) must be able to list reports and backups.
#[actix_web::test]
#[ignore = "requires TEST_DATABASE_URL"]
async fn test_super_admin_can_access_reports_and_backups_lists() {
    let pool = test_pool().await;
    let suffix = &Uuid::new_v4().to_string()[..8];
    let admin_name = format!("api_super_admin_{}", suffix);
    let admin_id = seed_user(&pool, &admin_name, "Administrator").await;
    make_super_admin(&pool, admin_id).await;

    let app = init_service(
        App::new()
            .app_data(web::Data::new(pool.clone()))
            .configure(configure_routes),
    )
    .await;
    let token = login_token(&app, &admin_name).await;

    for path in ["/api/v1/reports", "/api/v1/backups"] {
        let req = TestRequest::get()
            .uri(path)
            .insert_header(("Authorization", format!("Bearer {}", token)))
            .to_request();
        let resp = call_service(&app, req).await;
        assert_eq!(resp.status(), 200, "{} should be accessible to super-admin", path);
        let body: Value = read_body_json(resp).await;
        assert!(
            body.is_array(),
            "{} 200 response must be a JSON array, got: {}",
            path,
            body
        );
    }
}

/// Student role must be denied 403 on the admin users endpoint.
#[actix_web::test]
#[ignore = "requires TEST_DATABASE_URL"]
async fn test_student_cannot_access_admin_users_endpoint() {
    let pool = test_pool().await;
    let suffix = &Uuid::new_v4().to_string()[..8];
    let student_name = format!("api_student_{}", suffix);
    seed_user(&pool, &student_name, "Student").await;

    let app = init_service(
        App::new()
            .app_data(web::Data::new(pool.clone()))
            .configure(configure_routes),
    )
    .await;
    let token = login_token(&app, &student_name).await;

    let req = TestRequest::get()
        .uri("/api/v1/admin/users")
        .insert_header(("Authorization", format!("Bearer {}", token)))
        .to_request();
    let resp = call_service(&app, req).await;
    assert_eq!(resp.status(), 403, "/api/v1/admin/users must be administrator-only");
}

/// Unauthenticated request to /api/v1/admin/users must receive 401.
#[actix_web::test]
#[ignore = "requires TEST_DATABASE_URL"]
async fn test_admin_users_endpoint_requires_authentication() {
    let pool = test_pool().await;
    let app = init_service(
        App::new()
            .app_data(web::Data::new(pool))
            .configure(configure_routes),
    )
    .await;

    let req = TestRequest::get().uri("/api/v1/admin/users").to_request();
    let resp = call_service(&app, req).await;
    assert_eq!(resp.status(), 401, "/api/v1/admin/users must reject unauthenticated requests");
}
