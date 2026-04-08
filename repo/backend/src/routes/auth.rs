use actix_web::{web, HttpRequest, HttpResponse};
use argon2::{Argon2, PasswordHash, PasswordVerifier};
use chrono::{Duration, Utc};
use rand::Rng;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use uuid::Uuid;

use crate::db::DbPool;
use crate::errors::AppError;
use crate::middleware::auth::AuthContext;
use crate::models::user::{User, UserPublic};

// ---------------------------------------------------------------------------
// Rate limiting constants
// ---------------------------------------------------------------------------

const MAX_ATTEMPTS_PER_WINDOW: i64 = 5;
const ATTEMPT_WINDOW_MINUTES: i64 = 15;
const LOCKOUT_MINUTES: i64 = 30;

/// Valid Argon2 hash that never matches a real password, used when username is not found
/// to equalize timing between "user not found" and "wrong password".
const DUMMY_HASH: &str = "$argon2id$v=19$m=19456,t=2,p=1$c2FsdHNhbHRzYWx0c2FsdA$aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";

// ---------------------------------------------------------------------------
// Request / response types
// ---------------------------------------------------------------------------

#[derive(Debug, Deserialize)]
pub struct LoginRequest {
    pub username: String,
    pub password: String,
}

#[derive(Debug, Serialize)]
pub struct LoginResponse {
    pub token: String,
    pub user: UserPublic,
}

#[derive(Debug, Serialize)]
pub struct HealthResponse {
    pub status: &'static str,
    pub version: &'static str,
}

#[derive(Deserialize)]
struct DeletionRequestBody {
    reason: Option<String>,
}

#[derive(Deserialize)]
struct VerifyPasswordBody {
    password: String,
}

// ---------------------------------------------------------------------------
// Route configuration
// ---------------------------------------------------------------------------

pub fn configure(cfg: &mut web::ServiceConfig) {
    cfg.service(
        web::scope("/auth")
            .route("/login", web::post().to(login))
            .route("/me", web::get().to(me))
            .route("/logout", web::post().to(logout))
            .route("/verify", web::post().to(verify_password))
            .route("/request-deletion", web::post().to(request_account_deletion)),
    )
    // Top-level health check (no /auth prefix)
    .route("/health", web::get().to(health));
}

// ---------------------------------------------------------------------------
// Rate limiting helpers
// ---------------------------------------------------------------------------

async fn check_login_rate_limit(pool: &DbPool, username: &str) -> Result<(), AppError> {
    // 1. Check persisted lockout first.
    let locked_until: Option<chrono::DateTime<Utc>> = sqlx::query_scalar(
        "SELECT locked_until FROM login_lockouts \
         WHERE username = $1 AND locked_until > NOW()",
    )
    .bind(username)
    .fetch_optional(pool)
    .await?;

    if locked_until.is_some() {
        log::warn!("Login blocked for '{}': account is locked out", username);
        return Err(AppError::TooManyRequests(format!(
            "Too many failed login attempts for this account. Please wait {} minutes before trying again.",
            LOCKOUT_MINUTES
        )));
    }

    // 2. Count failures in the rolling window.
    let failed_count: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM login_attempts \
         WHERE username = $1 \
           AND success = FALSE \
           AND attempted_at > NOW() - ($2 * INTERVAL '1 minute')",
    )
    .bind(username)
    .bind(ATTEMPT_WINDOW_MINUTES)
    .fetch_one(pool)
    .await?;

    if failed_count >= MAX_ATTEMPTS_PER_WINDOW {
        // Persist a 30-minute lockout.
        sqlx::query(
            "INSERT INTO login_lockouts (username, locked_until) \
             VALUES ($1, NOW() + ($2 * INTERVAL '1 minute')) \
             ON CONFLICT (username) DO UPDATE \
               SET locked_until = NOW() + ($2 * INTERVAL '1 minute')",
        )
        .bind(username)
        .bind(LOCKOUT_MINUTES)
        .execute(pool)
        .await?;

        log::warn!(
            "Login rate limit exceeded for username '{}' ({} attempts in {} min window); locked for {} min",
            username,
            failed_count,
            ATTEMPT_WINDOW_MINUTES,
            LOCKOUT_MINUTES,
        );
        return Err(AppError::TooManyRequests(format!(
            "Too many failed login attempts for this account. Please wait {} minutes before trying again.",
            LOCKOUT_MINUTES
        )));
    }
    Ok(())
}

async fn record_login_attempt(
    pool: &DbPool,
    username: &str,
    ip_address: Option<&str>,
    success: bool,
) -> Result<(), AppError> {
    sqlx::query(
        "INSERT INTO login_attempts (id, username, ip_address, attempted_at, success) \
         VALUES ($1, $2, $3, NOW(), $4)",
    )
    .bind(Uuid::new_v4())
    .bind(username)
    .bind(ip_address)
    .bind(success)
    .execute(pool)
    .await?;
    Ok(())
}

fn dummy_verify() {
    let _ = PasswordHash::new(DUMMY_HASH)
        .map(|h| Argon2::default().verify_password(b"dummy", &h));
}

// ---------------------------------------------------------------------------
// Handlers
// ---------------------------------------------------------------------------

/// POST /api/v1/auth/login
async fn login(
    pool: web::Data<DbPool>,
    req: HttpRequest,
    body: web::Json<LoginRequest>,
) -> Result<HttpResponse, AppError> {
    let ip = extract_ip(&req);

    // 1. Basic validation
    if body.username.trim().is_empty() || body.password.trim().is_empty() {
        return Err(AppError::ValidationError(
            "username and password must not be empty".into(),
        ));
    }

    // 2. Check rate limit
    check_login_rate_limit(pool.get_ref(), &body.username).await?;

    // 3. Fetch user by username
    let user_opt = sqlx::query_as::<_, User>(
        "SELECT id, username, email, password_hash, display_name, account_state, \
         created_at, updated_at \
         FROM users WHERE username = $1",
    )
    .bind(&body.username)
    .fetch_optional(pool.get_ref())
    .await?;

    // 4. Verify password — timing equalization when user not found
    let password_ok = match &user_opt {
        Some(user) => {
            // Verify against actual hash
            match PasswordHash::new(&user.password_hash) {
                Ok(parsed_hash) => Argon2::default()
                    .verify_password(body.password.as_bytes(), &parsed_hash)
                    .is_ok(),
                Err(_) => false,
            }
        }
        None => {
            // User not found — run dummy verify for timing equalization
            dummy_verify();
            false
        }
    };

    // 5. Record login attempt
    let _ = record_login_attempt(
        pool.get_ref(),
        &body.username,
        ip.as_deref(),
        password_ok,
    )
    .await;

    // 6. If password not ok, return generic error
    if !password_ok {
        log::info!(
            "Failed login attempt for username '{}' from {:?}",
            body.username,
            ip
        );
        let _ = log_access(
            pool.get_ref(),
            None,
            "login",
            ip.as_deref(),
            false,
            Some("Invalid username or password"),
        )
        .await;
        return Err(AppError::Unauthorized("Invalid username or password".into()));
    }

    // 7. Unwrap user (password_ok = true so user exists)
    let user = user_opt.unwrap();

    // 8. Check account state
    match user.account_state.as_str() {
        "active" => {}
        "disabled" => {
            let _ = log_access(
                pool.get_ref(),
                Some(user.id),
                "login",
                ip.as_deref(),
                false,
                Some("Account disabled"),
            )
            .await;
            return Err(AppError::Forbidden(
                "Your account has been disabled. Contact your administrator.".into(),
            ));
        }
        "frozen" => {
            let _ = log_access(
                pool.get_ref(),
                Some(user.id),
                "login",
                ip.as_deref(),
                false,
                Some("Account frozen"),
            )
            .await;
            return Err(AppError::Forbidden(
                "Your account has been temporarily frozen. Contact your administrator.".into(),
            ));
        }
        "blacklisted" => {
            let _ = log_access(
                pool.get_ref(),
                Some(user.id),
                "login",
                ip.as_deref(),
                false,
                Some("Account blacklisted"),
            )
            .await;
            return Err(AppError::Forbidden(
                "Your account access has been revoked.".into(),
            ));
        }
        other => {
            let _ = log_access(
                pool.get_ref(),
                Some(user.id),
                "login",
                ip.as_deref(),
                false,
                Some(&format!("Account in non-active state: {}", other)),
            )
            .await;
            return Err(AppError::Forbidden("Account access denied.".into()));
        }
    }

    // 9. Generate session token (64 hex chars = 32 random bytes)
    let session_secret = std::env::var("SESSION_SECRET").unwrap_or_else(|_| {
        "local-dev-session-secret-fallback-64-chars-minimum-000000000000".to_string()
    });
    let token = generate_token(&session_secret);
    let session_id = Uuid::new_v4();
    let ttl_seconds = std::env::var("SESSION_MAX_AGE_SECONDS")
        .ok()
        .and_then(|v| v.parse::<i64>().ok())
        .unwrap_or(3600)
        .max(60);
    let expires_at = Utc::now() + Duration::seconds(ttl_seconds);
    let user_agent = extract_user_agent(&req);

    sqlx::query(
        "INSERT INTO sessions (id, user_id, token, created_at, expires_at, ip_address, user_agent) \
         VALUES ($1, $2, $3, NOW(), $4, $5, $6)",
    )
    .bind(session_id)
    .bind(user.id)
    .bind(&token)
    .bind(expires_at)
    .bind(ip.as_deref())
    .bind(user_agent.as_deref())
    .execute(pool.get_ref())
    .await?;

    // Log successful access
    let _ = log_access(
        pool.get_ref(),
        Some(user.id),
        "login",
        ip.as_deref(),
        true,
        None,
    )
    .await;

    // Fetch roles for response
    let roles = fetch_user_roles(pool.get_ref(), user.id).await?;

    let user_public = UserPublic {
        id: user.id,
        username: user.username.clone(),
        email: user.email.clone(),
        display_name: user.display_name.clone(),
        account_state: user.account_state.clone(),
        roles,
        created_at: user.created_at,
    };

    Ok(HttpResponse::Ok().json(LoginResponse {
        token,
        user: user_public,
    }))
}

/// GET /api/v1/auth/me
async fn me(
    pool: web::Data<DbPool>,
    auth: AuthContext,
) -> Result<HttpResponse, AppError> {
    // Fetch fresh user record from DB using auth.0.user_id
    let user = sqlx::query_as::<_, User>(
        "SELECT id, username, email, password_hash, display_name, account_state, \
         created_at, updated_at \
         FROM users WHERE id = $1",
    )
    .bind(auth.0.user_id)
    .fetch_optional(pool.get_ref())
    .await?
    .ok_or_else(|| AppError::NotFound("User not found".into()))?;

    let user_public = UserPublic {
        id: user.id,
        username: user.username,
        email: user.email,
        display_name: user.display_name,
        account_state: user.account_state,
        roles: auth.0.roles,
        created_at: user.created_at,
    };

    Ok(HttpResponse::Ok().json(user_public))
}

/// POST /api/v1/auth/logout
async fn logout(
    pool: web::Data<DbPool>,
    req: HttpRequest,
) -> Result<HttpResponse, AppError> {
    let token = extract_bearer_token_from_http(&req)?;

    sqlx::query("DELETE FROM sessions WHERE token = $1")
        .bind(&token)
        .execute(pool.get_ref())
        .await?;

    Ok(HttpResponse::Ok().json(serde_json::json!({ "message": "Logged out successfully" })))
}

/// GET /api/v1/health
async fn health() -> HttpResponse {
    HttpResponse::Ok().json(HealthResponse {
        status: "ok",
        version: "0.1.0",
    })
}

/// POST /api/v1/auth/request-deletion
async fn request_account_deletion(
    pool: web::Data<DbPool>,
    auth: AuthContext,
    body: web::Json<DeletionRequestBody>,
) -> Result<HttpResponse, AppError> {
    // Check if there's already a pending deletion request for this user
    let pending_count: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM account_deletion_requests WHERE user_id = $1 AND status = 'pending'",
    )
    .bind(auth.0.user_id)
    .fetch_one(pool.get_ref())
    .await?;

    if pending_count > 0 {
        return Err(AppError::ConflictError(
            "A deletion request is already pending.".into(),
        ));
    }

    // Insert new deletion request with status 'pending'
    sqlx::query(
        "INSERT INTO account_deletion_requests (id, user_id, reason, status, requested_at) \
         VALUES ($1, $2, $3, 'pending', NOW())",
    )
    .bind(Uuid::new_v4())
    .bind(auth.0.user_id)
    .bind(body.reason.as_deref())
    .execute(pool.get_ref())
    .await?;

    // Log audit event
    sqlx::query(
        "INSERT INTO audit_logs (id, actor_id, action, entity_type, entity_id, old_data, new_data, created_at) \
         VALUES ($1, $2, $3, $4, $5, $6, $7, NOW())",
    )
    .bind(Uuid::new_v4())
    .bind(auth.0.user_id)
    .bind("deletion_request_submitted")
    .bind("user")
    .bind(auth.0.user_id.to_string())
    .bind(Option::<serde_json::Value>::None)
    .bind(Option::<serde_json::Value>::None)
    .execute(pool.get_ref())
    .await?;

    Ok(HttpResponse::Created().json(serde_json::json!({
        "message": "Account deletion request submitted. An administrator will review it."
    })))
}

/// POST /api/v1/auth/verify
///
/// Quick-lock re-authentication: verifies the current session user's password
/// without creating a new session or invalidating the existing one.
///
/// Reuses the login rate-limiting table to prevent brute-force on locked screens.
/// Returns 200 `{"verified": true}` on success or 401 on failure.
async fn verify_password(
    pool: web::Data<DbPool>,
    auth: AuthContext,
    body: web::Json<VerifyPasswordBody>,
) -> Result<HttpResponse, AppError> {
    if body.password.trim().is_empty() {
        return Err(AppError::ValidationError("password must not be empty".into()));
    }

    // Resolve username for rate-limit key (same table as login).
    let username: String = sqlx::query_scalar("SELECT username FROM users WHERE id = $1")
        .bind(auth.0.user_id)
        .fetch_one(pool.get_ref())
        .await?;

    check_login_rate_limit(pool.get_ref(), &username).await?;

    // Fetch the stored hash.
    let hash: String =
        sqlx::query_scalar("SELECT password_hash FROM users WHERE id = $1")
            .bind(auth.0.user_id)
            .fetch_one(pool.get_ref())
            .await?;

    let ok = PasswordHash::new(&hash)
        .map(|h| {
            Argon2::default()
                .verify_password(body.password.as_bytes(), &h)
                .is_ok()
        })
        .unwrap_or(false);

    let _ = record_login_attempt(pool.get_ref(), &username, None, ok).await;

    if ok {
        let _ = log_access(
            pool.get_ref(),
            Some(auth.0.user_id),
            "quick_lock_verify",
            None,
            true,
            Some("Quick-lock password verification succeeded"),
        )
        .await;
        Ok(HttpResponse::Ok().json(serde_json::json!({ "verified": true })))
    } else {
        let _ = log_access(
            pool.get_ref(),
            Some(auth.0.user_id),
            "quick_lock_verify",
            None,
            false,
            Some("Quick-lock password verification failed"),
        )
        .await;
        Err(AppError::Unauthorized("Incorrect password.".into()))
    }
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

fn generate_token(session_secret: &str) -> String {
    let bytes: [u8; 32] = rand::thread_rng().gen();
    let mut hasher = Sha256::new();
    hasher.update(bytes);
    hasher.update(session_secret.as_bytes());
    let digest = hasher.finalize();
    digest.iter().map(|b| format!("{:02x}", b)).collect()
}

fn extract_ip(req: &HttpRequest) -> Option<String> {
    // Check X-Forwarded-For first, fall back to peer address
    req.headers()
        .get("X-Forwarded-For")
        .and_then(|v| v.to_str().ok())
        .map(|s| s.split(',').next().unwrap_or(s).trim().to_string())
        .or_else(|| req.peer_addr().map(|addr| addr.ip().to_string()))
}

fn extract_user_agent(req: &HttpRequest) -> Option<String> {
    req.headers()
        .get("User-Agent")
        .and_then(|v| v.to_str().ok())
        .map(|s| s.to_string())
}

/// Extract Bearer token from an HttpRequest (as opposed to ServiceRequest).
fn extract_bearer_token_from_http(req: &HttpRequest) -> Result<String, AppError> {
    req.headers()
        .get("Authorization")
        .and_then(|v| v.to_str().ok())
        .and_then(|v| v.strip_prefix("Bearer "))
        .map(|s| s.to_string())
        .ok_or_else(|| AppError::Unauthorized("Missing or invalid Authorization header".into()))
}

async fn fetch_user_roles(pool: &DbPool, user_id: Uuid) -> Result<Vec<String>, AppError> {
    let roles = sqlx::query_scalar::<_, String>(
        "SELECT r.name FROM roles r \
         JOIN user_roles ur ON r.id = ur.role_id \
         WHERE ur.user_id = $1",
    )
    .bind(user_id)
    .fetch_all(pool)
    .await?;
    Ok(roles)
}

async fn log_access(
    pool: &DbPool,
    user_id: Option<Uuid>,
    action: &str,
    ip_address: Option<&str>,
    success: bool,
    details: Option<&str>,
) -> Result<(), AppError> {
    sqlx::query(
        "INSERT INTO access_logs (id, user_id, action, ip_address, success, details, created_at) \
         VALUES ($1, $2, $3, $4, $5, $6, NOW())",
    )
    .bind(Uuid::new_v4())
    .bind(user_id)
    .bind(action)
    .bind(ip_address)
    .bind(success)
    .bind(details)
    .execute(pool)
    .await?;
    Ok(())
}


// ─────────────────────────────────────────────────────────────────────────────
// Tests
//
// Unit tests (password validation) run without a database.
// Integration tests (HTTP flows) require DATABASE_URL to point at a live
// PostgreSQL instance.  They are marked #[ignore] so `cargo test` skips them
// unless you pass `-- --include-ignored` or set DATABASE_URL explicitly.
//
// Run integration tests:
//   DATABASE_URL=postgres://meridian:meridian@localhost/meridian cargo test -- --include-ignored
// This file is appended to auth.rs — do not use directly.
// Unit tests (password validation) run without a database.
// Integration tests require DATABASE_URL and are marked #[ignore].
// Run integration tests:
//   DATABASE_URL=postgres://meridian:meridian@localhost/meridian \
//     cargo test -- --include-ignored --test-threads=1
#[cfg(test)]
mod tests {
    use super::*;
    // Import specific items to avoid shadowing the built-in #[test] attribute
    use actix_web::test::{call_service, init_service, read_body_json, TestRequest};
    use actix_web::{web, App};
    use serde_json::{json, Value};
    use sqlx::postgres::PgPoolOptions;
    use sqlx::PgPool;

    // ── Helpers ──────────────────────────────────────────────────────────────

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
            .expect("Failed to run migrations");
        pool
    }

    async fn seed_user(
        pool: &PgPool,
        username: &str,
        password: &str,
        role: &str,
        state: &str,
    ) {
        sqlx::query("DELETE FROM login_lockouts WHERE username = $1")
            .bind(username)
            .execute(pool)
            .await
            .expect("cleanup login_lockouts failed");
        sqlx::query("DELETE FROM login_attempts WHERE username = $1")
            .bind(username)
            .execute(pool)
            .await
            .expect("cleanup login_attempts failed");

        let hash = crate::services::auth::hash_password(password).unwrap();
        sqlx::query(
            "INSERT INTO users (id, username, email, password_hash, account_state, created_at, updated_at)
             VALUES (gen_random_uuid(), $1, $2, $3, $4, NOW(), NOW())
             ON CONFLICT (username) DO UPDATE
               SET password_hash = EXCLUDED.password_hash,
                   account_state = EXCLUDED.account_state,
                   updated_at    = NOW()",
        )
        .bind(username)
        .bind(format!("{}@test.local", username))
        .bind(&hash)
        .bind(state)
        .execute(pool)
        .await
        .expect("seed_user insert failed");

        let role_id: i32 = sqlx::query_scalar("SELECT id FROM roles WHERE name = $1")
            .bind(role)
            .fetch_one(pool)
            .await
            .expect("role not found — did migrations run?");

        let actual_id: Uuid = sqlx::query_scalar("SELECT id FROM users WHERE username = $1")
            .bind(username)
            .fetch_one(pool)
            .await
            .unwrap();

        sqlx::query(
            "INSERT INTO user_roles (user_id, role_id) VALUES ($1, $2) ON CONFLICT DO NOTHING",
        )
        .bind(actual_id)
        .bind(role_id)
        .execute(pool)
        .await
        .expect("user_roles insert failed");
    }

    async fn login_and_get_token(
        app: &impl actix_web::dev::Service<
            actix_http::Request,
            Response = actix_web::dev::ServiceResponse,
            Error = actix_web::Error,
        >,
        username: &str,
        password: &str,
    ) -> String {
        let req = TestRequest::post()
            .uri("/api/v1/auth/login")
            .set_json(json!({"username": username, "password": password}))
            .to_request();
        let resp = call_service(app, req).await;
        let body: Value = read_body_json(resp).await;
        body["token"]
            .as_str()
            .expect("login did not return a token")
            .to_string()
    }

    // ── Password validation — no DB required ────────────────────────────────

    #[test]
    fn test_password_min_length_enforced() {
        assert!(
            crate::services::auth::hash_password("short").is_err(),
            "< 12 chars should be rejected"
        );
    }

    #[test]
    fn test_password_exactly_12_accepted() {
        assert!(
            crate::services::auth::hash_password("Meridian2024!").is_ok(),
            "12-char password should be accepted"
        );
    }

    #[test]
    fn test_hash_verify_roundtrip() {
        let pw = "CorrectPassword123!";
        let hash = crate::services::auth::hash_password(pw).unwrap();
        assert!(crate::services::auth::verify_password(pw, &hash).is_ok());
    }

    #[test]
    fn test_wrong_password_rejected() {
        let hash = crate::services::auth::hash_password("CorrectPassword123!").unwrap();
        assert!(crate::services::auth::verify_password("Wrong!", &hash).is_err());
    }

    // ── Integration: login success ───────────────────────────────────────────

    #[actix_web::test]
    #[ignore = "requires DATABASE_URL pointing to a test PostgreSQL instance"]
    async fn test_login_success() {
        let pool = test_pool().await;
        seed_user(&pool, "t_ok", "TestPass2024!!", "Administrator", "active").await;
        let pool_data = web::Data::new(pool);
        let app = init_service(
            App::new()
                .app_data(pool_data)
                .configure(crate::routes::configure_routes),
        )
        .await;

        let req = TestRequest::post()
            .uri("/api/v1/auth/login")
            .set_json(json!({"username": "t_ok", "password": "TestPass2024!!"}))
            .to_request();
        let resp = call_service(&app, req).await;
        assert_eq!(resp.status(), 200);
        let body: Value = read_body_json(resp).await;
        assert!(body["token"].is_string());
        assert_eq!(body["user"]["username"], "t_ok");
    }

    // ── Integration: wrong password → 401 with safe message ─────────────────

    #[actix_web::test]
    #[ignore = "requires DATABASE_URL"]
    async fn test_login_wrong_password() {
        let pool = test_pool().await;
        seed_user(&pool, "t_wrong", "CorrectPass2024!", "Teacher", "active").await;
        let pool_data = web::Data::new(pool);
        let app = init_service(
            App::new()
                .app_data(pool_data)
                .configure(crate::routes::configure_routes),
        )
        .await;

        let req = TestRequest::post()
            .uri("/api/v1/auth/login")
            .set_json(json!({"username": "t_wrong", "password": "WrongPass2024!"}))
            .to_request();
        let resp = call_service(&app, req).await;
        assert_eq!(resp.status(), 401);
        let body: Value = read_body_json(resp).await;
        let msg = body["error"].as_str().unwrap_or("");
        // Must not leak whether the username exists
        assert_eq!(msg, "Invalid username or password.", "got: {}", msg);
    }

    // ── Integration: unknown username → same 401 as wrong password ───────────

    #[actix_web::test]
    #[ignore = "requires DATABASE_URL"]
    async fn test_login_unknown_user_same_message() {
        let pool = test_pool().await;
        let pool_data = web::Data::new(pool);
        let app = init_service(
            App::new()
                .app_data(pool_data)
                .configure(crate::routes::configure_routes),
        )
        .await;

        let req = TestRequest::post()
            .uri("/api/v1/auth/login")
            .set_json(json!({"username": "no_such_user_xyz999", "password": "SomePass2024!"}))
            .to_request();
        let resp = call_service(&app, req).await;
        assert_eq!(resp.status(), 401);
        let body: Value = read_body_json(resp).await;
        assert_eq!(
            body["error"].as_str().unwrap_or(""),
            "Invalid username or password."
        );
    }

    // ── Integration: disabled account → 403 ─────────────────────────────────

    #[actix_web::test]
    #[ignore = "requires DATABASE_URL"]
    async fn test_login_disabled_account() {
        let pool = test_pool().await;
        seed_user(&pool, "t_disabled", "DisabledPass2024!", "Student", "disabled").await;
        let pool_data = web::Data::new(pool);
        let app = init_service(
            App::new()
                .app_data(pool_data)
                .configure(crate::routes::configure_routes),
        )
        .await;

        let req = TestRequest::post()
            .uri("/api/v1/auth/login")
            .set_json(json!({"username": "t_disabled", "password": "DisabledPass2024!"}))
            .to_request();
        let resp = call_service(&app, req).await;
        assert_eq!(resp.status(), 403);
        let body: Value = read_body_json(resp).await;
        let msg = body["error"].as_str().unwrap_or("");
        assert!(msg.to_lowercase().contains("disabled"), "got: {}", msg);
    }

    // ── Integration: blacklisted account → 403 ──────────────────────────────

    #[actix_web::test]
    #[ignore = "requires DATABASE_URL"]
    async fn test_login_blacklisted_account() {
        let pool = test_pool().await;
        seed_user(&pool, "t_black", "BlackPass2024!!", "Student", "blacklisted").await;
        let pool_data = web::Data::new(pool);
        let app = init_service(
            App::new()
                .app_data(pool_data)
                .configure(crate::routes::configure_routes),
        )
        .await;

        let req = TestRequest::post()
            .uri("/api/v1/auth/login")
            .set_json(json!({"username": "t_black", "password": "BlackPass2024!!"}))
            .to_request();
        let resp = call_service(&app, req).await;
        assert_eq!(resp.status(), 403);
        let body: Value = read_body_json(resp).await;
        let msg = body["error"].as_str().unwrap_or("");
        assert!(
            msg.to_lowercase().contains("revoked") || msg.to_lowercase().contains("blacklist"),
            "got: {}",
            msg
        );
    }

    // ── Integration: 5 failures → 6th → 429 (lockout) ───────────────────────

    #[actix_web::test]
    #[ignore = "requires DATABASE_URL"]
    async fn test_login_lockout_after_threshold() {
        let pool = test_pool().await;
        seed_user(&pool, "t_lock", "RealPass2024!!", "Teacher", "active").await;

        // Clear prior attempts so test is deterministic
        sqlx::query("DELETE FROM login_attempts WHERE username = 't_lock'")
            .execute(&pool)
            .await
            .unwrap();

        let pool_data = web::Data::new(pool);
        let app = init_service(
            App::new()
                .app_data(pool_data)
                .configure(crate::routes::configure_routes),
        )
        .await;

        for i in 0..5u8 {
            let req = TestRequest::post()
                .uri("/api/v1/auth/login")
                .set_json(json!({"username": "t_lock", "password": "WrongPass2024!!"}))
                .to_request();
            let resp = call_service(&app, req).await;
            assert_eq!(resp.status(), 401, "attempt {} should be 401", i + 1);
        }

        // 6th → locked (429)
        let req = TestRequest::post()
            .uri("/api/v1/auth/login")
            .set_json(json!({"username": "t_lock", "password": "WrongPass2024!!"}))
            .to_request();
        let resp = call_service(&app, req).await;
        assert_eq!(resp.status(), 429, "6th failed attempt should be 429");

        // Correct password while locked → still 429
        let req = TestRequest::post()
            .uri("/api/v1/auth/login")
            .set_json(json!({"username": "t_lock", "password": "RealPass2024!!"}))
            .to_request();
        let resp = call_service(&app, req).await;
        assert_eq!(
            resp.status(),
            429,
            "correct password during lockout should still be 429"
        );
    }

    // ── Integration: /auth/me without token → 401 ───────────────────────────

    #[actix_web::test]
    #[ignore = "requires DATABASE_URL"]
    async fn test_me_no_token() {
        let pool = test_pool().await;
        let pool_data = web::Data::new(pool);
        let app = init_service(
            App::new()
                .app_data(pool_data)
                .configure(crate::routes::configure_routes),
        )
        .await;

        let req = TestRequest::get().uri("/api/v1/auth/me").to_request();
        let resp = call_service(&app, req).await;
        assert_eq!(resp.status(), 401);
    }

    // ── Integration: /auth/me with bad token → 401 ──────────────────────────

    #[actix_web::test]
    #[ignore = "requires DATABASE_URL"]
    async fn test_me_invalid_token() {
        let pool = test_pool().await;
        let pool_data = web::Data::new(pool);
        let app = init_service(
            App::new()
                .app_data(pool_data)
                .configure(crate::routes::configure_routes),
        )
        .await;

        let req = TestRequest::get()
            .uri("/api/v1/auth/me")
            .insert_header(("Authorization", "Bearer totally_invalid_token"))
            .to_request();
        let resp = call_service(&app, req).await;
        assert_eq!(resp.status(), 401);
    }

    // ── Integration: /auth/me with valid token → 200 ────────────────────────

    #[actix_web::test]
    #[ignore = "requires DATABASE_URL"]
    async fn test_me_valid_token() {
        let pool = test_pool().await;
        seed_user(&pool, "t_me", "MeTestPass2024!", "Teacher", "active").await;
        let pool_data = web::Data::new(pool);
        let app = init_service(
            App::new()
                .app_data(pool_data)
                .configure(crate::routes::configure_routes),
        )
        .await;

        let token = login_and_get_token(&app, "t_me", "MeTestPass2024!").await;

        let req = TestRequest::get()
            .uri("/api/v1/auth/me")
            .insert_header(("Authorization", format!("Bearer {}", token)))
            .to_request();
        let resp = call_service(&app, req).await;
        assert_eq!(resp.status(), 200);
        let body: Value = read_body_json(resp).await;
        assert_eq!(body["username"], "t_me");
    }

    // ── Integration: admin endpoint → 403 for teacher ───────────────────────

    #[actix_web::test]
    #[ignore = "requires DATABASE_URL"]
    async fn test_admin_endpoint_forbidden_for_teacher() {
        let pool = test_pool().await;
        seed_user(&pool, "t_teach403", "TeacherPass2024!", "Teacher", "active").await;
        let pool_data = web::Data::new(pool);
        let app = init_service(
            App::new()
                .app_data(pool_data)
                .configure(crate::routes::configure_routes),
        )
        .await;

        let token = login_and_get_token(&app, "t_teach403", "TeacherPass2024!").await;
        let req = TestRequest::get()
            .uri("/api/v1/admin/users")
            .insert_header(("Authorization", format!("Bearer {}", token)))
            .to_request();
        let resp = call_service(&app, req).await;
        assert_eq!(resp.status(), 403, "teacher should be forbidden from admin endpoints");
    }

    // ── Integration: admin endpoint → 401 without token ─────────────────────

    #[actix_web::test]
    #[ignore = "requires DATABASE_URL"]
    async fn test_admin_endpoint_unauthenticated() {
        let pool = test_pool().await;
        let pool_data = web::Data::new(pool);
        let app = init_service(
            App::new()
                .app_data(pool_data)
                .configure(crate::routes::configure_routes),
        )
        .await;

        let req = TestRequest::get()
            .uri("/api/v1/admin/users")
            .to_request();
        let resp = call_service(&app, req).await;
        assert_eq!(resp.status(), 401);
    }

    // ── Integration: admin can list users ────────────────────────────────────

    #[actix_web::test]
    #[ignore = "requires DATABASE_URL"]
    async fn test_admin_can_list_users() {
        let pool = test_pool().await;
        seed_user(&pool, "t_adminlist", "AdminPass2024!!", "Administrator", "active").await;
        let pool_data = web::Data::new(pool);
        let app = init_service(
            App::new()
                .app_data(pool_data)
                .configure(crate::routes::configure_routes),
        )
        .await;

        let token = login_and_get_token(&app, "t_adminlist", "AdminPass2024!!").await;
        let req = TestRequest::get()
            .uri("/api/v1/admin/users")
            .insert_header(("Authorization", format!("Bearer {}", token)))
            .to_request();
        let resp = call_service(&app, req).await;
        assert_eq!(resp.status(), 200);
        let body: Value = read_body_json(resp).await;
        assert!(body.is_array(), "expected JSON array, got: {:?}", body);
    }

    // ── Integration: teacher sees no windows when unassigned ─────────────────

    #[actix_web::test]
    #[ignore = "requires DATABASE_URL"]
    async fn test_teacher_out_of_scope_windows() {
        let pool = test_pool().await;
        seed_user(&pool, "t_unassigned", "TeachPass2024!!", "Teacher", "active").await;
        let pool_data = web::Data::new(pool);
        let app = init_service(
            App::new()
                .app_data(pool_data)
                .configure(crate::routes::configure_routes),
        )
        .await;

        let token = login_and_get_token(&app, "t_unassigned", "TeachPass2024!!").await;
        let req = TestRequest::get()
            .uri("/api/v1/check-ins/windows")
            .insert_header(("Authorization", format!("Bearer {}", token)))
            .to_request();
        let resp = call_service(&app, req).await;
        assert_eq!(resp.status(), 200, "should succeed but return scoped data");
        let body: Value = read_body_json(resp).await;
        assert!(
            body.as_array().map(|a| a.is_empty()).unwrap_or(false),
            "unassigned teacher should see no windows, got: {:?}",
            body
        );
    }

    // ── Integration: 404 for unknown route ──────────────────────────────────

    #[actix_web::test]
    #[ignore = "requires DATABASE_URL"]
    async fn test_verify_password_success() {
        let pool = test_pool().await;
        let suffix = Uuid::new_v4().to_string()[..8].to_string();
        let username = format!("verify_ok_{}", suffix);
        let password = "VerifyPass2024!!";
        seed_user(&pool, &username, password, "Student", "active").await;

        let app = init_service(
            App::new()
                .app_data(web::Data::new(pool.clone()))
                .configure(crate::routes::configure_routes),
        )
        .await;

        let token = login_and_get_token(&app, &username, password).await;

        let req = TestRequest::post()
            .uri("/api/v1/auth/verify")
            .insert_header(("Authorization", format!("Bearer {}", token)))
            .set_json(json!({ "password": password }))
            .to_request();
        let resp = call_service(&app, req).await;
        assert_eq!(resp.status(), 200, "correct password should unlock");
    }

    #[actix_web::test]
    #[ignore = "requires DATABASE_URL"]
    async fn test_verify_password_rejects_wrong_password() {
        let pool = test_pool().await;
        let suffix = Uuid::new_v4().to_string()[..8].to_string();
        let username = format!("verify_bad_{}", suffix);
        let password = "VerifyPass2024!!";
        seed_user(&pool, &username, password, "Teacher", "active").await;

        let app = init_service(
            App::new()
                .app_data(web::Data::new(pool.clone()))
                .configure(crate::routes::configure_routes),
        )
        .await;

        let token = login_and_get_token(&app, &username, password).await;

        let req = TestRequest::post()
            .uri("/api/v1/auth/verify")
            .insert_header(("Authorization", format!("Bearer {}", token)))
            .set_json(json!({ "password": "WrongPass2024!!" }))
            .to_request();
        let resp = call_service(&app, req).await;
        assert_eq!(resp.status(), 401, "wrong password should be rejected");
    }

    #[actix_web::test]
    #[ignore = "requires DATABASE_URL"]
    async fn test_verify_password_requires_auth() {
        let pool = test_pool().await;
        let app = init_service(
            App::new()
                .app_data(web::Data::new(pool.clone()))
                .configure(crate::routes::configure_routes),
        )
        .await;

        let req = TestRequest::post()
            .uri("/api/v1/auth/verify")
            .set_json(json!({ "password": "AnyPassword2024!!" }))
            .to_request();
        let resp = call_service(&app, req).await;
        assert_eq!(resp.status(), 401, "verify endpoint must require auth");
    }

    #[actix_web::test]
    #[ignore = "requires DATABASE_URL"]
    async fn test_404_for_unknown_route() {
        let pool = test_pool().await;
        let pool_data = web::Data::new(pool);
        let app = init_service(
            App::new()
                .app_data(pool_data)
                .configure(crate::routes::configure_routes),
        )
        .await;

        let req = TestRequest::get()
            .uri("/api/v1/does-not-exist")
            .to_request();
        let resp = call_service(&app, req).await;
        assert_eq!(resp.status(), 404);
    }
}
