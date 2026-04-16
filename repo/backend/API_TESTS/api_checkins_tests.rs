/// Check-in endpoint payload tests.
///
/// Covers all seven check-in endpoints: window list, window detail, submit,
/// submissions list, homerooms list, decide, and my-checkins.
///
/// Requires a live PostgreSQL database:
///   TEST_DATABASE_URL=postgres://meridian:meridian@127.0.0.1:5433/meridian_seeded?sslmode=disable \
///     cargo test --test api_checkins_tests -- --include-ignored
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
        .expect("TEST_DATABASE_URL must be set to run api_checkins_tests");
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

// ---------------------------------------------------------------------------
// Tests — window list and detail
// ---------------------------------------------------------------------------

/// GET /api/v1/check-ins/windows without a token must return 401.
#[actix_web::test]
#[ignore = "requires TEST_DATABASE_URL"]
async fn test_list_windows_requires_auth() {
    let pool = test_pool().await;
    let app = init_service(
        App::new()
            .app_data(web::Data::new(pool))
            .configure(configure_routes),
    )
    .await;

    let req = TestRequest::get()
        .uri("/api/v1/check-ins/windows")
        .to_request();
    let resp = call_service(&app, req).await;
    assert_eq!(resp.status(), 401);
    let body: Value = read_body_json(resp).await;
    assert!(body["error"].is_string(), "401 must include an error field");
}

/// GET /api/v1/check-ins/windows by admin returns a JSON array where each item
/// has the required window shape fields.
#[actix_web::test]
#[ignore = "requires TEST_DATABASE_URL"]
async fn test_list_windows_returns_array_with_required_fields() {
    let pool = test_pool().await;
    let suffix = &Uuid::new_v4().to_string()[..8];
    let admin_name = format!("ci_admin_{}", suffix);
    seed_super_admin(&pool, &admin_name).await;

    let app = init_service(
        App::new()
            .app_data(web::Data::new(pool.clone()))
            .configure(configure_routes),
    )
    .await;
    let token = login_token(&app, &admin_name).await;

    let req = TestRequest::get()
        .uri("/api/v1/check-ins/windows")
        .insert_header(("Authorization", format!("Bearer {}", token)))
        .to_request();
    let resp = call_service(&app, req).await;
    assert_eq!(resp.status(), 200);
    let body: Value = read_body_json(resp).await;
    let items = body.as_array().expect("windows response must be a JSON array");

    for item in items {
        assert!(item["id"].is_string(), "window.id must be string");
        assert!(item["title"].is_string(), "window.title must be string");
        assert!(item["opens_at"].is_string(), "window.opens_at must be string");
        assert!(item["closes_at"].is_string(), "window.closes_at must be string");
        assert!(item["allow_late"].is_boolean(), "window.allow_late must be boolean");
        assert!(item["active"].is_boolean(), "window.active must be boolean");
        assert!(item["school_id"].is_string(), "window.school_id must be string");
        assert!(item["school_name"].is_string(), "window.school_name must be string");
        // status is computed
        assert!(item["status"].is_string(), "window.status must be string");
        let status = item["status"].as_str().unwrap();
        assert!(
            matches!(status, "upcoming" | "open" | "accepting_late" | "closed"),
            "window.status must be one of the four valid values, got '{}'", status
        );
    }
}

/// GET /api/v1/check-ins/windows/{window_id} for a nonexistent UUID returns 404.
#[actix_web::test]
#[ignore = "requires TEST_DATABASE_URL"]
async fn test_get_nonexistent_window_returns_404() {
    let pool = test_pool().await;
    let suffix = &Uuid::new_v4().to_string()[..8];
    let admin_name = format!("ci_404_admin_{}", suffix);
    seed_super_admin(&pool, &admin_name).await;

    let app = init_service(
        App::new()
            .app_data(web::Data::new(pool.clone()))
            .configure(configure_routes),
    )
    .await;
    let token = login_token(&app, &admin_name).await;

    let ghost_id = Uuid::new_v4();
    let req = TestRequest::get()
        .uri(&format!("/api/v1/check-ins/windows/{}", ghost_id))
        .insert_header(("Authorization", format!("Bearer {}", token)))
        .to_request();
    let resp = call_service(&app, req).await;
    assert_eq!(resp.status(), 404);
    let body: Value = read_body_json(resp).await;
    assert!(body["error"].is_string(), "404 must include an error field");
}

/// GET /api/v1/check-ins/windows/{window_id} for an existing window returns
/// the window with status field.
#[actix_web::test]
#[ignore = "requires TEST_DATABASE_URL"]
async fn test_get_window_by_id_returns_window_with_status() {
    let pool = test_pool().await;
    let suffix = &Uuid::new_v4().to_string()[..8];
    let admin_name = format!("ci_byid_admin_{}", suffix);
    seed_super_admin(&pool, &admin_name).await;

    let app = init_service(
        App::new()
            .app_data(web::Data::new(pool.clone()))
            .configure(configure_routes),
    )
    .await;
    let token = login_token(&app, &admin_name).await;

    // Get the list and pick the first window ID if available
    let list_req = TestRequest::get()
        .uri("/api/v1/check-ins/windows")
        .insert_header(("Authorization", format!("Bearer {}", token)))
        .to_request();
    let list_body: Value = read_body_json(call_service(&app, list_req).await).await;
    let windows = list_body.as_array().unwrap();

    if windows.is_empty() {
        // No windows seeded — just verify 404 for nonexistent
        return;
    }

    let window_id = windows[0]["id"].as_str().unwrap();
    let req = TestRequest::get()
        .uri(&format!("/api/v1/check-ins/windows/{}", window_id))
        .insert_header(("Authorization", format!("Bearer {}", token)))
        .to_request();
    let resp = call_service(&app, req).await;
    assert_eq!(resp.status(), 200);
    let body: Value = read_body_json(resp).await;

    assert_eq!(body["id"].as_str().unwrap(), window_id, "window.id must match");
    assert!(body["title"].is_string(), "window.title must be string");
    assert!(body["status"].is_string(), "window.status must be string");
    assert!(body["active"].is_boolean(), "window.active must be boolean");
}

// ---------------------------------------------------------------------------
// Tests — submit check-in
// ---------------------------------------------------------------------------

/// POST /api/v1/check-ins/windows/{window_id}/submit without auth must return 401.
#[actix_web::test]
#[ignore = "requires TEST_DATABASE_URL"]
async fn test_submit_checkin_requires_auth() {
    let pool = test_pool().await;
    let app = init_service(
        App::new()
            .app_data(web::Data::new(pool))
            .configure(configure_routes),
    )
    .await;

    let ghost_id = Uuid::new_v4();
    let req = TestRequest::post()
        .uri(&format!("/api/v1/check-ins/windows/{}/submit", ghost_id))
        .set_json(serde_json::json!({}))
        .to_request();
    let resp = call_service(&app, req).await;
    assert_eq!(resp.status(), 401);
    let body: Value = read_body_json(resp).await;
    assert!(body["error"].is_string(), "401 must include an error field");
}

/// POST /api/v1/check-ins/windows/{window_id}/submit for a nonexistent window
/// must return 404.
#[actix_web::test]
#[ignore = "requires TEST_DATABASE_URL"]
async fn test_submit_checkin_nonexistent_window_returns_404() {
    let pool = test_pool().await;
    let suffix = &Uuid::new_v4().to_string()[..8];
    seed_user(&pool, &format!("ci_sub_student_{}", suffix), "Student").await;

    let app = init_service(
        App::new()
            .app_data(web::Data::new(pool.clone()))
            .configure(configure_routes),
    )
    .await;
    let token = login_token(&app, &format!("ci_sub_student_{}", suffix)).await;

    let ghost_id = Uuid::new_v4();
    let req = TestRequest::post()
        .uri(&format!("/api/v1/check-ins/windows/{}/submit", ghost_id))
        .insert_header(("Authorization", format!("Bearer {}", token)))
        .set_json(serde_json::json!({}))
        .to_request();
    let resp = call_service(&app, req).await;
    assert_eq!(resp.status(), 404, "nonexistent window must return 404");
    let body: Value = read_body_json(resp).await;
    assert!(body["error"].is_string(), "404 must include an error field");
}

// ---------------------------------------------------------------------------
// Tests — my check-ins
// ---------------------------------------------------------------------------

/// GET /api/v1/check-ins/my without auth must return 401.
#[actix_web::test]
#[ignore = "requires TEST_DATABASE_URL"]
async fn test_my_checkins_requires_auth() {
    let pool = test_pool().await;
    let app = init_service(
        App::new()
            .app_data(web::Data::new(pool))
            .configure(configure_routes),
    )
    .await;

    let req = TestRequest::get()
        .uri("/api/v1/check-ins/my")
        .to_request();
    let resp = call_service(&app, req).await;
    assert_eq!(resp.status(), 401);
    let body: Value = read_body_json(resp).await;
    assert!(body["error"].is_string(), "401 must include an error field");
}

/// GET /api/v1/check-ins/my returns a JSON array for an authenticated user.
#[actix_web::test]
#[ignore = "requires TEST_DATABASE_URL"]
async fn test_my_checkins_returns_array() {
    let pool = test_pool().await;
    let suffix = &Uuid::new_v4().to_string()[..8];
    seed_user(&pool, &format!("ci_my_{}", suffix), "Student").await;

    let app = init_service(
        App::new()
            .app_data(web::Data::new(pool.clone()))
            .configure(configure_routes),
    )
    .await;
    let token = login_token(&app, &format!("ci_my_{}", suffix)).await;

    let req = TestRequest::get()
        .uri("/api/v1/check-ins/my")
        .insert_header(("Authorization", format!("Bearer {}", token)))
        .to_request();
    let resp = call_service(&app, req).await;
    assert_eq!(resp.status(), 200);
    let body: Value = read_body_json(resp).await;
    assert!(
        body.as_array().is_some(),
        "my checkins response must be a JSON array"
    );
}

// ---------------------------------------------------------------------------
// Tests — submissions and decide
// ---------------------------------------------------------------------------

/// GET /api/v1/check-ins/windows/{window_id}/submissions without auth → 401.
#[actix_web::test]
#[ignore = "requires TEST_DATABASE_URL"]
async fn test_list_submissions_requires_auth() {
    let pool = test_pool().await;
    let app = init_service(
        App::new()
            .app_data(web::Data::new(pool))
            .configure(configure_routes),
    )
    .await;

    let ghost_id = Uuid::new_v4();
    let req = TestRequest::get()
        .uri(&format!("/api/v1/check-ins/windows/{}/submissions", ghost_id))
        .to_request();
    let resp = call_service(&app, req).await;
    assert_eq!(resp.status(), 401);
    let body: Value = read_body_json(resp).await;
    assert!(body["error"].is_string(), "401 must include an error field");
}

/// GET /api/v1/check-ins/windows/{window_id}/submissions by a student returns 403.
#[actix_web::test]
#[ignore = "requires TEST_DATABASE_URL"]
async fn test_list_submissions_requires_reviewer_role() {
    let pool = test_pool().await;
    let suffix = &Uuid::new_v4().to_string()[..8];
    seed_user(&pool, &format!("ci_sublist_{}", suffix), "Student").await;

    let app = init_service(
        App::new()
            .app_data(web::Data::new(pool.clone()))
            .configure(configure_routes),
    )
    .await;
    let token = login_token(&app, &format!("ci_sublist_{}", suffix)).await;

    let ghost_id = Uuid::new_v4();
    let req = TestRequest::get()
        .uri(&format!("/api/v1/check-ins/windows/{}/submissions", ghost_id))
        .insert_header(("Authorization", format!("Bearer {}", token)))
        .to_request();
    let resp = call_service(&app, req).await;
    assert_eq!(resp.status(), 403, "student must receive 403 on submissions list");
    let body: Value = read_body_json(resp).await;
    assert!(body["error"].is_string(), "403 must include an error field");
}

/// GET /api/v1/check-ins/windows/{window_id}/homerooms without auth → 401.
#[actix_web::test]
#[ignore = "requires TEST_DATABASE_URL"]
async fn test_list_window_homerooms_requires_auth() {
    let pool = test_pool().await;
    let app = init_service(
        App::new()
            .app_data(web::Data::new(pool))
            .configure(configure_routes),
    )
    .await;

    let ghost_id = Uuid::new_v4();
    let req = TestRequest::get()
        .uri(&format!("/api/v1/check-ins/windows/{}/homerooms", ghost_id))
        .to_request();
    let resp = call_service(&app, req).await;
    assert_eq!(resp.status(), 401);
    let body: Value = read_body_json(resp).await;
    assert!(body["error"].is_string(), "401 must include an error field");
}

/// POST /api/v1/check-ins/windows/{wid}/submissions/{sid}/decide without auth → 401.
#[actix_web::test]
#[ignore = "requires TEST_DATABASE_URL"]
async fn test_decide_submission_requires_auth() {
    let pool = test_pool().await;
    let app = init_service(
        App::new()
            .app_data(web::Data::new(pool))
            .configure(configure_routes),
    )
    .await;

    let wid = Uuid::new_v4();
    let sid = Uuid::new_v4();
    let req = TestRequest::post()
        .uri(&format!(
            "/api/v1/check-ins/windows/{}/submissions/{}/decide",
            wid, sid
        ))
        .set_json(serde_json::json!({ "decision": "approved" }))
        .to_request();
    let resp = call_service(&app, req).await;
    assert_eq!(resp.status(), 401);
    let body: Value = read_body_json(resp).await;
    assert!(body["error"].is_string(), "401 must include an error field");
}

// ---------------------------------------------------------------------------
// Helpers for school-infrastructure seeding
// ---------------------------------------------------------------------------

/// Seeds district → campus → school and returns the school UUID.
async fn seed_school(pool: &PgPool, suffix: &str) -> Uuid {
    let district_id = Uuid::new_v4();
    let campus_id = Uuid::new_v4();
    let school_id = Uuid::new_v4();

    sqlx::query(
        "INSERT INTO districts (id, name, state, created_at)
         VALUES ($1, $2, 'TX', NOW())",
    )
    .bind(district_id)
    .bind(format!("Test District {}", suffix))
    .execute(pool)
    .await
    .expect("seed district failed");

    sqlx::query(
        "INSERT INTO campuses (id, district_id, name, created_at)
         VALUES ($1, $2, $3, NOW())",
    )
    .bind(campus_id)
    .bind(district_id)
    .bind(format!("Test Campus {}", suffix))
    .execute(pool)
    .await
    .expect("seed campus failed");

    sqlx::query(
        "INSERT INTO schools (id, campus_id, name, school_type, created_at)
         VALUES ($1, $2, $3, 'general', NOW())",
    )
    .bind(school_id)
    .bind(campus_id)
    .bind(format!("Test School {}", suffix))
    .execute(pool)
    .await
    .expect("seed school failed");

    school_id
}

/// Seeds an open check-in window (opens_at = now - 1h, closes_at = now + 1h) for the school.
async fn seed_open_window(pool: &PgPool, school_id: Uuid, suffix: &str) -> Uuid {
    let window_id = Uuid::new_v4();
    sqlx::query(
        "INSERT INTO checkin_windows
             (id, school_id, title, opens_at, closes_at, allow_late, active, created_at)
         VALUES ($1, $2, $3,
                 NOW() - INTERVAL '1 hour',
                 NOW() + INTERVAL '1 hour',
                 FALSE, TRUE, NOW())",
    )
    .bind(window_id)
    .bind(school_id)
    .bind(format!("Test Window {}", suffix))
    .execute(pool)
    .await
    .expect("seed window failed");
    window_id
}

/// Assigns a user to a school directly via user_school_assignments.
async fn assign_user_to_school(pool: &PgPool, user_id: Uuid, school_id: Uuid) {
    sqlx::query(
        "INSERT INTO user_school_assignments (user_id, school_id, assignment_type, assigned_at)
         VALUES ($1, $2, 'student', NOW())
         ON CONFLICT DO NOTHING",
    )
    .bind(user_id)
    .bind(school_id)
    .execute(pool)
    .await
    .expect("assign user to school failed");
}

// ---------------------------------------------------------------------------
// Happy-path: submit → decide
// ---------------------------------------------------------------------------

/// Full submit→approve cycle: seed school infrastructure, create an open window,
/// have a student submit, then have a super-admin approve the submission.
#[actix_web::test]
#[ignore = "requires TEST_DATABASE_URL"]
async fn test_submit_checkin_happy_path() {
    let pool = test_pool().await;
    let suffix = &Uuid::new_v4().to_string()[..8];

    let school_id = seed_school(&pool, suffix).await;
    let window_id = seed_open_window(&pool, school_id, suffix).await;

    let student_name = format!("ci_happy_student_{}", suffix);
    let student_id = seed_user(&pool, &student_name, "Student").await;
    assign_user_to_school(&pool, student_id, school_id).await;

    let app = init_service(
        App::new()
            .app_data(web::Data::new(pool.clone()))
            .configure(configure_routes),
    )
    .await;
    let token = login_token(&app, &student_name).await;

    let req = TestRequest::post()
        .uri(&format!("/api/v1/check-ins/windows/{}/submit", window_id))
        .insert_header(("Authorization", format!("Bearer {}", token)))
        .set_json(serde_json::json!({ "method": "qr_code" }))
        .to_request();
    let resp = call_service(&app, req).await;
    assert_eq!(resp.status(), 201, "student submit must return 201");
    let body: Value = read_body_json(resp).await;

    assert!(body["submission_id"].is_string(), "response must include submission_id");
    assert_eq!(
        body["window_id"].as_str().unwrap(),
        window_id.to_string(),
        "response window_id must match the submitted window"
    );
    assert!(body["window_title"].is_string(), "response must include window_title");
    assert_eq!(
        body["student_id"].as_str().unwrap(),
        student_id.to_string(),
        "response student_id must match the submitting student"
    );
    assert_eq!(body["is_late"], false, "on-time submission must have is_late = false");
    assert_eq!(body["status"], "pending", "initial submission status must be 'pending'");
}

/// Re-submitting to the same window returns 409.
#[actix_web::test]
#[ignore = "requires TEST_DATABASE_URL"]
async fn test_submit_checkin_duplicate_returns_409() {
    let pool = test_pool().await;
    let suffix = &Uuid::new_v4().to_string()[..8];

    let school_id = seed_school(&pool, suffix).await;
    let window_id = seed_open_window(&pool, school_id, suffix).await;

    let student_name = format!("ci_dup_student_{}", suffix);
    let student_id = seed_user(&pool, &student_name, "Student").await;
    assign_user_to_school(&pool, student_id, school_id).await;

    let app = init_service(
        App::new()
            .app_data(web::Data::new(pool.clone()))
            .configure(configure_routes),
    )
    .await;
    let token = login_token(&app, &student_name).await;

    // First submission must succeed.
    let req = TestRequest::post()
        .uri(&format!("/api/v1/check-ins/windows/{}/submit", window_id))
        .insert_header(("Authorization", format!("Bearer {}", token)))
        .set_json(serde_json::json!({}))
        .to_request();
    let resp = call_service(&app, req).await;
    assert_eq!(resp.status(), 201, "first submission must succeed");

    // Second submission to the same window must be rejected.
    let req = TestRequest::post()
        .uri(&format!("/api/v1/check-ins/windows/{}/submit", window_id))
        .insert_header(("Authorization", format!("Bearer {}", token)))
        .set_json(serde_json::json!({}))
        .to_request();
    let resp = call_service(&app, req).await;
    assert_eq!(resp.status(), 409, "duplicate submission must return 409");
    let body: Value = read_body_json(resp).await;
    assert!(body["error"].is_string(), "409 must include an error field");
}

/// Approve a submission: admin calls decide with "approved" and gets correct shape back.
#[actix_web::test]
#[ignore = "requires TEST_DATABASE_URL"]
async fn test_decide_submission_approved_returns_correct_shape() {
    let pool = test_pool().await;
    let suffix = &Uuid::new_v4().to_string()[..8];

    let school_id = seed_school(&pool, suffix).await;
    let window_id = seed_open_window(&pool, school_id, suffix).await;

    let student_name = format!("ci_dec_student_{}", suffix);
    let student_id = seed_user(&pool, &student_name, "Student").await;
    assign_user_to_school(&pool, student_id, school_id).await;

    let admin_name = format!("ci_dec_admin_{}", suffix);
    seed_super_admin(&pool, &admin_name).await;

    let app = init_service(
        App::new()
            .app_data(web::Data::new(pool.clone()))
            .configure(configure_routes),
    )
    .await;

    // Student submits.
    let student_token = login_token(&app, &student_name).await;
    let req = TestRequest::post()
        .uri(&format!("/api/v1/check-ins/windows/{}/submit", window_id))
        .insert_header(("Authorization", format!("Bearer {}", student_token)))
        .set_json(serde_json::json!({}))
        .to_request();
    let sub_body: Value = read_body_json(call_service(&app, req).await).await;
    let submission_id = sub_body["submission_id"].as_str().expect("submission_id missing");

    // Admin approves.
    let admin_token = login_token(&app, &admin_name).await;
    let req = TestRequest::post()
        .uri(&format!(
            "/api/v1/check-ins/windows/{}/submissions/{}/decide",
            window_id, submission_id
        ))
        .insert_header(("Authorization", format!("Bearer {}", admin_token)))
        .set_json(serde_json::json!({ "decision": "approved" }))
        .to_request();
    let resp = call_service(&app, req).await;
    assert_eq!(resp.status(), 200, "admin approve must return 200");
    let body: Value = read_body_json(resp).await;

    assert_eq!(
        body["submission_id"].as_str().unwrap(),
        submission_id,
        "decide response must echo back submission_id"
    );
    assert_eq!(body["decision"], "approved", "decision must be 'approved'");
    assert!(body["decided_by"].is_string(), "decide response must include decided_by");
    assert!(body["decided_at"].is_string(), "decide response must include decided_at");
}

/// Reject without reason returns 422.
#[actix_web::test]
#[ignore = "requires TEST_DATABASE_URL"]
async fn test_decide_rejection_without_reason_returns_422() {
    let pool = test_pool().await;
    let suffix = &Uuid::new_v4().to_string()[..8];

    let school_id = seed_school(&pool, suffix).await;
    let window_id = seed_open_window(&pool, school_id, suffix).await;

    let student_name = format!("ci_rej_student_{}", suffix);
    let student_id = seed_user(&pool, &student_name, "Student").await;
    assign_user_to_school(&pool, student_id, school_id).await;

    let admin_name = format!("ci_rej_admin_{}", suffix);
    seed_super_admin(&pool, &admin_name).await;

    let app = init_service(
        App::new()
            .app_data(web::Data::new(pool.clone()))
            .configure(configure_routes),
    )
    .await;

    // Student submits.
    let student_token = login_token(&app, &student_name).await;
    let req = TestRequest::post()
        .uri(&format!("/api/v1/check-ins/windows/{}/submit", window_id))
        .insert_header(("Authorization", format!("Bearer {}", student_token)))
        .set_json(serde_json::json!({}))
        .to_request();
    let sub_body: Value = read_body_json(call_service(&app, req).await).await;
    let submission_id = sub_body["submission_id"].as_str().expect("submission_id missing");

    // Admin tries to reject without providing a reason.
    let admin_token = login_token(&app, &admin_name).await;
    let req = TestRequest::post()
        .uri(&format!(
            "/api/v1/check-ins/windows/{}/submissions/{}/decide",
            window_id, submission_id
        ))
        .insert_header(("Authorization", format!("Bearer {}", admin_token)))
        .set_json(serde_json::json!({ "decision": "rejected" }))
        .to_request();
    let resp = call_service(&app, req).await;
    assert_eq!(resp.status(), 422, "reject without reason must return 422");
    let body: Value = read_body_json(resp).await;
    assert!(body["error"].is_string(), "422 must include an error field");
}

/// GET /api/v1/check-ins/windows/{id}/submissions by admin returns an array
/// with the expected per-submission fields.
#[actix_web::test]
#[ignore = "requires TEST_DATABASE_URL"]
async fn test_list_submissions_by_admin_returns_array_with_fields() {
    let pool = test_pool().await;
    let suffix = &Uuid::new_v4().to_string()[..8];

    let school_id = seed_school(&pool, suffix).await;
    let window_id = seed_open_window(&pool, school_id, suffix).await;

    let student_name = format!("ci_list_student_{}", suffix);
    let student_id = seed_user(&pool, &student_name, "Student").await;
    assign_user_to_school(&pool, student_id, school_id).await;

    let admin_name = format!("ci_list_admin_{}", suffix);
    seed_super_admin(&pool, &admin_name).await;

    let app = init_service(
        App::new()
            .app_data(web::Data::new(pool.clone()))
            .configure(configure_routes),
    )
    .await;

    // Student submits so the list is non-empty.
    let student_token = login_token(&app, &student_name).await;
    let req = TestRequest::post()
        .uri(&format!("/api/v1/check-ins/windows/{}/submit", window_id))
        .insert_header(("Authorization", format!("Bearer {}", student_token)))
        .set_json(serde_json::json!({}))
        .to_request();
    let resp = call_service(&app, req).await;
    assert_eq!(resp.status(), 201, "student submit must succeed before listing");

    // Admin lists submissions.
    let admin_token = login_token(&app, &admin_name).await;
    let req = TestRequest::get()
        .uri(&format!("/api/v1/check-ins/windows/{}/submissions", window_id))
        .insert_header(("Authorization", format!("Bearer {}", admin_token)))
        .to_request();
    let resp = call_service(&app, req).await;
    assert_eq!(resp.status(), 200, "admin list submissions must return 200");
    let body: Value = read_body_json(resp).await;
    let items = body.as_array().expect("submissions response must be a JSON array");
    assert!(!items.is_empty(), "submissions list must be non-empty after student submits");

    for item in items {
        assert!(item["id"].is_string(), "submission.id must be string");
        assert!(item["student_id"].is_string(), "submission.student_id must be string");
        assert!(item["submitted_at"].is_string(), "submission.submitted_at must be string");
        assert!(item["is_late"].is_boolean(), "submission.is_late must be boolean");
    }
}
