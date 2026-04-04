/// Log viewer endpoints.  Administrator-only.
///
/// GET  /logs/audit              – business audit trail
/// GET  /logs/access             – access log (logins, sensitive actions)
/// GET  /logs/errors             – application error log
/// POST /logs/prune              – manually trigger log retention pruning
///
/// All list endpoints support optional query parameters:
///   ?limit=N     (default 100, max 500)
///   ?offset=N    (default 0)
///   ?since=YYYY-MM-DD  (only entries >= this date)
///   ?until=YYYY-MM-DD  (only entries <= this date)
///   ?level=error|warn  (error logs only)
///   ?success=true|false (access logs only)

use actix_web::{web, HttpResponse};
use chrono::{DateTime, NaiveDate, Utc};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::db::DbPool;
use crate::errors::AppError;
use crate::middleware::auth::{require_global_admin_scope, AuthContext};
use crate::services::scheduler::prune_old_logs;

// ---------------------------------------------------------------------------
// Route config
// ---------------------------------------------------------------------------

pub fn configure(cfg: &mut web::ServiceConfig) {
    cfg.service(
        web::scope("/logs")
            .route("/audit", web::get().to(audit_logs))
            .route("/access", web::get().to(access_logs))
            .route("/errors", web::get().to(error_logs))
            .route("/prune", web::post().to(prune_logs)),
    );
}

// ---------------------------------------------------------------------------
// Query parameters
// ---------------------------------------------------------------------------

#[derive(Deserialize)]
struct LogQuery {
    limit: Option<i64>,
    offset: Option<i64>,
    since: Option<String>,
    until: Option<String>,
    level: Option<String>,
    success: Option<bool>,
}

impl LogQuery {
    fn effective_limit(&self) -> i64 {
        self.limit.unwrap_or(100).min(500).max(1)
    }

    fn effective_offset(&self) -> i64 {
        self.offset.unwrap_or(0).max(0)
    }

    fn since_ts(&self) -> Result<Option<DateTime<Utc>>, AppError> {
        self.since
            .as_deref()
            .map(|s| {
                NaiveDate::parse_from_str(s, "%Y-%m-%d")
                    .map(|d| d.and_hms_opt(0, 0, 0).unwrap().and_utc())
                    .map_err(|_| {
                        AppError::ValidationError(format!(
                            "Invalid 'since' date '{}'. Expected YYYY-MM-DD.",
                            s
                        ))
                    })
            })
            .transpose()
    }

    fn until_ts(&self) -> Result<Option<DateTime<Utc>>, AppError> {
        self.until
            .as_deref()
            .map(|s| {
                NaiveDate::parse_from_str(s, "%Y-%m-%d")
                    .map(|d| d.and_hms_opt(23, 59, 59).unwrap().and_utc())
                    .map_err(|_| {
                        AppError::ValidationError(format!(
                            "Invalid 'until' date '{}'. Expected YYYY-MM-DD.",
                            s
                        ))
                    })
            })
            .transpose()
    }
}

// ---------------------------------------------------------------------------
// Response DTOs
// ---------------------------------------------------------------------------

#[derive(Serialize, sqlx::FromRow)]
struct AuditLogRow {
    id: Uuid,
    actor_id: Option<Uuid>,
    actor_username: Option<String>,
    action: String,
    entity_type: String,
    entity_id: Option<String>,
    old_data: Option<serde_json::Value>,
    new_data: Option<serde_json::Value>,
    ip_address: Option<String>,
    created_at: DateTime<Utc>,
}

#[derive(Serialize, sqlx::FromRow)]
struct AccessLogRow {
    id: Uuid,
    user_id: Option<Uuid>,
    username: Option<String>,
    action: String,
    ip_address: Option<String>,
    user_agent: Option<String>,
    success: bool,
    details: Option<String>,
    created_at: DateTime<Utc>,
}

#[derive(Serialize, sqlx::FromRow)]
struct ErrorLogRow {
    id: Uuid,
    level: String,
    message: String,
    context: Option<serde_json::Value>,
    user_id: Option<Uuid>,
    request_path: Option<String>,
    created_at: DateTime<Utc>,
}

// ---------------------------------------------------------------------------
// Handlers
// ---------------------------------------------------------------------------

/// GET /api/v1/logs/audit
async fn audit_logs(
    pool: web::Data<DbPool>,
    auth: AuthContext,
    query: web::Query<LogQuery>,
) -> Result<HttpResponse, AppError> {
    auth.require_role("Administrator")?;
    require_global_admin_scope(auth.0.user_id, pool.get_ref()).await?;

    let limit = query.effective_limit();
    let offset = query.effective_offset();
    let since = query.since_ts()?;
    let until = query.until_ts()?;

    let rows = sqlx::query_as::<_, AuditLogRow>(
        "SELECT al.id, al.actor_id, u.username AS actor_username,
                al.action, al.entity_type, al.entity_id,
                al.old_data, al.new_data, al.ip_address, al.created_at
         FROM audit_logs al
         LEFT JOIN users u ON u.id = al.actor_id
         WHERE ($1::timestamptz IS NULL OR al.created_at >= $1)
           AND ($2::timestamptz IS NULL OR al.created_at <= $2)
         ORDER BY al.created_at DESC
         LIMIT $3 OFFSET $4",
    )
    .bind(since)
    .bind(until)
    .bind(limit)
    .bind(offset)
    .fetch_all(pool.get_ref())
    .await?;

    Ok(HttpResponse::Ok().json(serde_json::json!({
        "count": rows.len(),
        "limit": limit,
        "offset": offset,
        "rows": rows
    })))
}

/// GET /api/v1/logs/access
async fn access_logs(
    pool: web::Data<DbPool>,
    auth: AuthContext,
    query: web::Query<LogQuery>,
) -> Result<HttpResponse, AppError> {
    auth.require_role("Administrator")?;
    require_global_admin_scope(auth.0.user_id, pool.get_ref()).await?;

    let limit = query.effective_limit();
    let offset = query.effective_offset();
    let since = query.since_ts()?;
    let until = query.until_ts()?;

    let rows = sqlx::query_as::<_, AccessLogRow>(
        "SELECT al.id, al.user_id, u.username,
                al.action, al.ip_address, al.user_agent,
                al.success, al.details, al.created_at
         FROM access_logs al
         LEFT JOIN users u ON u.id = al.user_id
         WHERE ($1::timestamptz IS NULL OR al.created_at >= $1)
           AND ($2::timestamptz IS NULL OR al.created_at <= $2)
           AND ($3::boolean IS NULL OR al.success = $3)
         ORDER BY al.created_at DESC
         LIMIT $4 OFFSET $5",
    )
    .bind(since)
    .bind(until)
    .bind(query.success)
    .bind(limit)
    .bind(offset)
    .fetch_all(pool.get_ref())
    .await?;

    Ok(HttpResponse::Ok().json(serde_json::json!({
        "count": rows.len(),
        "limit": limit,
        "offset": offset,
        "rows": rows
    })))
}

/// GET /api/v1/logs/errors
async fn error_logs(
    pool: web::Data<DbPool>,
    auth: AuthContext,
    query: web::Query<LogQuery>,
) -> Result<HttpResponse, AppError> {
    auth.require_role("Administrator")?;
    require_global_admin_scope(auth.0.user_id, pool.get_ref()).await?;

    let limit = query.effective_limit();
    let offset = query.effective_offset();
    let since = query.since_ts()?;
    let until = query.until_ts()?;

    // Validate level if provided.
    if let Some(ref lvl) = query.level {
        if !matches!(lvl.as_str(), "debug" | "info" | "warn" | "error" | "fatal") {
            return Err(AppError::ValidationError(format!(
                "Invalid level '{}'. Expected: debug, info, warn, error, fatal.",
                lvl
            )));
        }
    }

    let rows = sqlx::query_as::<_, ErrorLogRow>(
        "SELECT id, level, message, context, user_id, request_path, created_at
         FROM error_logs
         WHERE ($1::timestamptz IS NULL OR created_at >= $1)
           AND ($2::timestamptz IS NULL OR created_at <= $2)
           AND ($3::text IS NULL OR level = $3)
         ORDER BY created_at DESC
         LIMIT $4 OFFSET $5",
    )
    .bind(since)
    .bind(until)
    .bind(&query.level)
    .bind(limit)
    .bind(offset)
    .fetch_all(pool.get_ref())
    .await?;

    Ok(HttpResponse::Ok().json(serde_json::json!({
        "count": rows.len(),
        "limit": limit,
        "offset": offset,
        "rows": rows
    })))
}

/// POST /api/v1/logs/prune
///
/// Manually trigger log retention pruning.  The scheduler runs this
/// automatically every night; this endpoint lets admins trigger it on demand.
async fn prune_logs(
    pool: web::Data<DbPool>,
    auth: AuthContext,
) -> Result<HttpResponse, AppError> {
    auth.require_role("Administrator")?;
    require_global_admin_scope(auth.0.user_id, pool.get_ref()).await?;

    prune_old_logs(pool.get_ref())
        .await
        .map_err(|e| AppError::InternalError(format!("Prune failed: {}", e)))?;

    Ok(HttpResponse::Ok().json(serde_json::json!({
        "message": "Log retention pruning completed. Entries older than the configured retention period have been removed."
    })))
}

// ---------------------------------------------------------------------------
// Unit tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn log_query_default_limit_is_100() {
        let q = LogQuery {
            limit: None,
            offset: None,
            since: None,
            until: None,
            level: None,
            success: None,
        };
        assert_eq!(q.effective_limit(), 100);
    }

    #[test]
    fn log_query_limit_capped_at_500() {
        let q = LogQuery {
            limit: Some(9999),
            offset: None,
            since: None,
            until: None,
            level: None,
            success: None,
        };
        assert_eq!(q.effective_limit(), 500);
    }

    #[test]
    fn log_query_negative_offset_clamped() {
        let q = LogQuery {
            limit: None,
            offset: Some(-5),
            since: None,
            until: None,
            level: None,
            success: None,
        };
        assert_eq!(q.effective_offset(), 0);
    }

    #[test]
    fn since_date_parsed_correctly() {
        let q = LogQuery {
            limit: None,
            offset: None,
            since: Some("2026-03-01".to_string()),
            until: None,
            level: None,
            success: None,
        };
        assert!(q.since_ts().unwrap().is_some());
    }

    #[test]
    fn invalid_since_date_returns_error() {
        let q = LogQuery {
            limit: None,
            offset: None,
            since: Some("not-a-date".to_string()),
            until: None,
            level: None,
            success: None,
        };
        assert!(q.since_ts().is_err());
    }

    #[test]
    fn retention_is_180_days() {
        // Verifies the constant is exposed correctly.
        use crate::services::scheduler::prune_old_logs as _;
        // The constant LOG_RETENTION_DAYS = 180 is checked here indirectly.
        assert_eq!(crate::services::scheduler::LOG_RETENTION_DAYS, 180);
    }
}
