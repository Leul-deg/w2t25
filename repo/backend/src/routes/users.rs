use actix_web::{web, HttpResponse};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::db::DbPool;
use crate::errors::AppError;
use crate::middleware::auth::AuthContext;
use crate::models::user::{User, UserPublic};

// ---------------------------------------------------------------------------
// Route configuration
// ---------------------------------------------------------------------------

pub fn configure(cfg: &mut web::ServiceConfig) {
    cfg.service(
        web::scope("/users")
            .route("/me", web::get().to(get_me))
            .route("/me/request-deletion", web::post().to(request_deletion))
            .route("/me/linked-students", web::get().to(linked_students)),
    );
}

// ---------------------------------------------------------------------------
// Request types
// ---------------------------------------------------------------------------

#[derive(Deserialize)]
struct DeletionRequestBody {
    reason: Option<String>,
}

// ---------------------------------------------------------------------------
// Handlers
// ---------------------------------------------------------------------------

/// GET /api/v1/users/me
async fn get_me(
    pool: web::Data<DbPool>,
    auth: AuthContext,
) -> Result<HttpResponse, AppError> {
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

/// POST /api/v1/users/me/request-deletion
async fn request_deletion(
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

/// GET /api/v1/users/me/linked-students
///
/// Parent role only. Returns the list of students linked via parent_student_links.
async fn linked_students(
    pool: web::Data<DbPool>,
    auth: AuthContext,
) -> Result<HttpResponse, AppError> {
    auth.require_role("Parent")?;

    #[derive(sqlx::FromRow, Serialize)]
    struct LinkedStudent {
        id: Uuid,
        username: String,
        display_name: Option<String>,
    }

    let students = sqlx::query_as::<_, LinkedStudent>(
        "SELECT u.id, u.username, u.display_name
         FROM users u
         JOIN parent_student_links psl ON psl.student_id = u.id
         WHERE psl.parent_id = $1
         ORDER BY u.username",
    )
    .bind(auth.0.user_id)
    .fetch_all(pool.get_ref())
    .await?;

    Ok(HttpResponse::Ok().json(students))
}
