/// User notification preference management.
///
/// GET  /api/v1/preferences   → fetch current user's preferences (defaults if unset)
/// PATCH /api/v1/preferences  → upsert preferences (partial update supported)
use actix_web::{web, HttpResponse};
use chrono::NaiveTime;
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::db::DbPool;
use crate::errors::AppError;
use crate::middleware::auth::AuthContext;
use crate::services::notifications::get_preferences;

// ---------------------------------------------------------------------------
// Route configuration
// ---------------------------------------------------------------------------

pub fn configure(cfg: &mut web::ServiceConfig) {
    cfg.service(
        web::scope("/preferences")
            .route("", web::get().to(get_prefs))
            .route("", web::patch().to(patch_prefs)),
    );
}

// ---------------------------------------------------------------------------
// Types
// ---------------------------------------------------------------------------

/// Wire-format for preference responses (times as "HH:MM" strings for clarity).
#[derive(Serialize)]
struct PrefsResponse {
    notif_checkin: bool,
    notif_order: bool,
    notif_general: bool,
    dnd_enabled: bool,
    dnd_start: String,
    dnd_end: String,
    inbox_frequency: String,
}

/// All fields optional — PATCH applies only what is supplied.
#[derive(Deserialize)]
struct PatchPrefsBody {
    notif_checkin: Option<bool>,
    notif_order: Option<bool>,
    notif_general: Option<bool>,
    dnd_enabled: Option<bool>,
    /// "HH:MM" format, e.g. "21:00"
    dnd_start: Option<String>,
    /// "HH:MM" format, e.g. "06:00"
    dnd_end: Option<String>,
    /// "immediate" | "daily" | "weekly"
    inbox_frequency: Option<String>,
}

// ---------------------------------------------------------------------------
// Handlers
// ---------------------------------------------------------------------------

/// GET /api/v1/preferences
async fn get_prefs(
    pool: web::Data<DbPool>,
    auth: AuthContext,
) -> Result<HttpResponse, AppError> {
    let prefs = get_preferences(pool.get_ref(), auth.0.user_id).await?;
    Ok(HttpResponse::Ok().json(to_response(prefs)))
}

/// PATCH /api/v1/preferences
async fn patch_prefs(
    pool: web::Data<DbPool>,
    auth: AuthContext,
    body: web::Json<PatchPrefsBody>,
) -> Result<HttpResponse, AppError> {
    // Validate inbox_frequency value if supplied.
    if let Some(ref freq) = body.inbox_frequency {
        if !matches!(freq.as_str(), "immediate" | "daily" | "weekly") {
            return Err(AppError::ValidationError(
                "inbox_frequency must be 'immediate', 'daily', or 'weekly'.".into(),
            ));
        }
    }

    // Parse optional time strings.
    let dnd_start: Option<NaiveTime> = body
        .dnd_start
        .as_deref()
        .map(|s| {
            NaiveTime::parse_from_str(s, "%H:%M")
                .map_err(|_| AppError::ValidationError("dnd_start must be in HH:MM format.".into()))
        })
        .transpose()?;

    let dnd_end: Option<NaiveTime> = body
        .dnd_end
        .as_deref()
        .map(|s| {
            NaiveTime::parse_from_str(s, "%H:%M")
                .map_err(|_| AppError::ValidationError("dnd_end must be in HH:MM format.".into()))
        })
        .transpose()?;

    // Load current prefs so we can do a partial update.
    let current = get_preferences(pool.get_ref(), auth.0.user_id).await?;

    let new_notif_checkin = body.notif_checkin.unwrap_or(current.notif_checkin);
    let new_notif_order = body.notif_order.unwrap_or(current.notif_order);
    let new_notif_general = body.notif_general.unwrap_or(current.notif_general);
    let new_dnd_enabled = body.dnd_enabled.unwrap_or(current.dnd_enabled);
    let new_dnd_start = dnd_start.unwrap_or(current.dnd_start);
    let new_dnd_end = dnd_end.unwrap_or(current.dnd_end);
    let new_freq = body
        .inbox_frequency
        .as_deref()
        .unwrap_or(&current.inbox_frequency)
        .to_string();

    // Upsert — ON CONFLICT means we handle both first-save and update in one query.
    sqlx::query(
        "INSERT INTO user_preferences
         (user_id, notif_checkin, notif_order, notif_general,
          dnd_enabled, dnd_start, dnd_end, inbox_frequency, updated_at)
         VALUES ($1, $2, $3, $4, $5, $6, $7, $8, NOW())
         ON CONFLICT (user_id) DO UPDATE SET
           notif_checkin   = EXCLUDED.notif_checkin,
           notif_order     = EXCLUDED.notif_order,
           notif_general   = EXCLUDED.notif_general,
           dnd_enabled     = EXCLUDED.dnd_enabled,
           dnd_start       = EXCLUDED.dnd_start,
           dnd_end         = EXCLUDED.dnd_end,
           inbox_frequency = EXCLUDED.inbox_frequency,
           updated_at      = NOW()",
    )
    .bind(auth.0.user_id)
    .bind(new_notif_checkin)
    .bind(new_notif_order)
    .bind(new_notif_general)
    .bind(new_dnd_enabled)
    .bind(new_dnd_start)
    .bind(new_dnd_end)
    .bind(&new_freq)
    .execute(pool.get_ref())
    .await?;

    log::info!("preferences updated for user {}", auth.0.user_id);

    Ok(HttpResponse::Ok().json(PrefsResponse {
        notif_checkin: new_notif_checkin,
        notif_order: new_notif_order,
        notif_general: new_notif_general,
        dnd_enabled: new_dnd_enabled,
        dnd_start: fmt_time(new_dnd_start),
        dnd_end: fmt_time(new_dnd_end),
        inbox_frequency: new_freq,
    }))
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

fn to_response(prefs: crate::services::notifications::UserPreferences) -> PrefsResponse {
    PrefsResponse {
        notif_checkin: prefs.notif_checkin,
        notif_order: prefs.notif_order,
        notif_general: prefs.notif_general,
        dnd_enabled: prefs.dnd_enabled,
        dnd_start: fmt_time(prefs.dnd_start),
        dnd_end: fmt_time(prefs.dnd_end),
        inbox_frequency: prefs.inbox_frequency,
    }
}

fn fmt_time(t: NaiveTime) -> String {
    t.format("%H:%M").to_string()
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

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
        sqlx::migrate!("../migrations").run(&pool).await.expect("migration failed");
        pool
    }

    async fn seed_and_login(pool: &PgPool, username: &str, role: &str) -> String {
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
        .execute(pool).await.expect("seed user failed");

        let role_id: i32 = sqlx::query_scalar("SELECT id FROM roles WHERE name = $1")
            .bind(role).fetch_one(pool).await.expect("role not found");
        let uid: uuid::Uuid = sqlx::query_scalar("SELECT id FROM users WHERE username = $1")
            .bind(username).fetch_one(pool).await.unwrap();
        sqlx::query("INSERT INTO user_roles (user_id, role_id) VALUES ($1, $2) ON CONFLICT DO NOTHING")
            .bind(uid).bind(role_id).execute(pool).await.unwrap();

        // Return a fresh token by logging in via the test app.
        uid.to_string() // just return uid; we'll get token in the test
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

    // ── GET preferences returns defaults for new user ─────────────────────

    #[actix_web::test]
    #[ignore = "requires DATABASE_URL"]
    async fn test_get_preferences_defaults() {
        let pool = test_pool().await;
        let suffix = uuid::Uuid::new_v4().to_string()[..8].to_string();
        let username = format!("pref_new_{}", suffix);
        seed_and_login(&pool, &username, "Student").await;

        let app = init_service(
            App::new()
                .app_data(web::Data::new(pool.clone()))
                .configure(crate::routes::configure_routes),
        )
        .await;
        let token = login_token(&app, &username).await;

        let req = TestRequest::get()
            .uri("/api/v1/preferences")
            .insert_header(("Authorization", format!("Bearer {}", token)))
            .to_request();
        let resp = call_service(&app, req).await;
        assert_eq!(resp.status(), 200);
        let body: Value = read_body_json(resp).await;
        assert_eq!(body["inbox_frequency"], "immediate", "default frequency should be immediate");
        assert_eq!(body["dnd_enabled"], false, "DND should be disabled by default");
        assert_eq!(body["notif_checkin"], true, "check-in notifications on by default");
        assert_eq!(body["dnd_start"], "21:00");
        assert_eq!(body["dnd_end"], "06:00");
    }

    // ── PATCH preferences persists changes ────────────────────────────────

    #[actix_web::test]
    #[ignore = "requires DATABASE_URL"]
    async fn test_update_preferences_persists() {
        let pool = test_pool().await;
        let suffix = uuid::Uuid::new_v4().to_string()[..8].to_string();
        let username = format!("pref_upd_{}", suffix);
        seed_and_login(&pool, &username, "Teacher").await;

        let app = init_service(
            App::new()
                .app_data(web::Data::new(pool.clone()))
                .configure(crate::routes::configure_routes),
        )
        .await;
        let token = login_token(&app, &username).await;

        // Update preferences
        let patch = json!({
            "inbox_frequency": "daily",
            "dnd_enabled": true,
            "dnd_start": "22:00",
            "dnd_end": "07:00",
            "notif_order": false,
        });
        let req = TestRequest::patch()
            .uri("/api/v1/preferences")
            .insert_header(("Authorization", format!("Bearer {}", token)))
            .set_json(&patch)
            .to_request();
        let resp = call_service(&app, req).await;
        assert_eq!(resp.status(), 200);

        // GET should reflect changes
        let req = TestRequest::get()
            .uri("/api/v1/preferences")
            .insert_header(("Authorization", format!("Bearer {}", token)))
            .to_request();
        let resp = call_service(&app, req).await;
        let body: Value = read_body_json(resp).await;
        assert_eq!(body["inbox_frequency"], "daily");
        assert_eq!(body["dnd_enabled"], true);
        assert_eq!(body["dnd_start"], "22:00");
        assert_eq!(body["dnd_end"], "07:00");
        assert_eq!(body["notif_order"], false);
        // untouched field should retain default
        assert_eq!(body["notif_checkin"], true);
    }

    // ── Invalid frequency rejected ─────────────────────────────────────────

    #[actix_web::test]
    #[ignore = "requires DATABASE_URL"]
    async fn test_invalid_frequency_rejected() {
        let pool = test_pool().await;
        let suffix = uuid::Uuid::new_v4().to_string()[..8].to_string();
        let username = format!("pref_bad_{}", suffix);
        seed_and_login(&pool, &username, "Student").await;

        let app = init_service(
            App::new()
                .app_data(web::Data::new(pool.clone()))
                .configure(crate::routes::configure_routes),
        )
        .await;
        let token = login_token(&app, &username).await;

        let req = TestRequest::patch()
            .uri("/api/v1/preferences")
            .insert_header(("Authorization", format!("Bearer {}", token)))
            .set_json(json!({ "inbox_frequency": "hourly" }))
            .to_request();
        let resp = call_service(&app, req).await;
        assert_eq!(resp.status(), 422, "invalid frequency value must return 422");
    }

    // ── Unauthenticated GET → 401 ─────────────────────────────────────────

    #[actix_web::test]
    #[ignore = "requires DATABASE_URL"]
    async fn test_preferences_requires_auth() {
        let pool = test_pool().await;
        let app = init_service(
            App::new()
                .app_data(web::Data::new(pool.clone()))
                .configure(crate::routes::configure_routes),
        )
        .await;

        let req = TestRequest::get().uri("/api/v1/preferences").to_request();
        let resp = call_service(&app, req).await;
        assert_eq!(resp.status(), 401);
    }
}
