/// Backup and report endpoint payload tests.
///
/// Covers:
///   Backups: GET /backups (list, auth, shape), GET /backups/{id} (404),
///            POST /backups auth+role enforcement
///   Reports: GET /reports (list, auth, shape), POST /reports validation
///            (invalid type, invalid date), GET /reports/{id} (404)
///
/// Note: POST /backups (create) and POST /reports (create) require external
/// process calls (pg_dump) and a configured EXPORTS_DIR / BACKUPS_DIR, so
/// those creation side-effects are not tested here. Auth and validation
/// checks are exercised without needing a real filesystem.
///
/// Requires a live PostgreSQL database:
///   TEST_DATABASE_URL=postgres://meridian:meridian@127.0.0.1:5433/meridian_seeded?sslmode=disable \
///     cargo test --test api_backups_reports_tests -- --include-ignored
use actix_web::test::{call_service, init_service, read_body, read_body_json, TestRequest};
use actix_web::{web, App};
use meridian_backend::config::Config;
use meridian_backend::routes::configure_routes;
use meridian_backend::services::auth::hash_password;
use serde_json::Value;
use sqlx::postgres::PgPoolOptions;
use sqlx::PgPool;
use uuid::Uuid;

async fn test_pool() -> PgPool {
    let url = std::env::var("TEST_DATABASE_URL")
        .expect("TEST_DATABASE_URL must be set to run api_backups_reports_tests");
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

fn test_config() -> Config {
    let db_url = std::env::var("TEST_DATABASE_URL")
        .unwrap_or_else(|_| "postgres://meridian:meridian@127.0.0.1:55432/meridian_seeded?sslmode=disable".into());
    let tmp = std::env::temp_dir().to_string_lossy().to_string();
    Config {
        database_url: db_url,
        host: "127.0.0.1".into(),
        port: 8080,
        session_secret: "x".repeat(64),
        session_max_age_seconds: 3600,
        log_level: "info".into(),
        backup_encryption_key: "test_backup_key_must_be_32chars_!".into(),
        exports_dir: tmp.clone(),
        backups_dir: tmp,
    }
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
// Backups — auth enforcement
// ---------------------------------------------------------------------------

/// GET /api/v1/backups without auth must return 401.
#[actix_web::test]
#[ignore = "requires TEST_DATABASE_URL"]
async fn test_list_backups_requires_auth() {
    let pool = test_pool().await;
    let app = init_service(
        App::new()
            .app_data(web::Data::new(pool))
            .app_data(web::Data::new(test_config()))
            .configure(configure_routes),
    )
    .await;

    let req = TestRequest::get().uri("/api/v1/backups").to_request();
    let resp = call_service(&app, req).await;
    assert_eq!(resp.status(), 401);
    let body: Value = read_body_json(resp).await;
    assert!(body["error"].is_string(), "401 must include an error field");
}

/// GET /api/v1/backups by a non-admin must return 403.
#[actix_web::test]
#[ignore = "requires TEST_DATABASE_URL"]
async fn test_list_backups_requires_admin_role() {
    let pool = test_pool().await;
    let suffix = &Uuid::new_v4().to_string()[..8];
    seed_user(&pool, &format!("bk_nadmin_{}", suffix), "Student").await;

    let app = init_service(
        App::new()
            .app_data(web::Data::new(pool.clone()))
            .app_data(web::Data::new(test_config()))
            .configure(configure_routes),
    )
    .await;
    let token = login_token(&app, &format!("bk_nadmin_{}", suffix)).await;

    let req = TestRequest::get()
        .uri("/api/v1/backups")
        .insert_header(("Authorization", format!("Bearer {}", token)))
        .to_request();
    let resp = call_service(&app, req).await;
    assert_eq!(resp.status(), 403, "non-admin must receive 403");
    let body: Value = read_body_json(resp).await;
    assert!(body["error"].is_string(), "403 must include an error field");
}

// ---------------------------------------------------------------------------
// Backups — response shape
// ---------------------------------------------------------------------------

/// GET /api/v1/backups by admin returns a JSON array (possibly empty for a
/// fresh test DB) and each item has the required metadata fields.
#[actix_web::test]
#[ignore = "requires TEST_DATABASE_URL"]
async fn test_list_backups_returns_array_with_required_fields() {
    let pool = test_pool().await;
    let suffix = &Uuid::new_v4().to_string()[..8];
    let admin_name = format!("bk_admin_{}", suffix);
    seed_super_admin(&pool, &admin_name).await;

    let app = init_service(
        App::new()
            .app_data(web::Data::new(pool.clone()))
            .app_data(web::Data::new(test_config()))
            .configure(configure_routes),
    )
    .await;
    let token = login_token(&app, &admin_name).await;

    // Seed a backup metadata row directly so the list is non-empty
    let backup_id = Uuid::new_v4();
    sqlx::query(
        "INSERT INTO backup_metadata (id, filename, backup_type, status, created_at)
         VALUES ($1, $2, 'full', 'completed', NOW())
         ON CONFLICT DO NOTHING",
    )
    .bind(backup_id)
    .bind(format!("backup_{}.mbak", backup_id))
    .execute(&pool)
    .await
    .expect("seed backup failed");

    let req = TestRequest::get()
        .uri("/api/v1/backups")
        .insert_header(("Authorization", format!("Bearer {}", token)))
        .to_request();
    let resp = call_service(&app, req).await;
    assert_eq!(resp.status(), 200);
    let body: Value = read_body_json(resp).await;
    let items = body.as_array().expect("backups must be a JSON array");
    assert!(!items.is_empty(), "backup list must be non-empty after seeding");

    for item in items {
        assert!(item["id"].is_string(), "backup.id must be string");
        assert!(item["filename"].is_string(), "backup.filename must be string");
        assert!(item["backup_type"].is_string(), "backup.backup_type must be string");
        assert!(item["status"].is_string(), "backup.status must be string");
        assert!(item["created_at"].is_string(), "backup.created_at must be string");
    }
}

/// GET /api/v1/backups/{id} for a nonexistent UUID must return 404.
#[actix_web::test]
#[ignore = "requires TEST_DATABASE_URL"]
async fn test_get_nonexistent_backup_returns_404() {
    let pool = test_pool().await;
    let suffix = &Uuid::new_v4().to_string()[..8];
    let admin_name = format!("bk_404_admin_{}", suffix);
    seed_super_admin(&pool, &admin_name).await;

    let app = init_service(
        App::new()
            .app_data(web::Data::new(pool.clone()))
            .app_data(web::Data::new(test_config()))
            .configure(configure_routes),
    )
    .await;
    let token = login_token(&app, &admin_name).await;

    let ghost_id = Uuid::new_v4();
    let req = TestRequest::get()
        .uri(&format!("/api/v1/backups/{}", ghost_id))
        .insert_header(("Authorization", format!("Bearer {}", token)))
        .to_request();
    let resp = call_service(&app, req).await;
    assert_eq!(resp.status(), 404);
    let body: Value = read_body_json(resp).await;
    assert!(body["error"].is_string(), "404 must include an error field");
}

/// GET /api/v1/backups/{id} for a seeded metadata entry returns the backup shape.
#[actix_web::test]
#[ignore = "requires TEST_DATABASE_URL"]
async fn test_get_backup_by_id_returns_correct_shape() {
    let pool = test_pool().await;
    let suffix = &Uuid::new_v4().to_string()[..8];
    let admin_name = format!("bk_byid_admin_{}", suffix);
    seed_super_admin(&pool, &admin_name).await;

    // Seed a backup record
    let backup_id = Uuid::new_v4();
    sqlx::query(
        "INSERT INTO backup_metadata (id, filename, backup_type, status, created_at)
         VALUES ($1, $2, 'full', 'completed', NOW())",
    )
    .bind(backup_id)
    .bind(format!("backup_{}.mbak", backup_id))
    .execute(&pool)
    .await
    .expect("seed backup failed");

    let app = init_service(
        App::new()
            .app_data(web::Data::new(pool.clone()))
            .app_data(web::Data::new(test_config()))
            .configure(configure_routes),
    )
    .await;
    let token = login_token(&app, &admin_name).await;

    let req = TestRequest::get()
        .uri(&format!("/api/v1/backups/{}", backup_id))
        .insert_header(("Authorization", format!("Bearer {}", token)))
        .to_request();
    let resp = call_service(&app, req).await;
    assert_eq!(resp.status(), 200);
    let body: Value = read_body_json(resp).await;

    assert_eq!(body["id"].as_str().unwrap(), backup_id.to_string(),
        "backup.id must match requested id");
    assert!(body["filename"].is_string(), "backup.filename must be string");
    assert_eq!(body["status"], "completed", "backup.status must be 'completed'");
}

// ---------------------------------------------------------------------------
// Reports — auth enforcement
// ---------------------------------------------------------------------------

/// GET /api/v1/reports without auth must return 401.
#[actix_web::test]
#[ignore = "requires TEST_DATABASE_URL"]
async fn test_list_reports_requires_auth() {
    let pool = test_pool().await;
    let app = init_service(
        App::new()
            .app_data(web::Data::new(pool))
            .app_data(web::Data::new(test_config()))
            .configure(configure_routes),
    )
    .await;

    let req = TestRequest::get().uri("/api/v1/reports").to_request();
    let resp = call_service(&app, req).await;
    assert_eq!(resp.status(), 401);
    let body: Value = read_body_json(resp).await;
    assert!(body["error"].is_string(), "401 must include an error field");
}

/// GET /api/v1/reports by non-admin must return 403.
#[actix_web::test]
#[ignore = "requires TEST_DATABASE_URL"]
async fn test_list_reports_requires_admin_role() {
    let pool = test_pool().await;
    let suffix = &Uuid::new_v4().to_string()[..8];
    seed_user(&pool, &format!("rpt_nadmin_{}", suffix), "Student").await;

    let app = init_service(
        App::new()
            .app_data(web::Data::new(pool.clone()))
            .app_data(web::Data::new(test_config()))
            .configure(configure_routes),
    )
    .await;
    let token = login_token(&app, &format!("rpt_nadmin_{}", suffix)).await;

    let req = TestRequest::get()
        .uri("/api/v1/reports")
        .insert_header(("Authorization", format!("Bearer {}", token)))
        .to_request();
    let resp = call_service(&app, req).await;
    assert_eq!(resp.status(), 403, "non-admin must receive 403");
    let body: Value = read_body_json(resp).await;
    assert!(body["error"].is_string(), "403 must include an error field");
}

// ---------------------------------------------------------------------------
// Reports — list shape
// ---------------------------------------------------------------------------

/// GET /api/v1/reports by admin returns a JSON array; each item has required fields.
#[actix_web::test]
#[ignore = "requires TEST_DATABASE_URL"]
async fn test_list_reports_returns_array() {
    let pool = test_pool().await;
    let suffix = &Uuid::new_v4().to_string()[..8];
    let admin_name = format!("rpt_admin_{}", suffix);
    seed_super_admin(&pool, &admin_name).await;

    let app = init_service(
        App::new()
            .app_data(web::Data::new(pool.clone()))
            .app_data(web::Data::new(test_config()))
            .configure(configure_routes),
    )
    .await;
    let token = login_token(&app, &admin_name).await;

    let req = TestRequest::get()
        .uri("/api/v1/reports")
        .insert_header(("Authorization", format!("Bearer {}", token)))
        .to_request();
    let resp = call_service(&app, req).await;
    assert_eq!(resp.status(), 200);
    let body: Value = read_body_json(resp).await;
    let items = body.as_array().expect("reports must be a JSON array");

    for item in items {
        assert!(item["id"].is_string(), "report.id must be string");
        assert!(item["name"].is_string(), "report.name must be string");
        assert!(item["report_type"].is_string(), "report.report_type must be string");
        assert!(item["status"].is_string(), "report.status must be string");
        assert!(item["pii_masked"].is_boolean(), "report.pii_masked must be boolean");
        assert!(item["created_at"].is_string(), "report.created_at must be string");
    }
}

// ---------------------------------------------------------------------------
// Reports — creation validation
// ---------------------------------------------------------------------------

/// POST /api/v1/reports with an invalid report_type must return 422.
#[actix_web::test]
#[ignore = "requires TEST_DATABASE_URL"]
async fn test_create_report_invalid_type_returns_422() {
    let pool = test_pool().await;
    let suffix = &Uuid::new_v4().to_string()[..8];
    let admin_name = format!("rpt_inv_admin_{}", suffix);
    seed_super_admin(&pool, &admin_name).await;

    let app = init_service(
        App::new()
            .app_data(web::Data::new(pool.clone()))
            .app_data(web::Data::new(test_config()))
            .configure(configure_routes),
    )
    .await;
    let token = login_token(&app, &admin_name).await;

    let req = TestRequest::post()
        .uri("/api/v1/reports")
        .insert_header(("Authorization", format!("Bearer {}", token)))
        .set_json(serde_json::json!({
            "report_type": "unknown_type",
            "start_date": "2026-01-01",
            "end_date": "2026-01-31"
        }))
        .to_request();
    let resp = call_service(&app, req).await;
    assert_eq!(resp.status(), 422, "invalid report_type must return 422");
    let body: Value = read_body_json(resp).await;
    assert!(body["error"].is_string(), "422 must include an error field");
}

/// POST /api/v1/reports with an invalid start_date must return 422.
#[actix_web::test]
#[ignore = "requires TEST_DATABASE_URL"]
async fn test_create_report_invalid_date_returns_422() {
    let pool = test_pool().await;
    let suffix = &Uuid::new_v4().to_string()[..8];
    let admin_name = format!("rpt_date_admin_{}", suffix);
    seed_super_admin(&pool, &admin_name).await;

    let app = init_service(
        App::new()
            .app_data(web::Data::new(pool.clone()))
            .app_data(web::Data::new(test_config()))
            .configure(configure_routes),
    )
    .await;
    let token = login_token(&app, &admin_name).await;

    let req = TestRequest::post()
        .uri("/api/v1/reports")
        .insert_header(("Authorization", format!("Bearer {}", token)))
        .set_json(serde_json::json!({
            "report_type": "orders",
            "start_date": "not-a-date",
            "end_date": "2026-01-31"
        }))
        .to_request();
    let resp = call_service(&app, req).await;
    assert_eq!(resp.status(), 422, "invalid date must return 422");
    let body: Value = read_body_json(resp).await;
    assert!(body["error"].is_string(), "422 must include an error field");
}

/// POST /api/v1/reports without auth must return 401.
#[actix_web::test]
#[ignore = "requires TEST_DATABASE_URL"]
async fn test_create_report_requires_auth() {
    let pool = test_pool().await;
    let app = init_service(
        App::new()
            .app_data(web::Data::new(pool))
            .app_data(web::Data::new(test_config()))
            .configure(configure_routes),
    )
    .await;

    let req = TestRequest::post()
        .uri("/api/v1/reports")
        .set_json(serde_json::json!({
            "report_type": "orders",
            "start_date": "2026-01-01",
            "end_date": "2026-01-31"
        }))
        .to_request();
    let resp = call_service(&app, req).await;
    assert_eq!(resp.status(), 401);
    let body: Value = read_body_json(resp).await;
    assert!(body["error"].is_string(), "401 must include an error field");
}

/// GET /api/v1/reports/{id} for a nonexistent UUID must return 404.
#[actix_web::test]
#[ignore = "requires TEST_DATABASE_URL"]
async fn test_get_nonexistent_report_returns_404() {
    let pool = test_pool().await;
    let suffix = &Uuid::new_v4().to_string()[..8];
    let admin_name = format!("rpt_404_admin_{}", suffix);
    seed_super_admin(&pool, &admin_name).await;

    let app = init_service(
        App::new()
            .app_data(web::Data::new(pool.clone()))
            .app_data(web::Data::new(test_config()))
            .configure(configure_routes),
    )
    .await;
    let token = login_token(&app, &admin_name).await;

    let ghost_id = Uuid::new_v4();
    let req = TestRequest::get()
        .uri(&format!("/api/v1/reports/{}", ghost_id))
        .insert_header(("Authorization", format!("Bearer {}", token)))
        .to_request();
    let resp = call_service(&app, req).await;
    assert_eq!(resp.status(), 404);
    let body: Value = read_body_json(resp).await;
    assert!(body["error"].is_string(), "404 must include an error field");
}

// ---------------------------------------------------------------------------
// Reports — POST creation happy path
// ---------------------------------------------------------------------------

/// POST /api/v1/reports with a valid "orders" type returns 201 with the full
/// completed-report shape: job_id, name, report_type, status, output_path,
/// row_count, pii_masked, checksum.
#[actix_web::test]
#[ignore = "requires TEST_DATABASE_URL"]
async fn test_create_orders_report_returns_completed_shape() {
    let pool = test_pool().await;
    let suffix = &Uuid::new_v4().to_string()[..8];
    let admin_name = format!("rpt_create_admin_{}", suffix);
    seed_super_admin(&pool, &admin_name).await;

    let app = init_service(
        App::new()
            .app_data(web::Data::new(pool.clone()))
            .app_data(web::Data::new(test_config()))
            .configure(configure_routes),
    )
    .await;
    let token = login_token(&app, &admin_name).await;

    let req = TestRequest::post()
        .uri("/api/v1/reports")
        .insert_header(("Authorization", format!("Bearer {}", token)))
        .set_json(serde_json::json!({
            "report_type": "orders",
            "start_date": "2025-01-01",
            "end_date": "2025-12-31",
            "pii_masked": true
        }))
        .to_request();
    let resp = call_service(&app, req).await;
    assert_eq!(resp.status(), 201, "valid report creation must return 201");
    let body: Value = read_body_json(resp).await;

    assert!(body["job_id"].is_string(), "response must include job_id");
    assert!(body["name"].is_string(), "response must include name");
    assert_eq!(body["report_type"], "orders", "response report_type must match request");
    assert_eq!(body["status"], "completed", "successfully generated report must have status 'completed'");
    assert!(body["output_path"].is_string(), "response must include output_path");
    assert!(body["row_count"].is_number(), "response must include row_count");
    assert_eq!(body["pii_masked"], true, "pii_masked must reflect the request value");
    assert!(body["checksum"].is_string(), "response must include checksum");
}

/// POST /api/v1/reports; then GET /api/v1/reports/{job_id} returns the same record.
#[actix_web::test]
#[ignore = "requires TEST_DATABASE_URL"]
async fn test_get_created_report_by_id_returns_correct_shape() {
    let pool = test_pool().await;
    let suffix = &Uuid::new_v4().to_string()[..8];
    let admin_name = format!("rpt_getbyid_admin_{}", suffix);
    seed_super_admin(&pool, &admin_name).await;

    let app = init_service(
        App::new()
            .app_data(web::Data::new(pool.clone()))
            .app_data(web::Data::new(test_config()))
            .configure(configure_routes),
    )
    .await;
    let token = login_token(&app, &admin_name).await;

    // Create a report first.
    let req = TestRequest::post()
        .uri("/api/v1/reports")
        .insert_header(("Authorization", format!("Bearer {}", token)))
        .set_json(serde_json::json!({
            "report_type": "kpi",
            "start_date": "2025-01-01",
            "end_date": "2025-12-31",
            "pii_masked": true
        }))
        .to_request();
    let create_body: Value = read_body_json(call_service(&app, req).await).await;
    let job_id = create_body["job_id"].as_str().expect("job_id missing from create response");

    // Retrieve it by ID.
    let req = TestRequest::get()
        .uri(&format!("/api/v1/reports/{}", job_id))
        .insert_header(("Authorization", format!("Bearer {}", token)))
        .to_request();
    let resp = call_service(&app, req).await;
    assert_eq!(resp.status(), 200, "GET /reports/{{id}} must return 200 after creation");
    let body: Value = read_body_json(resp).await;

    assert_eq!(
        body["id"].as_str().unwrap(),
        job_id,
        "retrieved report id must match job_id from create response"
    );
    assert!(body["name"].is_string(), "report.name must be string");
    assert_eq!(body["report_type"], "kpi", "report_type must match");
    assert_eq!(body["status"], "completed", "report status must be 'completed'");
    assert!(body["pii_masked"].is_boolean(), "report.pii_masked must be boolean");
    assert!(body["created_at"].is_string(), "report.created_at must be string");
}

// ---------------------------------------------------------------------------
// Backups — POST creation
// ---------------------------------------------------------------------------

/// POST /api/v1/backups by an authenticated admin must NOT return 401 or 403.
/// It returns either 201 (pg_dump available) or 500 (pg_dump not found in CI).
/// Either outcome proves the auth layer is satisfied; we verify the 201 shape
/// when the command succeeds.
#[actix_web::test]
#[ignore = "requires TEST_DATABASE_URL"]
async fn test_create_backup_auth_passes_or_internal_error() {
    let pool = test_pool().await;
    let suffix = &Uuid::new_v4().to_string()[..8];
    let admin_name = format!("bk_create_admin_{}", suffix);
    seed_super_admin(&pool, &admin_name).await;

    let app = init_service(
        App::new()
            .app_data(web::Data::new(pool.clone()))
            .app_data(web::Data::new(test_config()))
            .configure(configure_routes),
    )
    .await;
    let token = login_token(&app, &admin_name).await;

    let req = TestRequest::post()
        .uri("/api/v1/backups")
        .insert_header(("Authorization", format!("Bearer {}", token)))
        .set_json(serde_json::json!({ "notes": "test backup" }))
        .to_request();
    let resp = call_service(&app, req).await;

    let status = resp.status().as_u16();
    assert!(
        status != 401 && status != 403,
        "authenticated admin must not receive 401 or 403, got {}",
        status
    );

    if status == 201 {
        let body: Value = read_body_json(resp).await;
        assert!(body["backup_id"].is_string(), "backup_id must be string on 201");
        assert!(body["filename"].is_string(), "filename must be string on 201");
        assert_eq!(body["status"], "completed", "status must be 'completed' on 201");
        assert!(body["message"].is_string(), "message must be string on 201");
        assert!(body["checksum"].is_string(), "checksum must be string on 201");
    }
    // status == 500 means pg_dump is unavailable in this environment — acceptable.
}

// ---------------------------------------------------------------------------
// Reports — GET /{id}/download
// ---------------------------------------------------------------------------

/// GET /api/v1/reports/{id}/download without auth returns 401.
#[actix_web::test]
#[ignore = "requires TEST_DATABASE_URL"]
async fn test_download_report_requires_auth() {
    let pool = test_pool().await;
    let app = init_service(
        App::new()
            .app_data(web::Data::new(pool))
            .app_data(web::Data::new(test_config()))
            .configure(configure_routes),
    )
    .await;

    let req = TestRequest::get()
        .uri(&format!("/api/v1/reports/{}/download", Uuid::new_v4()))
        .to_request();
    let resp = call_service(&app, req).await;
    assert_eq!(resp.status(), 401);
    let body: Value = read_body_json(resp).await;
    assert!(body["error"].is_string(), "401 must include an error field");
}

/// GET /api/v1/reports/{id}/download with a non-existent ID returns 404.
#[actix_web::test]
#[ignore = "requires TEST_DATABASE_URL"]
async fn test_download_nonexistent_report_returns_404() {
    let pool = test_pool().await;
    let suffix = &Uuid::new_v4().to_string()[..8];
    let admin_name = format!("rpt_dl_404_admin_{}", suffix);
    seed_super_admin(&pool, &admin_name).await;

    let app = init_service(
        App::new()
            .app_data(web::Data::new(pool.clone()))
            .app_data(web::Data::new(test_config()))
            .configure(configure_routes),
    )
    .await;
    let token = login_token(&app, &admin_name).await;

    let req = TestRequest::get()
        .uri(&format!("/api/v1/reports/{}/download", Uuid::new_v4()))
        .insert_header(("Authorization", format!("Bearer {}", token)))
        .to_request();
    let resp = call_service(&app, req).await;
    assert_eq!(resp.status(), 404);
    let body: Value = read_body_json(resp).await;
    assert!(body["error"].is_string(), "404 must include an error field");
}

/// Create a report then download it — response must be text/csv with non-empty content
/// and a Content-Disposition: attachment header.
#[actix_web::test]
#[ignore = "requires TEST_DATABASE_URL"]
async fn test_download_completed_report_returns_csv() {
    let pool = test_pool().await;
    let suffix = &Uuid::new_v4().to_string()[..8];
    let admin_name = format!("rpt_dl_admin_{}", suffix);
    seed_super_admin(&pool, &admin_name).await;

    let app = init_service(
        App::new()
            .app_data(web::Data::new(pool.clone()))
            .app_data(web::Data::new(test_config()))
            .configure(configure_routes),
    )
    .await;
    let token = login_token(&app, &admin_name).await;

    // Create the report first.
    let req = TestRequest::post()
        .uri("/api/v1/reports")
        .insert_header(("Authorization", format!("Bearer {}", token)))
        .set_json(serde_json::json!({
            "report_type": "orders",
            "start_date": "2025-01-01",
            "end_date": "2025-12-31",
            "pii_masked": true
        }))
        .to_request();
    let create_body: Value = read_body_json(call_service(&app, req).await).await;
    let job_id = create_body["job_id"].as_str().expect("job_id missing from create response");
    assert_eq!(
        create_body["status"], "completed",
        "report must reach completed status before download"
    );

    // Download it.
    let req = TestRequest::get()
        .uri(&format!("/api/v1/reports/{}/download", job_id))
        .insert_header(("Authorization", format!("Bearer {}", token)))
        .to_request();
    let resp = call_service(&app, req).await;
    assert_eq!(resp.status(), 200, "download must return 200");

    let content_type = resp
        .headers()
        .get("content-type")
        .expect("Content-Type header must be present")
        .to_str()
        .unwrap();
    assert!(
        content_type.contains("text/csv"),
        "Content-Type must be text/csv, got {}",
        content_type
    );

    let content_disp = resp
        .headers()
        .get("content-disposition")
        .expect("Content-Disposition header must be present")
        .to_str()
        .unwrap();
    assert!(
        content_disp.contains("attachment"),
        "Content-Disposition must indicate attachment, got {}",
        content_disp
    );

    let body_bytes = read_body(resp).await;
    assert!(!body_bytes.is_empty(), "CSV download body must not be empty");
}

/// POST /api/v1/backups/{id}/restore on a backup with status 'pending' returns 409.
#[actix_web::test]
#[ignore = "requires TEST_DATABASE_URL"]
async fn test_restore_pending_backup_returns_409() {
    let pool = test_pool().await;
    let suffix = &Uuid::new_v4().to_string()[..8];
    let admin_name = format!("bk_restore_admin_{}", suffix);
    seed_super_admin(&pool, &admin_name).await;

    // Seed a backup record with status 'pending' (not completed).
    let backup_id = Uuid::new_v4();
    sqlx::query(
        "INSERT INTO backup_metadata (id, filename, backup_type, status, created_at)
         VALUES ($1, $2, 'full', 'pending', NOW())",
    )
    .bind(backup_id)
    .bind(format!("backup_{}.mbak", backup_id))
    .execute(&pool)
    .await
    .expect("seed pending backup failed");

    let app = init_service(
        App::new()
            .app_data(web::Data::new(pool.clone()))
            .app_data(web::Data::new(test_config()))
            .configure(configure_routes),
    )
    .await;
    let token = login_token(&app, &admin_name).await;

    let req = TestRequest::post()
        .uri(&format!("/api/v1/backups/{}/restore", backup_id))
        .insert_header(("Authorization", format!("Bearer {}", token)))
        .set_json(serde_json::json!({}))
        .to_request();
    let resp = call_service(&app, req).await;
    assert_eq!(resp.status(), 409, "restoring a non-completed backup must return 409");
    let body: Value = read_body_json(resp).await;
    assert!(body["error"].is_string(), "409 must include an error field");
}
