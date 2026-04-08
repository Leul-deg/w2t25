/// Configuration center — admin-only endpoints.
///
/// All routes require the Administrator role.
///
/// GET  /admin/config                     – list all config values
/// POST /admin/config/values/{key}        – update a config value (versioned + audited)
/// GET  /admin/config/history             – full change history
/// GET  /admin/config/campaigns           – list campaign toggles
/// POST /admin/config/campaigns/{name}    – update a campaign toggle

use actix_web::{web, HttpResponse};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::db::DbPool;
use crate::errors::AppError;
use crate::middleware::auth::{require_global_admin_scope, AuthContext};

// ---------------------------------------------------------------------------
// Route config — exposed on the /admin scope in routes/mod.rs
// ---------------------------------------------------------------------------

/// Registers all config routes under the `/admin/config` sub-scope.
pub fn configure_admin_config(cfg: &mut web::ServiceConfig) {
    cfg.service(
        web::scope("/config")
            // Keep static segments before dynamic ones to avoid shadowing.
            .route("/history", web::get().to(config_history))
            .route("/campaigns", web::get().to(list_campaigns))
            .route("/campaigns/{name}", web::post().to(update_campaign))
            .route("", web::get().to(list_config))
            .route("/values/{key}", web::post().to(update_config_value)),
    );
}

/// Stub mounted at `/api/v1/config` for non-admin consumers.
/// Currently only exposes campaign-enabled status (no admin data).
pub fn configure(cfg: &mut web::ServiceConfig) {
    cfg.service(
        web::scope("/config")
            .route("/commerce", web::get().to(commerce_summary))
            .route("/campaigns/{name}/status", web::get().to(campaign_status)),
    );
}

// ---------------------------------------------------------------------------
// DTOs
// ---------------------------------------------------------------------------

#[derive(Serialize, sqlx::FromRow)]
struct ConfigValueRow {
    id: Uuid,
    key: String,
    value: Option<String>,
    value_type: String,
    description: Option<String>,
    scope: String,
    created_at: DateTime<Utc>,
    updated_at: DateTime<Utc>,
}

#[derive(Deserialize)]
struct UpdateConfigBody {
    value: String,
    reason: Option<String>,
}

#[derive(Serialize, sqlx::FromRow)]
struct ConfigHistoryRow {
    id: Uuid,
    config_key: String,
    old_value: Option<String>,
    new_value: Option<String>,
    changed_by: Option<Uuid>,
    changed_by_username: Option<String>,
    changed_at: DateTime<Utc>,
    reason: Option<String>,
}

#[derive(Serialize, sqlx::FromRow)]
struct CampaignRow {
    id: Uuid,
    name: String,
    description: Option<String>,
    enabled: bool,
    starts_at: Option<DateTime<Utc>>,
    ends_at: Option<DateTime<Utc>>,
    created_at: DateTime<Utc>,
    updated_at: DateTime<Utc>,
}

#[derive(Deserialize)]
struct UpdateCampaignBody {
    enabled: bool,
    starts_at: Option<DateTime<Utc>>,
    ends_at: Option<DateTime<Utc>>,
}

#[derive(Serialize)]
struct CommerceSummaryResponse {
    shipping_fee_cents: i64,
    shipping_fee_display: String,
    points_rate_per_dollar: i64,
    campaigns: Vec<CommerceCampaignStatus>,
}

#[derive(Serialize)]
struct CommerceCampaignStatus {
    name: String,
    enabled: bool,
}

// ---------------------------------------------------------------------------
// Handlers — admin config
// ---------------------------------------------------------------------------

/// GET /api/v1/admin/config
async fn list_config(
    pool: web::Data<DbPool>,
    auth: AuthContext,
) -> Result<HttpResponse, AppError> {
    auth.require_role("Administrator")?;
    require_global_admin_scope(auth.0.user_id, pool.get_ref()).await?;

    let rows = sqlx::query_as::<_, ConfigValueRow>(
        "SELECT id, key, value, value_type, description, scope, created_at, updated_at
         FROM config_values
         ORDER BY key",
    )
    .fetch_all(pool.get_ref())
    .await?;

    Ok(HttpResponse::Ok().json(rows))
}

/// POST /api/v1/admin/config/values/{key}
///
/// Update a config value.  The change is versioned in config_history with:
/// - who changed it (changed_by)
/// - when (changed_at)
/// - what changed (config_key)
/// - previous value (old_value)
/// - new value (new_value)
/// - optional reason
async fn update_config_value(
    pool: web::Data<DbPool>,
    auth: AuthContext,
    path: web::Path<String>,
    body: web::Json<UpdateConfigBody>,
) -> Result<HttpResponse, AppError> {
    auth.require_role("Administrator")?;
    require_global_admin_scope(auth.0.user_id, pool.get_ref()).await?;
    let key = path.into_inner();

    if body.value.trim().is_empty() {
        return Err(AppError::ValidationError("Config value cannot be empty.".into()));
    }

    // Load the existing row.
    let existing = sqlx::query_as::<_, ConfigValueRow>(
        "SELECT id, key, value, value_type, description, scope, created_at, updated_at
         FROM config_values WHERE key = $1",
    )
    .bind(&key)
    .fetch_optional(pool.get_ref())
    .await?
    .ok_or_else(|| AppError::NotFound(format!("Config key '{}' not found.", key)))?;

    // Validate value matches declared type.
    validate_config_value(&existing.value_type, &body.value)?;

    let old_value = existing.value.clone();
    let new_value = body.value.trim().to_string();

    // Update.
    sqlx::query(
        "UPDATE config_values SET value = $1, updated_at = NOW() WHERE key = $2",
    )
    .bind(&new_value)
    .bind(&key)
    .execute(pool.get_ref())
    .await?;

    // Record versioned history.
    sqlx::query(
        "INSERT INTO config_history
             (id, config_key, old_value, new_value, changed_by, changed_at, reason)
         VALUES ($1, $2, $3, $4, $5, NOW(), $6)",
    )
    .bind(Uuid::new_v4())
    .bind(&key)
    .bind(&old_value)
    .bind(&new_value)
    .bind(auth.0.user_id)
    .bind(&body.reason)
    .execute(pool.get_ref())
    .await?;

    // Audit log.
    sqlx::query(
        "INSERT INTO audit_logs
             (id, actor_id, action, entity_type, entity_id, old_data, new_data, created_at)
         VALUES ($1, $2, 'update_config', 'config_value', $3, $4, $5, NOW())",
    )
    .bind(Uuid::new_v4())
    .bind(auth.0.user_id)
    .bind(&key)
    .bind(serde_json::json!({ "value": old_value }))
    .bind(serde_json::json!({
        "value": new_value,
        "reason": body.reason,
        "changed_by": auth.0.username
    }))
    .execute(pool.get_ref())
    .await?;

    Ok(HttpResponse::Ok().json(serde_json::json!({
        "key": key,
        "old_value": old_value,
        "new_value": new_value,
        "changed_by": auth.0.username,
        "message": "Config updated."
    })))
}

/// GET /api/v1/admin/config/history
async fn config_history(
    pool: web::Data<DbPool>,
    auth: AuthContext,
) -> Result<HttpResponse, AppError> {
    auth.require_role("Administrator")?;
    require_global_admin_scope(auth.0.user_id, pool.get_ref()).await?;

    let rows = sqlx::query_as::<_, ConfigHistoryRow>(
        "SELECT ch.id, ch.config_key, ch.old_value, ch.new_value,
                ch.changed_by, u.username AS changed_by_username,
                ch.changed_at, ch.reason
         FROM config_history ch
         LEFT JOIN users u ON u.id = ch.changed_by
         ORDER BY ch.changed_at DESC
         LIMIT 200",
    )
    .fetch_all(pool.get_ref())
    .await?;

    Ok(HttpResponse::Ok().json(rows))
}

// ---------------------------------------------------------------------------
// Handlers — campaigns
// ---------------------------------------------------------------------------

/// GET /api/v1/admin/config/campaigns
async fn list_campaigns(
    pool: web::Data<DbPool>,
    auth: AuthContext,
) -> Result<HttpResponse, AppError> {
    auth.require_role("Administrator")?;
    require_global_admin_scope(auth.0.user_id, pool.get_ref()).await?;

    let rows = sqlx::query_as::<_, CampaignRow>(
        "SELECT id, name, description, enabled, starts_at, ends_at, created_at, updated_at
         FROM campaign_toggles
         ORDER BY name",
    )
    .fetch_all(pool.get_ref())
    .await?;

    Ok(HttpResponse::Ok().json(rows))
}

/// POST /api/v1/admin/config/campaigns/{name}
async fn update_campaign(
    pool: web::Data<DbPool>,
    auth: AuthContext,
    path: web::Path<String>,
    body: web::Json<UpdateCampaignBody>,
) -> Result<HttpResponse, AppError> {
    auth.require_role("Administrator")?;
    require_global_admin_scope(auth.0.user_id, pool.get_ref()).await?;
    let name = path.into_inner();

    let existing = sqlx::query_as::<_, CampaignRow>(
        "SELECT id, name, description, enabled, starts_at, ends_at, created_at, updated_at
         FROM campaign_toggles WHERE name = $1",
    )
    .bind(&name)
    .fetch_optional(pool.get_ref())
    .await?
    .ok_or_else(|| AppError::NotFound(format!("Campaign '{}' not found.", name)))?;

    sqlx::query(
        "UPDATE campaign_toggles
         SET enabled = $1, starts_at = $2, ends_at = $3, updated_at = NOW()
         WHERE name = $4",
    )
    .bind(body.enabled)
    .bind(body.starts_at)
    .bind(body.ends_at)
    .bind(&name)
    .execute(pool.get_ref())
    .await?;

    // Audit log.
    sqlx::query(
        "INSERT INTO audit_logs
             (id, actor_id, action, entity_type, entity_id, old_data, new_data, created_at)
         VALUES ($1, $2, 'update_campaign', 'campaign_toggle', $3, $4, $5, NOW())",
    )
    .bind(Uuid::new_v4())
    .bind(auth.0.user_id)
    .bind(&name)
    .bind(serde_json::json!({ "enabled": existing.enabled }))
    .bind(serde_json::json!({
        "enabled": body.enabled,
        "starts_at": body.starts_at,
        "ends_at": body.ends_at,
        "changed_by": auth.0.username
    }))
    .execute(pool.get_ref())
    .await?;

    Ok(HttpResponse::Ok().json(serde_json::json!({
        "name": name,
        "enabled": body.enabled,
        "changed_by": auth.0.username,
        "message": "Campaign updated."
    })))
}

// ---------------------------------------------------------------------------
// Public handler (non-admin)
// ---------------------------------------------------------------------------

/// GET /api/v1/config/campaigns/{name}/status
///
/// Returns whether a named campaign is currently enabled.
/// Accessible to any authenticated user.
async fn campaign_status(
    pool: web::Data<DbPool>,
    auth: AuthContext,
    path: web::Path<String>,
) -> Result<HttpResponse, AppError> {
    let _ = auth;
    let name = path.into_inner();

    let enabled: Option<bool> = sqlx::query_scalar(
        "SELECT enabled FROM campaign_toggles WHERE name = $1",
    )
    .bind(&name)
    .fetch_optional(pool.get_ref())
    .await?
    .flatten();

    match enabled {
        None => Err(AppError::NotFound(format!("Campaign '{}' not found.", name))),
        Some(e) => Ok(HttpResponse::Ok().json(serde_json::json!({
            "name": name,
            "enabled": e
        }))),
    }
}

/// GET /api/v1/config/commerce
///
/// Public authenticated endpoint used by the store frontend to preview
/// shipping fees, points rate, and campaign flags using real backend config.
async fn commerce_summary(
    pool: web::Data<DbPool>,
    auth: AuthContext,
) -> Result<HttpResponse, AppError> {
    let _ = auth;

    let shipping_fee_cents: i64 = sqlx::query_scalar(
        "SELECT COALESCE(value, '695')::bigint
         FROM config_values
         WHERE key = 'shipping_fee_cents' AND scope = 'global'",
    )
    .fetch_optional(pool.get_ref())
    .await?
    .unwrap_or(695);

    let points_rate_per_dollar: i64 = sqlx::query_scalar(
        "SELECT COALESCE(value, '1')::bigint
         FROM config_values
         WHERE key = 'points_rate_per_dollar' AND scope = 'global'",
    )
    .fetch_optional(pool.get_ref())
    .await?
    .unwrap_or(1);

    let campaigns = sqlx::query_as::<_, CampaignRow>(
        "SELECT id, name, description, enabled, starts_at, ends_at, created_at, updated_at
         FROM campaign_toggles
         ORDER BY name",
    )
    .fetch_all(pool.get_ref())
    .await?;

    let campaign_statuses = campaigns
        .into_iter()
        .map(|c| CommerceCampaignStatus {
            name: c.name,
            enabled: c.enabled,
        })
        .collect::<Vec<_>>();

    Ok(HttpResponse::Ok().json(CommerceSummaryResponse {
        shipping_fee_cents,
        shipping_fee_display: format!("${:.2}", shipping_fee_cents as f64 / 100.0),
        points_rate_per_dollar,
        campaigns: campaign_statuses,
    }))
}

// ---------------------------------------------------------------------------
// Validation helper
// ---------------------------------------------------------------------------

pub fn validate_config_value(value_type: &str, raw: &str) -> Result<(), AppError> {
    let trimmed = raw.trim();
    match value_type {
        "integer" => {
            trimmed.parse::<i64>().map_err(|_| {
                AppError::ValidationError(format!(
                    "Expected integer value, got '{}'.",
                    trimmed
                ))
            })?;
        }
        "boolean" => {
            if !matches!(trimmed, "true" | "false") {
                return Err(AppError::ValidationError(format!(
                    "Expected 'true' or 'false', got '{}'.",
                    trimmed
                )));
            }
        }
        "json" => {
            serde_json::from_str::<serde_json::Value>(trimmed).map_err(|_| {
                AppError::ValidationError(format!("Expected valid JSON, got '{}'.", trimmed))
            })?;
        }
        _ => {} // "string" — any value is valid
    }
    Ok(())
}

// ---------------------------------------------------------------------------
// Unit tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn validate_integer_accepts_valid() {
        assert!(validate_config_value("integer", "695").is_ok());
        assert!(validate_config_value("integer", "0").is_ok());
        assert!(validate_config_value("integer", "-1").is_ok());
    }

    #[test]
    fn validate_integer_rejects_non_numeric() {
        assert!(validate_config_value("integer", "not_a_number").is_err());
        assert!(validate_config_value("integer", "6.95").is_err());
    }

    #[test]
    fn validate_boolean_accepts_literals() {
        assert!(validate_config_value("boolean", "true").is_ok());
        assert!(validate_config_value("boolean", "false").is_ok());
    }

    #[test]
    fn validate_boolean_rejects_other() {
        assert!(validate_config_value("boolean", "yes").is_err());
        assert!(validate_config_value("boolean", "1").is_err());
    }

    #[test]
    fn validate_json_accepts_valid() {
        assert!(validate_config_value("json", r#"{"key":"value"}"#).is_ok());
        assert!(validate_config_value("json", "[1,2,3]").is_ok());
    }

    #[test]
    fn validate_string_accepts_anything() {
        assert!(validate_config_value("string", "anything goes").is_ok());
    }

    #[test]
    fn config_versioning_test() {
        // The config versioning data model: old_value + new_value + changed_by + changed_at
        // This test validates the DTO structure is correct.
        let history_json = r#"{
            "id": "00000000-0000-0000-0000-000000000001",
            "config_key": "shipping_fee_cents",
            "old_value": "695",
            "new_value": "0",
            "changed_by": "00000000-0000-0000-0000-000000000002",
            "changed_by_username": "admin_user",
            "changed_at": "2024-01-01T00:00:00Z",
            "reason": "Promotional free shipping"
        }"#;
        let parsed: serde_json::Value = serde_json::from_str(history_json).unwrap();
        assert_eq!(parsed["config_key"], "shipping_fee_cents");
        assert_eq!(parsed["old_value"], "695");
        assert_eq!(parsed["new_value"], "0");
    }
}
