use actix_web::{web, HttpResponse};
use chrono::{DateTime, Utc};
use serde::Serialize;
use uuid::Uuid;

use crate::db::DbPool;
use crate::errors::AppError;
use crate::middleware::auth::AuthContext;
use crate::services::notifications::create_user_notification_with_ref;

// ---------------------------------------------------------------------------
// Route configuration
// ---------------------------------------------------------------------------

pub fn configure(cfg: &mut web::ServiceConfig) {
    cfg.service(
        web::scope("/notifications")
            // Static sub-paths registered before dynamic /{id} so actix-web
            // doesn't try to parse them as UUIDs.
            .route("/unread-count", web::get().to(unread_count))
            .route("/reminders/generate", web::post().to(generate_reminders))
            .route("", web::get().to(list_notifications))
            .route("/{id}/read", web::post().to(mark_read)),
    );
}

// ---------------------------------------------------------------------------
// Types
// ---------------------------------------------------------------------------

#[derive(sqlx::FromRow, Serialize)]
struct NotificationRow {
    id: Uuid,
    subject: String,
    body: String,
    notification_type: String,
    read_at: Option<DateTime<Utc>>,
    created_at: DateTime<Utc>,
    display_after: Option<DateTime<Utc>>,
    sender_username: Option<String>,
}

// ---------------------------------------------------------------------------
// Handlers
// ---------------------------------------------------------------------------

/// GET /api/v1/notifications
///
/// Returns the calling user's inbox, newest first, limited to 50 items.
/// Only includes notifications that have passed their `display_after` time
/// (i.e. frequency/DND deferral has elapsed).
async fn list_notifications(
    pool: web::Data<DbPool>,
    auth: AuthContext,
) -> Result<HttpResponse, AppError> {
    let notifications = sqlx::query_as::<_, NotificationRow>(
        "SELECT
           n.id,
           n.subject,
           n.body,
           n.notification_type,
           n.read_at,
           n.created_at,
           n.display_after,
           s.username AS sender_username
         FROM notifications n
         LEFT JOIN users s ON n.sender_id = s.id
         WHERE n.recipient_id = $1
           AND (n.expires_at IS NULL OR n.expires_at > NOW())
           AND (n.display_after IS NULL OR n.display_after <= NOW())
         ORDER BY n.created_at DESC
         LIMIT 50",
    )
    .bind(auth.0.user_id)
    .fetch_all(pool.get_ref())
    .await?;

    Ok(HttpResponse::Ok().json(notifications))
}

/// GET /api/v1/notifications/unread-count
///
/// Unread count for the calling user, filtered by display_after so badges
/// reflect the same visibility rules as the inbox.
async fn unread_count(
    pool: web::Data<DbPool>,
    auth: AuthContext,
) -> Result<HttpResponse, AppError> {
    let count: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM notifications
         WHERE recipient_id = $1
           AND read_at IS NULL
           AND (expires_at IS NULL OR expires_at > NOW())
           AND (display_after IS NULL OR display_after <= NOW())",
    )
    .bind(auth.0.user_id)
    .fetch_one(pool.get_ref())
    .await?;

    Ok(HttpResponse::Ok().json(serde_json::json!({ "unread": count })))
}

/// POST /api/v1/notifications/read
///
/// Marks a notification as read. Silently succeeds if already read.
/// Returns 404 if the notification does not belong to the calling user.
async fn mark_read(
    pool: web::Data<DbPool>,
    auth: AuthContext,
    path: web::Path<Uuid>,
) -> Result<HttpResponse, AppError> {
    let notification_id = path.into_inner();

    // Verify ownership before updating.
    let owner: Option<Uuid> = sqlx::query_scalar(
        "SELECT recipient_id FROM notifications WHERE id = $1",
    )
    .bind(notification_id)
    .fetch_optional(pool.get_ref())
    .await?;

    match owner {
        None => return Err(AppError::NotFound("Notification not found".into())),
        Some(rid) if rid != auth.0.user_id => {
            return Err(AppError::Forbidden(
                "You do not own this notification.".into(),
            ))
        }
        _ => {}
    }

    sqlx::query(
        "UPDATE notifications SET read_at = NOW()
         WHERE id = $1 AND read_at IS NULL",
    )
    .bind(notification_id)
    .execute(pool.get_ref())
    .await?;

    Ok(HttpResponse::Ok().json(serde_json::json!({ "message": "Marked as read." })))
}

/// POST /api/v1/notifications/reminders/generate
///
/// Generates check-in reminders for the calling Student:
///
/// 1. Upcoming windows opening within the next 4 hours with no submission yet.
/// 2. Windows that closed in the last 2 hours with no submission (missed).
///
/// Uses `ref_key` deduplication — repeated calls within 12 hours are no-ops.
/// Non-student roles receive an empty result rather than an error.
async fn generate_reminders(
    pool: web::Data<DbPool>,
    auth: AuthContext,
) -> Result<HttpResponse, AppError> {
    let is_student = auth.0.roles.iter().any(|r| r == "Student");
    let is_parent = auth.0.roles.iter().any(|r| r == "Parent");
    if !is_student && !is_parent {
        return Ok(HttpResponse::Ok().json(serde_json::json!({ "generated": 0 })));
    }

    #[derive(sqlx::FromRow)]
    struct ReminderTarget {
        student_id: Uuid,
        student_username: String,
    }

    let targets: Vec<(Uuid, ReminderTarget)> = if is_student {
        let username: String = sqlx::query_scalar("SELECT username FROM users WHERE id = $1")
            .bind(auth.0.user_id)
            .fetch_one(pool.get_ref())
            .await?;
        vec![(
            auth.0.user_id,
            ReminderTarget {
                student_id: auth.0.user_id,
                student_username: username,
            },
        )]
    } else {
        let linked: Vec<ReminderTarget> = sqlx::query_as(
            "SELECT u.id AS student_id, u.username AS student_username
             FROM parent_student_links psl
             JOIN users u ON u.id = psl.student_id
             WHERE psl.parent_id = $1
             ORDER BY u.username ASC",
        )
        .bind(auth.0.user_id)
        .fetch_all(pool.get_ref())
        .await?;

        linked
            .into_iter()
            .map(|student| (auth.0.user_id, student))
            .collect()
    };

    // ── 1. Upcoming windows (open within 4 h, not yet submitted) ─────────
    #[derive(sqlx::FromRow)]
    struct WindowBrief {
        id: Uuid,
        title: String,
    }

    let mut generated: u32 = 0;

    for (recipient_id, target) in targets {
        let upcoming: Vec<WindowBrief> = sqlx::query_as::<_, WindowBrief>(
            "SELECT cw.id, cw.title
             FROM checkin_windows cw
             WHERE cw.active = true
               AND cw.opens_at > NOW()
               AND cw.opens_at <= NOW() + INTERVAL '4 hours'
               AND cw.school_id IN (
                 SELECT DISTINCT c.school_id
                 FROM class_enrollments ce
                 JOIN classes c ON ce.class_id = c.id
                 WHERE ce.student_id = $1 AND ce.status = 'active'
               )
               AND NOT EXISTS (
                 SELECT 1 FROM checkin_submissions cs
                 WHERE cs.window_id = cw.id AND cs.student_id = $1
               )",
        )
        .bind(target.student_id)
        .fetch_all(pool.get_ref())
        .await?;

        let missed: Vec<WindowBrief> = sqlx::query_as::<_, WindowBrief>(
            "SELECT cw.id, cw.title
             FROM checkin_windows cw
             WHERE cw.active = true
               AND cw.closes_at < NOW()
               AND cw.closes_at > NOW() - INTERVAL '2 hours'
               AND cw.allow_late = false
               AND cw.school_id IN (
                 SELECT DISTINCT c.school_id
                 FROM class_enrollments ce
                 JOIN classes c ON ce.class_id = c.id
                 WHERE ce.student_id = $1 AND ce.status = 'active'
               )
               AND NOT EXISTS (
                 SELECT 1 FROM checkin_submissions cs
                 WHERE cs.window_id = cw.id AND cs.student_id = $1
               )",
        )
        .bind(target.student_id)
        .fetch_all(pool.get_ref())
        .await?;

        let is_self = recipient_id == target.student_id;
        let subject_prefix = if is_self {
            "Upcoming check-in".to_string()
        } else {
            format!("Upcoming check-in for {}", target.student_username)
        };
        let missed_prefix = if is_self {
            "Missed check-in".to_string()
        } else {
            format!("Missed check-in for {}", target.student_username)
        };

        for w in upcoming {
            let ref_key = format!("reminder:upcoming:{}:{}:{}", w.id, recipient_id, target.student_id);
            let body = if is_self {
                format!(
                    "Your check-in window '{}' opens in the next 4 hours. Don't forget to check in.",
                    w.title
                )
            } else {
                format!(
                    "{} has a check-in window '{}' opening in the next 4 hours.",
                    target.student_username, w.title
                )
            };

            create_user_notification_with_ref(
                pool.get_ref(),
                recipient_id,
                None,
                &format!("{}: {}", subject_prefix, w.title),
                &body,
                "checkin",
                Some(&ref_key),
            )
            .await?;
            generated += 1;
        }

        for w in missed {
            let ref_key = format!("reminder:missed:{}:{}:{}", w.id, recipient_id, target.student_id);
            let body = if is_self {
                format!(
                    "You did not check in for '{}' and the window has closed.",
                    w.title
                )
            } else {
                format!(
                    "{} did not check in for '{}' and the window has closed.",
                    target.student_username, w.title
                )
            };

            create_user_notification_with_ref(
                pool.get_ref(),
                recipient_id,
                None,
                &format!("{}: {}", missed_prefix, w.title),
                &body,
                "checkin",
                Some(&ref_key),
            )
            .await?;
            generated += 1;
        }
    }

    log::info!(
        "generated {} reminders for user {}",
        generated,
        auth.0.user_id
    );

    Ok(HttpResponse::Ok().json(serde_json::json!({ "generated": generated })))
}

#[cfg(test)]
mod tests {
    use super::*;
    use actix_web::test::{call_service, init_service, read_body_json, TestRequest};
    use actix_web::{web, App};
    use serde_json::{json, Value};
    use sqlx::postgres::PgPoolOptions;
    use sqlx::PgPool;

    async fn test_pool() -> PgPool {
        let url = std::env::var("DATABASE_URL")
            .expect("DATABASE_URL must be set to run integration tests");
        let pool = PgPoolOptions::new()
            .max_connections(3)
            .connect(&url)
            .await
            .expect("Failed to connect to test database");
        sqlx::migrate!("../migrations")
            .run(&pool)
            .await
            .expect("migration failed");
        pool
    }

    async fn seed_user_with_role(pool: &PgPool, username: &str, role: &str) {
        let hash =
            crate::services::auth::hash_password("TestPass2024!!").expect("hash failed");

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

        sqlx::query(
            "INSERT INTO user_roles (user_id, role_id) VALUES ($1, $2) ON CONFLICT DO NOTHING",
        )
        .bind(uid)
        .bind(role_id)
        .execute(pool)
        .await
        .expect("assign role failed");
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
            .set_json(json!({"username": username, "password": "TestPass2024!!"}))
            .to_request();
        let resp = call_service(app, req).await;
        let body: Value = read_body_json(resp).await;
        body["token"].as_str().unwrap().to_string()
    }

    #[actix_web::test]
    #[ignore = "requires DATABASE_URL"]
    async fn test_list_notifications_returns_only_callers_notifications() {
        let pool = test_pool().await;
        let suffix = Uuid::new_v4().to_string()[..8].to_string();
        let username = format!("notif_user_{}", suffix);
        let other_username = format!("notif_other_{}", suffix);

        seed_user_with_role(&pool, &username, "Student").await;
        seed_user_with_role(&pool, &other_username, "Student").await;

        let user_id: Uuid = sqlx::query_scalar("SELECT id FROM users WHERE username = $1")
            .bind(&username)
            .fetch_one(&pool)
            .await
            .unwrap();
        let other_id: Uuid = sqlx::query_scalar("SELECT id FROM users WHERE username = $1")
            .bind(&other_username)
            .fetch_one(&pool)
            .await
            .unwrap();

        sqlx::query(
            "INSERT INTO notifications (id, recipient_id, subject, body, notification_type, created_at)
             VALUES ($1, $2, $3, $4, 'checkin', NOW()),
                    ($5, $6, $7, $8, 'checkin', NOW())",
        )
        .bind(Uuid::new_v4())
        .bind(user_id)
        .bind("Approved")
        .bind("Your check-in was approved.")
        .bind(Uuid::new_v4())
        .bind(other_id)
        .bind("Other")
        .bind("This should not be visible.")
        .execute(&pool)
        .await
        .unwrap();

        let app = init_service(
            App::new()
                .app_data(web::Data::new(pool.clone()))
                .configure(crate::routes::configure_routes),
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
        let items = body.as_array().expect("notifications array");
        assert_eq!(items.len(), 1, "caller should only receive owned notifications");
        assert_eq!(items[0]["subject"], "Approved");
    }

    #[actix_web::test]
    #[ignore = "requires DATABASE_URL"]
    async fn test_mark_read_rejects_foreign_notification() {
        let pool = test_pool().await;
        let suffix = Uuid::new_v4().to_string()[..8].to_string();
        let username = format!("notif_mark_{}", suffix);
        let other_username = format!("notif_owner_{}", suffix);

        seed_user_with_role(&pool, &username, "Student").await;
        seed_user_with_role(&pool, &other_username, "Student").await;

        let other_id: Uuid = sqlx::query_scalar("SELECT id FROM users WHERE username = $1")
            .bind(&other_username)
            .fetch_one(&pool)
            .await
            .unwrap();
        let notification_id = Uuid::new_v4();

        sqlx::query(
            "INSERT INTO notifications (id, recipient_id, subject, body, notification_type, created_at)
             VALUES ($1, $2, $3, $4, 'checkin', NOW())",
        )
        .bind(notification_id)
        .bind(other_id)
        .bind("Denied")
        .bind("Reason: missing note")
        .execute(&pool)
        .await
        .unwrap();

        let app = init_service(
            App::new()
                .app_data(web::Data::new(pool.clone()))
                .configure(crate::routes::configure_routes),
        )
        .await;
        let token = login_token(&app, &username).await;

        let req = TestRequest::post()
            .uri(&format!("/api/v1/notifications/{}/read", notification_id))
            .insert_header(("Authorization", format!("Bearer {}", token)))
            .to_request();
        let resp = call_service(&app, req).await;
        assert_eq!(resp.status(), 403, "foreign notification must be forbidden");
    }
}
