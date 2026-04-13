use actix_web::test::{call_service, init_service, read_body_json, TestRequest};
use actix_web::{web, App};
use meridian_backend::routes::configure_routes;
use meridian_backend::services::auth::hash_password;
use sqlx::postgres::PgPoolOptions;
use sqlx::PgPool;
use uuid::Uuid;

async fn test_pool() -> PgPool {
    let url = std::env::var("TEST_DATABASE_URL")
        .expect("TEST_DATABASE_URL must be set to run e2e_workflow_tests");
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
    let body: serde_json::Value = read_body_json(resp).await;
    body["token"].as_str().expect("token missing").to_string()
}

#[actix_web::test]
#[ignore = "requires TEST_DATABASE_URL"]
async fn test_student_deletion_request_roundtrip_disables_future_login() {
    let pool = test_pool().await;
    let suffix = &Uuid::new_v4().to_string()[..8];
    let student_name = format!("e2e_student_{}", suffix);
    let admin_name = format!("e2e_admin_{}", suffix);

    seed_user(&pool, &student_name, "Student").await;
    let admin_id = seed_user(&pool, &admin_name, "Administrator").await;
    make_super_admin(&pool, admin_id).await;

    let app = init_service(
        App::new()
            .app_data(web::Data::new(pool.clone()))
            .configure(configure_routes),
    )
    .await;

    let student_token = login_token(&app, &student_name).await;
    let req = TestRequest::post()
        .uri("/api/v1/users/me/request-deletion")
        .insert_header(("Authorization", format!("Bearer {}", student_token)))
        .set_json(serde_json::json!({"reason": "Please remove my account"}))
        .to_request();
    let resp = call_service(&app, req).await;
    assert_eq!(resp.status(), 201, "student should be able to request deletion");

    let admin_token = login_token(&app, &admin_name).await;
    let req = TestRequest::get()
        .uri("/api/v1/admin/deletion-requests")
        .insert_header(("Authorization", format!("Bearer {}", admin_token)))
        .to_request();
    let resp = call_service(&app, req).await;
    assert_eq!(resp.status(), 200);
    let body: serde_json::Value = read_body_json(resp).await;
    let request_id = body
        .as_array()
        .and_then(|rows| {
            rows.iter().find_map(|row| {
                (row["username"].as_str() == Some(student_name.as_str()))
                    .then(|| row["id"].as_str().map(str::to_string))
                    .flatten()
            })
        })
        .expect("deletion request missing");

    let req = TestRequest::post()
        .uri(&format!("/api/v1/admin/deletion-requests/{}/approve", request_id))
        .insert_header(("Authorization", format!("Bearer {}", admin_token)))
        .to_request();
    let resp = call_service(&app, req).await;
    assert_eq!(resp.status(), 200, "admin should approve deletion");

    let req = TestRequest::post()
        .uri("/api/v1/auth/login")
        .set_json(serde_json::json!({"username": student_name, "password": "TestPass2024!!"}))
        .to_request();
    let resp = call_service(&app, req).await;
    assert_eq!(resp.status(), 403, "disabled account should not be able to log in");
}

#[actix_web::test]
#[ignore = "requires TEST_DATABASE_URL"]
async fn test_student_preferences_and_notification_read_flow() {
    let pool = test_pool().await;
    let suffix = &Uuid::new_v4().to_string()[..8];
    let student_name = format!("e2e_notify_{}", suffix);
    let student_id = seed_user(&pool, &student_name, "Student").await;

    let app = init_service(
        App::new()
            .app_data(web::Data::new(pool.clone()))
            .configure(configure_routes),
    )
    .await;

    let token = login_token(&app, &student_name).await;

    let req = TestRequest::get()
        .uri("/api/v1/preferences")
        .insert_header(("Authorization", format!("Bearer {}", token)))
        .to_request();
    let resp = call_service(&app, req).await;
    assert_eq!(resp.status(), 200);
    let body: serde_json::Value = read_body_json(resp).await;
    assert_eq!(body["inbox_frequency"], "immediate");

    let req = TestRequest::patch()
        .uri("/api/v1/preferences")
        .insert_header(("Authorization", format!("Bearer {}", token)))
        .set_json(serde_json::json!({
            "inbox_frequency": "daily",
            "dnd_enabled": true,
            "dnd_start": "22:00",
            "dnd_end": "07:00"
        }))
        .to_request();
    let resp = call_service(&app, req).await;
    assert_eq!(resp.status(), 200);

    let notification_id = Uuid::new_v4();
    sqlx::query(
        "INSERT INTO notifications (id, recipient_id, subject, body, notification_type, created_at)
         VALUES ($1, $2, $3, $4, 'general', NOW())",
    )
    .bind(notification_id)
    .bind(student_id)
    .bind("Policy Update")
    .bind("There is a new handbook update.")
    .execute(&pool)
    .await
    .expect("insert notification failed");

    let req = TestRequest::get()
        .uri("/api/v1/notifications")
        .insert_header(("Authorization", format!("Bearer {}", token)))
        .to_request();
    let resp = call_service(&app, req).await;
    assert_eq!(resp.status(), 200);
    let body: serde_json::Value = read_body_json(resp).await;
    assert!(
        body.as_array()
            .expect("notifications array")
            .iter()
            .any(|row| row["id"].as_str() == Some(notification_id.to_string().as_str())),
        "new notification should be visible to the student"
    );

    let req = TestRequest::post()
        .uri(&format!("/api/v1/notifications/{}/read", notification_id))
        .insert_header(("Authorization", format!("Bearer {}", token)))
        .to_request();
    let resp = call_service(&app, req).await;
    assert_eq!(resp.status(), 200);

    let req = TestRequest::get()
        .uri("/api/v1/notifications/unread-count")
        .insert_header(("Authorization", format!("Bearer {}", token)))
        .to_request();
    let resp = call_service(&app, req).await;
    assert_eq!(resp.status(), 200);
    let body: serde_json::Value = read_body_json(resp).await;
    assert_eq!(body["unread"], 0);
}
