use actix_web::dev::ServiceRequest;
use actix_web::{web, FromRequest, HttpRequest};
use sqlx::Row;
use std::future::Future;
use std::pin::Pin;
use uuid::Uuid;

use crate::db::DbPool;
use crate::errors::AppError;
use crate::models::session::Session;
use crate::models::user::User;

/// Stored in request extensions after token validation.
#[derive(Debug, Clone)]
pub struct AuthenticatedUser {
    pub user_id: Uuid,
    pub username: String,
    pub roles: Vec<String>,
    pub account_state: String,
}

/// Extract Bearer token from the Authorization header.
pub fn extract_bearer_token(req: &ServiceRequest) -> Option<String> {
    req.headers()
        .get("Authorization")
        .and_then(|v| v.to_str().ok())
        .and_then(|v| v.strip_prefix("Bearer "))
        .map(|s| s.to_string())
}

/// Validate the token against the sessions table and build an AuthenticatedUser.
pub async fn resolve_authenticated_user(
    pool: &DbPool,
    token: &str,
) -> Result<AuthenticatedUser, AppError> {
    // 1. Look up a non-expired session by token.
    let session = sqlx::query_as::<_, Session>(
        "SELECT id, user_id, token, created_at, expires_at, ip_address, user_agent \
         FROM sessions \
         WHERE token = $1 AND expires_at > NOW()",
    )
    .bind(token)
    .fetch_optional(pool)
    .await?;

    let session = session.ok_or_else(|| {
        AppError::Unauthorized("Invalid or expired session".into())
    })?;

    // 2. Load the associated user.
    let user = sqlx::query_as::<_, User>(
        "SELECT id, username, email, password_hash, display_name, account_state, \
         created_at, updated_at \
         FROM users \
         WHERE id = $1",
    )
    .bind(session.user_id)
    .fetch_optional(pool)
    .await?;

    let user = user.ok_or_else(|| {
        AppError::Unauthorized("User associated with session no longer exists".into())
    })?;

    // 3. Verify account is active.
    if user.account_state != "active" {
        return Err(AppError::Forbidden(format!(
            "Account is {}",
            user.account_state
        )));
    }

    // 4. Fetch roles.
    let role_rows = sqlx::query_scalar::<_, String>(
        "SELECT r.name FROM roles r \
         JOIN user_roles ur ON r.id = ur.role_id \
         WHERE ur.user_id = $1",
    )
    .bind(user.id)
    .fetch_all(pool)
    .await?;

    Ok(AuthenticatedUser {
        user_id: user.id,
        username: user.username,
        roles: role_rows,
        account_state: user.account_state,
    })
}

/// Request-scoped authenticated user. Use as a handler parameter to require authentication.
/// Returns 401 if no/invalid token, 403 if account blocked.
pub struct AuthContext(pub AuthenticatedUser);

impl AuthContext {
    pub fn user(&self) -> &AuthenticatedUser {
        &self.0
    }

    /// Returns Forbidden if the user does not have the specified role.
    pub fn require_role(&self, role: &str) -> Result<(), AppError> {
        if self.0.roles.iter().any(|r| r == role) {
            Ok(())
        } else {
            Err(AppError::Forbidden(format!(
                "This action requires the {} role.",
                role
            )))
        }
    }

    /// Returns Forbidden if the user has none of the specified roles.
    pub fn require_any_role(&self, roles: &[&str]) -> Result<(), AppError> {
        if roles.iter().any(|r| self.0.roles.iter().any(|ur| ur == r)) {
            Ok(())
        } else {
            Err(AppError::Forbidden(
                "You do not have permission to perform this action.".into(),
            ))
        }
    }

    pub fn is_admin(&self) -> bool {
        self.0.roles.iter().any(|r| r == "Administrator")
    }

    pub fn is_teacher(&self) -> bool {
        self.0.roles.iter().any(|r| r == "Teacher")
    }
}

impl FromRequest for AuthContext {
    type Error = AppError;
    type Future = Pin<Box<dyn Future<Output = Result<Self, Self::Error>>>>;

    fn from_request(req: &HttpRequest, _payload: &mut actix_web::dev::Payload) -> Self::Future {
        let token = req
            .headers()
            .get("Authorization")
            .and_then(|v| v.to_str().ok())
            .and_then(|v| v.strip_prefix("Bearer "))
            .map(|s| s.to_string());

        let pool = req.app_data::<web::Data<crate::db::DbPool>>().cloned();

        Box::pin(async move {
            let token = token.ok_or_else(|| {
                AppError::Unauthorized("Missing Authorization header".into())
            })?;
            let pool = pool.ok_or_else(|| {
                AppError::InternalError("DB pool not configured".into())
            })?;
            let user = resolve_authenticated_user(pool.get_ref(), &token).await?;
            Ok(AuthContext(user))
        })
    }
}

/// Verify that a user (teacher or admin) has access to the given class.
/// Admins pass unconditionally. Teachers must be assigned to the class.
/// Other roles are rejected.
pub async fn require_class_access(
    auth: &AuthContext,
    pool: &crate::db::DbPool,
    class_id: Uuid,
) -> Result<(), AppError> {
    if auth.is_admin() {
        let school_id: Uuid = sqlx::query_scalar(
            "SELECT school_id FROM classes WHERE id = $1",
        )
        .bind(class_id)
        .fetch_optional(pool)
        .await?
        .ok_or_else(|| AppError::NotFound("Class not found.".into()))?;
        return require_school_access(auth, pool, school_id).await;
    }
    if !auth.is_teacher() {
        return Err(AppError::Forbidden(
            "Only teachers and administrators can access class data.".into(),
        ));
    }
    let count: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM classes WHERE id = $1 AND teacher_id = $2",
    )
    .bind(class_id)
    .bind(auth.0.user_id)
    .fetch_one(pool)
    .await?;

    if count == 0 {
        return Err(AppError::Forbidden(
            "You are not assigned to this class.".into(),
        ));
    }
    Ok(())
}

/// Verify that the calling admin is a super-admin (`is_super_admin = true`).
/// Returns `Forbidden` for any admin without the flag, regardless of whether
/// they have scope rows — campus/district-scoped admins must not mutate global
/// resources (product catalogue, system config, logs, backups, reports).
///
/// Only call this after `auth.require_role("Administrator")` has already passed.
pub async fn require_global_admin_scope(
    admin_id: Uuid,
    pool: &crate::db::DbPool,
) -> Result<(), AppError> {
    let is_super: bool = sqlx::query_scalar(
        "SELECT is_super_admin FROM users WHERE id = $1",
    )
    .bind(admin_id)
    .fetch_one(pool)
    .await?;

    if !is_super {
        return Err(AppError::Forbidden(
            "This operation requires unrestricted administrative access. \
             Your account is not designated as a super-admin."
                .into(),
        ));
    }
    Ok(())
}

/// Returns the set of campus IDs the admin may manage.
///
/// `None`      => user is a super-admin and can access all campuses.
/// `Some(ids)` => admin is restricted to those campus IDs.
pub async fn get_admin_campus_scope(
    pool: &crate::db::DbPool,
    admin_id: Uuid,
) -> Result<Option<Vec<Uuid>>, AppError> {
    let is_super: bool = sqlx::query_scalar(
        "SELECT is_super_admin FROM users WHERE id = $1",
    )
    .bind(admin_id)
    .fetch_one(pool)
    .await?;

    if is_super {
        return Ok(None);
    }

    let rows = sqlx::query(
        "SELECT scope_type, scope_id FROM admin_scope_assignments WHERE admin_id = $1",
    )
    .bind(admin_id)
    .fetch_all(pool)
    .await?;

    if rows.is_empty() {
        return Ok(Some(vec![]));
    }

    let mut campus_ids = Vec::new();
    for row in rows {
        let scope_type: String = row.try_get("scope_type")?;
        let scope_id: Uuid = row.try_get("scope_id")?;
        match scope_type.as_str() {
            "campus" => campus_ids.push(scope_id),
            "district" => {
                let ids: Vec<Uuid> = sqlx::query_scalar(
                    "SELECT id FROM campuses WHERE district_id = $1",
                )
                .bind(scope_id)
                .fetch_all(pool)
                .await?;
                campus_ids.extend(ids);
            }
            _ => {}
        }
    }

    Ok(Some(campus_ids))
}

pub async fn require_order_in_admin_scope(
    admin_id: Uuid,
    pool: &crate::db::DbPool,
    order_id: Uuid,
) -> Result<(), AppError> {
    let scope = get_admin_campus_scope(pool, admin_id).await?;
    let campus_ids = match scope {
        None => return Ok(()),
        Some(ids) => ids,
    };

    let in_scope: bool = sqlx::query_scalar(
        "SELECT EXISTS(
             SELECT 1
             FROM orders o
             JOIN user_school_assignments usa ON usa.user_id = o.user_id
             JOIN schools s ON s.id = usa.school_id
             WHERE o.id = $1 AND s.campus_id = ANY($2)
         )",
    )
    .bind(order_id)
    .bind(campus_ids.as_slice())
    .fetch_one(pool)
    .await?;

    if !in_scope {
        return Err(AppError::Forbidden(
            "Order is outside your administrative scope.".into(),
        ));
    }
    Ok(())
}

/// Verify that a user has access to the given school.
/// Super-admins pass. Scoped admins must have the school's campus in scope.
/// Teachers/AcademicStaff must be in user_school_assignments.
pub async fn require_school_access(
    auth: &AuthContext,
    pool: &crate::db::DbPool,
    school_id: Uuid,
) -> Result<(), AppError> {
    if auth.is_admin() {
        let scope = get_admin_campus_scope(pool, auth.0.user_id).await?;
        let campus_ids = match scope {
            None => return Ok(()),
            Some(ids) => ids,
        };

        let campus_id: Uuid = sqlx::query_scalar(
            "SELECT campus_id FROM schools WHERE id = $1",
        )
        .bind(school_id)
        .fetch_optional(pool)
        .await?
        .ok_or_else(|| AppError::NotFound("School not found.".into()))?;

        if campus_ids.contains(&campus_id) {
            return Ok(());
        }
        return Err(AppError::Forbidden(
            "You are not assigned to this school.".into(),
        ));
    }
    let count: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM user_school_assignments WHERE school_id = $1 AND user_id = $2",
    )
    .bind(school_id)
    .bind(auth.0.user_id)
    .fetch_one(pool)
    .await?;

    if count == 0 {
        return Err(AppError::Forbidden(
            "You are not assigned to this school.".into(),
        ));
    }
    Ok(())
}
