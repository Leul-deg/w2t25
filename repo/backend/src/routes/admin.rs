use actix_web::{web, HttpResponse};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::db::DbPool;
use crate::errors::AppError;
use crate::middleware::auth::AuthContext;
use crate::routes::config_routes::configure_admin_config;
use crate::routes::orders::fetch_order_detail;
use crate::routes::products::{
    admin_create_product, admin_deactivate_product, admin_list_products, admin_update_product,
};

// ---------------------------------------------------------------------------
// Route configuration
// ---------------------------------------------------------------------------

pub fn configure(cfg: &mut web::ServiceConfig) {
    cfg.service(
        web::scope("/admin")
            // ── user management ──────────────────────────────────────────
            .route("/users", web::get().to(list_users))
            .route("/users/{user_id}/set-state", web::post().to(set_user_state))
            .route("/deletion-requests", web::get().to(list_deletion_requests))
            .route(
                "/deletion-requests/{request_id}/approve",
                web::post().to(approve_deletion),
            )
            .route(
                "/deletion-requests/{request_id}/reject",
                web::post().to(reject_deletion),
            )
            // ── product management ────────────────────────────────────────
            .route("/products", web::get().to(admin_list_products))
            .route("/products", web::post().to(admin_create_product))
            .route(
                "/products/{id}/update",
                web::post().to(admin_update_product),
            )
            .route(
                "/products/{id}/deactivate",
                web::post().to(admin_deactivate_product),
            )
            // ── order management ──────────────────────────────────────────
            .route("/orders", web::get().to(admin_list_orders))
            .route("/orders/dashboard", web::get().to(admin_orders_dashboard))
            .route("/orders/{id}", web::get().to(admin_get_order))
            .route("/orders/{id}/status", web::post().to(admin_update_order_status))
            // ── KPI dashboard ─────────────────────────────────────────────
            .route("/kpi", web::get().to(admin_kpi))
            // ── config center (sub-scope) ─────────────────────────────────
            .configure(configure_admin_config),
    );
}

// ---------------------------------------------------------------------------
// Request / response types
// ---------------------------------------------------------------------------

#[derive(sqlx::FromRow, Serialize)]
struct UserRow {
    id: Uuid,
    username: String,
    email: String,
    display_name: Option<String>,
    account_state: String,
    created_at: DateTime<Utc>,
    roles: Vec<String>,
}

#[derive(Deserialize)]
struct SetStateBody {
    state: String,
    reason: Option<String>,
}

#[derive(sqlx::FromRow, Serialize)]
struct DeletionRequestRow {
    id: Uuid,
    user_id: Uuid,
    username: String,
    email: String,
    reason: Option<String>,
    status: String,
    requested_at: DateTime<Utc>,
}

#[derive(Deserialize)]
struct RejectBody {
    reason: Option<String>,
}

// ---------------------------------------------------------------------------
// Scope helpers
// ---------------------------------------------------------------------------

/// Returns the set of campus IDs the admin may manage.
///
/// `None`       → admin has `is_super_admin = true`; no restrictions.
/// `Some(ids)`  → admin is restricted to these campuses (district rows are
///               expanded to their constituent campuses).
/// `Some([])`   → admin has `is_super_admin = false` and no scope rows;
///               scoped-by-default means zero access until explicitly assigned.
async fn get_admin_campus_scope(
    pool: &DbPool,
    admin_id: Uuid,
) -> Result<Option<Vec<Uuid>>, AppError> {
    // Check the explicit super-admin flag first.
    let is_super: bool = sqlx::query_scalar(
        "SELECT is_super_admin FROM users WHERE id = $1",
    )
    .bind(admin_id)
    .fetch_one(pool)
    .await?;

    if is_super {
        return Ok(None); // unrestricted
    }

    #[derive(sqlx::FromRow)]
    struct ScopeRow {
        scope_type: String,
        scope_id: Uuid,
    }

    let rows = sqlx::query_as::<_, ScopeRow>(
        "SELECT scope_type, scope_id FROM admin_scope_assignments WHERE admin_id = $1",
    )
    .bind(admin_id)
    .fetch_all(pool)
    .await?;

    if rows.is_empty() {
        // is_super_admin = false with no scope rows → scoped-by-default: no access.
        return Ok(Some(vec![]));
    }

    let mut campus_ids: Vec<Uuid> = Vec::new();
    for row in &rows {
        match row.scope_type.as_str() {
            "campus" => campus_ids.push(row.scope_id),
            "district" => {
                let ids: Vec<Uuid> = sqlx::query_scalar(
                    "SELECT id FROM campuses WHERE district_id = $1",
                )
                .bind(row.scope_id)
                .fetch_all(pool)
                .await?;
                campus_ids.extend(ids);
            }
            _ => {}
        }
    }
    Ok(Some(campus_ids))
}

/// Returns `Forbidden` if `user_id` is not reachable within the admin's scope.
/// Pass `scope = None` (super-admin) to skip the check.
async fn assert_user_in_scope(
    pool: &DbPool,
    scope: &Option<Vec<Uuid>>,
    user_id: Uuid,
) -> Result<(), AppError> {
    let campus_ids = match scope {
        None => return Ok(()),
        Some(ids) => ids,
    };
    let in_scope: bool = sqlx::query_scalar(
        "SELECT EXISTS( \
             SELECT 1 FROM user_school_assignments usa \
             JOIN schools s ON s.id = usa.school_id \
             WHERE usa.user_id = $1 AND s.campus_id = ANY($2) \
         )",
    )
    .bind(user_id)
    .bind(campus_ids.as_slice())
    .fetch_one(pool)
    .await?;
    if !in_scope {
        return Err(AppError::Forbidden(
            "Target user is outside your administrative scope.".into(),
        ));
    }
    Ok(())
}

/// Returns `Forbidden` if the order's owning user is outside the admin's scope.
async fn assert_order_in_scope(
    pool: &DbPool,
    scope: &Option<Vec<Uuid>>,
    order_id: Uuid,
) -> Result<(), AppError> {
    let campus_ids = match scope {
        None => return Ok(()),
        Some(ids) => ids,
    };
    let in_scope: bool = sqlx::query_scalar(
        "SELECT EXISTS( \
             SELECT 1 FROM orders o \
             JOIN user_school_assignments usa ON usa.user_id = o.user_id \
             JOIN schools s ON s.id = usa.school_id \
             WHERE o.id = $1 AND s.campus_id = ANY($2) \
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

// ---------------------------------------------------------------------------
// Handlers
// ---------------------------------------------------------------------------

/// GET /api/v1/admin/users
async fn list_users(
    pool: web::Data<DbPool>,
    auth: AuthContext,
) -> Result<HttpResponse, AppError> {
    auth.require_role("Administrator")?;
    let scope = get_admin_campus_scope(pool.get_ref(), auth.0.user_id).await?;

    let users = match &scope {
        None => sqlx::query_as::<_, UserRow>(
            "SELECT u.id, u.username, u.email, u.display_name, u.account_state, u.created_at,
                    COALESCE(array_agg(r.name) FILTER (WHERE r.name IS NOT NULL), '{}') as roles
             FROM users u
             LEFT JOIN user_roles ur ON u.id = ur.user_id
             LEFT JOIN roles r ON ur.role_id = r.id
             GROUP BY u.id
             ORDER BY u.created_at DESC",
        )
        .fetch_all(pool.get_ref())
        .await?,
        Some(campus_ids) => sqlx::query_as::<_, UserRow>(
            "SELECT u.id, u.username, u.email, u.display_name, u.account_state, u.created_at,
                    COALESCE(array_agg(r.name) FILTER (WHERE r.name IS NOT NULL), '{}') as roles
             FROM users u
             LEFT JOIN user_roles ur ON u.id = ur.user_id
             LEFT JOIN roles r ON ur.role_id = r.id
             WHERE u.id IN (
                 SELECT usa.user_id FROM user_school_assignments usa
                 JOIN schools s ON s.id = usa.school_id
                 WHERE s.campus_id = ANY($1)
             )
             GROUP BY u.id
             ORDER BY u.created_at DESC",
        )
        .bind(campus_ids.as_slice())
        .fetch_all(pool.get_ref())
        .await?,
    };

    Ok(HttpResponse::Ok().json(users))
}

/// POST /api/v1/admin/users/{user_id}/set-state
async fn set_user_state(
    pool: web::Data<DbPool>,
    auth: AuthContext,
    path: web::Path<Uuid>,
    body: web::Json<SetStateBody>,
) -> Result<HttpResponse, AppError> {
    auth.require_role("Administrator")?;

    let target_user_id = path.into_inner();
    let scope = get_admin_campus_scope(pool.get_ref(), auth.0.user_id).await?;
    assert_user_in_scope(pool.get_ref(), &scope, target_user_id).await?;

    // Validate state
    let valid_states = ["active", "disabled", "frozen", "blacklisted"];
    if !valid_states.contains(&body.state.as_str()) {
        return Err(AppError::ValidationError(format!(
            "Invalid state '{}'. Must be one of: active, disabled, frozen, blacklisted",
            body.state
        )));
    }

    // Prevent admin from disabling/blacklisting themselves
    if target_user_id == auth.0.user_id
        && (body.state == "disabled" || body.state == "blacklisted")
    {
        return Err(AppError::Forbidden(
            "You cannot disable or blacklist your own account.".into(),
        ));
    }

    // Load the target user
    let target_user = sqlx::query_as::<_, crate::models::user::User>(
        "SELECT id, username, email, password_hash, display_name, account_state, \
         created_at, updated_at FROM users WHERE id = $1",
    )
    .bind(target_user_id)
    .fetch_optional(pool.get_ref())
    .await?
    .ok_or_else(|| AppError::NotFound("User not found".into()))?;

    let old_state = target_user.account_state.clone();

    // Update account state
    sqlx::query("UPDATE users SET account_state = $1, updated_at = NOW() WHERE id = $2")
        .bind(&body.state)
        .bind(target_user_id)
        .execute(pool.get_ref())
        .await?;

    // If blacklisted, insert into blacklist_entries
    if body.state == "blacklisted" {
        sqlx::query(
            "INSERT INTO blacklist_entries (id, user_id, reason, blacklisted_by, created_at) \
             VALUES ($1, $2, $3, $4, NOW())",
        )
        .bind(Uuid::new_v4())
        .bind(target_user_id)
        .bind(body.reason.as_deref())
        .bind(auth.0.user_id)
        .execute(pool.get_ref())
        .await?;
    }

    // Invalidate all sessions for that user
    sqlx::query("DELETE FROM sessions WHERE user_id = $1")
        .bind(target_user_id)
        .execute(pool.get_ref())
        .await?;

    // Log audit event
    log_audit(
        pool.get_ref(),
        auth.0.user_id,
        "set_user_state",
        "user",
        &target_user_id.to_string(),
        Some(serde_json::json!({ "account_state": old_state })),
        Some(serde_json::json!({
            "account_state": body.state,
            "reason": body.reason
        })),
    )
    .await?;

    Ok(HttpResponse::Ok().json(serde_json::json!({
        "user_id": target_user_id,
        "username": target_user.username,
        "old_state": old_state,
        "new_state": body.state,
        "message": "Account state updated successfully"
    })))
}

/// GET /api/v1/admin/deletion-requests
async fn list_deletion_requests(
    pool: web::Data<DbPool>,
    auth: AuthContext,
) -> Result<HttpResponse, AppError> {
    auth.require_role("Administrator")?;
    let scope = get_admin_campus_scope(pool.get_ref(), auth.0.user_id).await?;

    let requests = match &scope {
        None => sqlx::query_as::<_, DeletionRequestRow>(
            "SELECT adr.id, adr.user_id, u.username, u.email, adr.reason, adr.status, adr.requested_at
             FROM account_deletion_requests adr
             JOIN users u ON adr.user_id = u.id
             WHERE adr.status = 'pending'
             ORDER BY adr.requested_at ASC",
        )
        .fetch_all(pool.get_ref())
        .await?,
        Some(campus_ids) => sqlx::query_as::<_, DeletionRequestRow>(
            "SELECT adr.id, adr.user_id, u.username, u.email, adr.reason, adr.status, adr.requested_at
             FROM account_deletion_requests adr
             JOIN users u ON adr.user_id = u.id
             WHERE adr.status = 'pending'
               AND u.id IN (
                   SELECT usa.user_id FROM user_school_assignments usa
                   JOIN schools s ON s.id = usa.school_id
                   WHERE s.campus_id = ANY($1)
               )
             ORDER BY adr.requested_at ASC",
        )
        .bind(campus_ids.as_slice())
        .fetch_all(pool.get_ref())
        .await?,
    };

    Ok(HttpResponse::Ok().json(requests))
}

/// POST /api/v1/admin/deletion-requests/{request_id}/approve
async fn approve_deletion(
    pool: web::Data<DbPool>,
    auth: AuthContext,
    path: web::Path<Uuid>,
) -> Result<HttpResponse, AppError> {
    auth.require_role("Administrator")?;

    let request_id = path.into_inner();
    let scope = get_admin_campus_scope(pool.get_ref(), auth.0.user_id).await?;

    // Load the deletion request, verify status = 'pending'
    let (user_id, status): (Uuid, String) = sqlx::query_as(
        "SELECT user_id, status FROM account_deletion_requests WHERE id = $1",
    )
    .bind(request_id)
    .fetch_optional(pool.get_ref())
    .await?
    .ok_or_else(|| AppError::NotFound("Deletion request not found".into()))?;

    assert_user_in_scope(pool.get_ref(), &scope, user_id).await?;

    if status != "pending" {
        return Err(AppError::ConflictError(format!(
            "Deletion request is already '{}'",
            status
        )));
    }

    // Update request status
    sqlx::query(
        "UPDATE account_deletion_requests \
         SET status = 'completed', reviewed_by = $1, reviewed_at = NOW() \
         WHERE id = $2",
    )
    .bind(auth.0.user_id)
    .bind(request_id)
    .execute(pool.get_ref())
    .await?;

    // Disable the user's account
    sqlx::query("UPDATE users SET account_state = 'disabled', updated_at = NOW() WHERE id = $1")
        .bind(user_id)
        .execute(pool.get_ref())
        .await?;

    // Invalidate all sessions for that user
    sqlx::query("DELETE FROM sessions WHERE user_id = $1")
        .bind(user_id)
        .execute(pool.get_ref())
        .await?;

    // Log audit event
    log_audit(
        pool.get_ref(),
        auth.0.user_id,
        "deletion_request_approved",
        "account_deletion_request",
        &request_id.to_string(),
        Some(serde_json::json!({ "status": "pending" })),
        Some(serde_json::json!({ "status": "completed" })),
    )
    .await?;

    Ok(HttpResponse::Ok().json(serde_json::json!({
        "message": "Deletion request approved. Account has been disabled."
    })))
}

/// POST /api/v1/admin/deletion-requests/{request_id}/reject
async fn reject_deletion(
    pool: web::Data<DbPool>,
    auth: AuthContext,
    path: web::Path<Uuid>,
    body: web::Json<RejectBody>,
) -> Result<HttpResponse, AppError> {
    auth.require_role("Administrator")?;

    let request_id = path.into_inner();
    let scope = get_admin_campus_scope(pool.get_ref(), auth.0.user_id).await?;

    // Load the deletion request, verify status = 'pending'
    let (user_id, status): (Uuid, String) = sqlx::query_as(
        "SELECT user_id, status FROM account_deletion_requests WHERE id = $1",
    )
    .bind(request_id)
    .fetch_optional(pool.get_ref())
    .await?
    .ok_or_else(|| AppError::NotFound("Deletion request not found".into()))?;

    assert_user_in_scope(pool.get_ref(), &scope, user_id).await?;

    if status != "pending" {
        return Err(AppError::ConflictError(format!(
            "Deletion request is already '{}'",
            status
        )));
    }

    // Update request status
    sqlx::query(
        "UPDATE account_deletion_requests \
         SET status = 'rejected', reviewed_by = $1, reviewed_at = NOW() \
         WHERE id = $2",
    )
    .bind(auth.0.user_id)
    .bind(request_id)
    .execute(pool.get_ref())
    .await?;

    // Log audit event
    log_audit(
        pool.get_ref(),
        auth.0.user_id,
        "deletion_request_rejected",
        "account_deletion_request",
        &request_id.to_string(),
        Some(serde_json::json!({ "status": "pending" })),
        Some(serde_json::json!({
            "status": "rejected",
            "reason": body.reason
        })),
    )
    .await?;

    Ok(HttpResponse::Ok().json(serde_json::json!({
        "message": "Deletion request rejected."
    })))
}

// ---------------------------------------------------------------------------
// Order management handlers
// ---------------------------------------------------------------------------

#[derive(Serialize, sqlx::FromRow)]
struct AdminOrderRow {
    id: Uuid,
    user_id: Uuid,
    username: String,
    status: String,
    total_cents: i32,
    shipping_fee_cents: i32,
    points_earned: i32,
    notes: Option<String>,
    item_count: i64,
    created_at: DateTime<Utc>,
    updated_at: DateTime<Utc>,
}

#[derive(Deserialize)]
struct UpdateOrderStatusBody {
    status: String,
    notes: Option<String>,
}

#[derive(Serialize, sqlx::FromRow)]
struct DashboardLowStockRow {
    product_id: Uuid,
    product_name: String,
    quantity: i32,
    low_stock_threshold: i32,
}

#[derive(Serialize)]
struct OrdersDashboardResponse {
    pending_orders: i64,
    confirmed_orders: i64,
    fulfilled_orders: i64,
    cancelled_orders: i64,
    pending_over_30_min: i64,
    low_stock_products: Vec<DashboardLowStockRow>,
    recent_orders: Vec<AdminOrderRow>,
}

/// GET /api/v1/admin/orders
async fn admin_list_orders(
    pool: web::Data<DbPool>,
    auth: AuthContext,
) -> Result<HttpResponse, AppError> {
    auth.require_role("Administrator")?;
    let scope = get_admin_campus_scope(pool.get_ref(), auth.0.user_id).await?;

    let rows = match &scope {
        None => sqlx::query_as::<_, AdminOrderRow>(
            "SELECT o.id, o.user_id, u.username, o.status, o.total_cents,
                    o.shipping_fee_cents, o.points_earned, o.notes,
                    COUNT(oi.id) AS item_count,
                    o.created_at, o.updated_at
             FROM orders o
             JOIN users u ON u.id = o.user_id
             LEFT JOIN order_items oi ON oi.order_id = o.id
             GROUP BY o.id, u.username
             ORDER BY o.created_at DESC
             LIMIT 500",
        )
        .fetch_all(pool.get_ref())
        .await?,
        Some(campus_ids) => sqlx::query_as::<_, AdminOrderRow>(
            "SELECT o.id, o.user_id, u.username, o.status, o.total_cents,
                    o.shipping_fee_cents, o.points_earned, o.notes,
                    COUNT(oi.id) AS item_count,
                    o.created_at, o.updated_at
             FROM orders o
             JOIN users u ON u.id = o.user_id
             LEFT JOIN order_items oi ON oi.order_id = o.id
             WHERE o.user_id IN (
                 SELECT usa.user_id FROM user_school_assignments usa
                 JOIN schools s ON s.id = usa.school_id
                 WHERE s.campus_id = ANY($1)
             )
             GROUP BY o.id, u.username
             ORDER BY o.created_at DESC
             LIMIT 500",
        )
        .bind(campus_ids.as_slice())
        .fetch_all(pool.get_ref())
        .await?,
    };

    Ok(HttpResponse::Ok().json(rows))
}

/// GET /api/v1/admin/orders/dashboard
///
/// Near-real-time operational summary for the admin console.
async fn admin_orders_dashboard(
    pool: web::Data<DbPool>,
    auth: AuthContext,
) -> Result<HttpResponse, AppError> {
    auth.require_role("Administrator")?;
    let scope = get_admin_campus_scope(pool.get_ref(), auth.0.user_id).await?;

    // Build a reusable scope predicate fragment depending on whether the admin
    // is scoped.  We use separate queries for each branch to keep sqlx happy
    // with static SQL and typed bindings.
    let (
        pending_orders,
        confirmed_orders,
        fulfilled_orders,
        cancelled_orders,
        pending_over_30_min,
        recent_orders,
    ) = match &scope {
        None => {
            let pending: i64 = sqlx::query_scalar(
                "SELECT COUNT(*) FROM orders WHERE status = 'pending'",
            )
            .fetch_one(pool.get_ref())
            .await?;
            let confirmed: i64 = sqlx::query_scalar(
                "SELECT COUNT(*) FROM orders WHERE status = 'confirmed'",
            )
            .fetch_one(pool.get_ref())
            .await?;
            let fulfilled: i64 = sqlx::query_scalar(
                "SELECT COUNT(*) FROM orders WHERE status = 'fulfilled'",
            )
            .fetch_one(pool.get_ref())
            .await?;
            let cancelled: i64 = sqlx::query_scalar(
                "SELECT COUNT(*) FROM orders WHERE status = 'cancelled'",
            )
            .fetch_one(pool.get_ref())
            .await?;
            let pend_30: i64 = sqlx::query_scalar(
                "SELECT COUNT(*) FROM orders
                 WHERE status = 'pending'
                   AND created_at < NOW() - INTERVAL '30 minutes'",
            )
            .fetch_one(pool.get_ref())
            .await?;
            let recent = sqlx::query_as::<_, AdminOrderRow>(
                "SELECT o.id, o.user_id, u.username, o.status, o.total_cents,
                        o.shipping_fee_cents, o.points_earned, o.notes,
                        COUNT(oi.id) AS item_count,
                        o.created_at, o.updated_at
                 FROM orders o
                 JOIN users u ON u.id = o.user_id
                 LEFT JOIN order_items oi ON oi.order_id = o.id
                 GROUP BY o.id, u.username
                 ORDER BY o.created_at DESC
                 LIMIT 10",
            )
            .fetch_all(pool.get_ref())
            .await?;
            (pending, confirmed, fulfilled, cancelled, pend_30, recent)
        }
        Some(campus_ids) => {
            let pending: i64 = sqlx::query_scalar(
                "SELECT COUNT(*) FROM orders
                 WHERE status = 'pending'
                   AND user_id IN (
                       SELECT usa.user_id FROM user_school_assignments usa
                       JOIN schools s ON s.id = usa.school_id
                       WHERE s.campus_id = ANY($1)
                   )",
            )
            .bind(campus_ids.as_slice())
            .fetch_one(pool.get_ref())
            .await?;
            let confirmed: i64 = sqlx::query_scalar(
                "SELECT COUNT(*) FROM orders
                 WHERE status = 'confirmed'
                   AND user_id IN (
                       SELECT usa.user_id FROM user_school_assignments usa
                       JOIN schools s ON s.id = usa.school_id
                       WHERE s.campus_id = ANY($1)
                   )",
            )
            .bind(campus_ids.as_slice())
            .fetch_one(pool.get_ref())
            .await?;
            let fulfilled: i64 = sqlx::query_scalar(
                "SELECT COUNT(*) FROM orders
                 WHERE status = 'fulfilled'
                   AND user_id IN (
                       SELECT usa.user_id FROM user_school_assignments usa
                       JOIN schools s ON s.id = usa.school_id
                       WHERE s.campus_id = ANY($1)
                   )",
            )
            .bind(campus_ids.as_slice())
            .fetch_one(pool.get_ref())
            .await?;
            let cancelled: i64 = sqlx::query_scalar(
                "SELECT COUNT(*) FROM orders
                 WHERE status = 'cancelled'
                   AND user_id IN (
                       SELECT usa.user_id FROM user_school_assignments usa
                       JOIN schools s ON s.id = usa.school_id
                       WHERE s.campus_id = ANY($1)
                   )",
            )
            .bind(campus_ids.as_slice())
            .fetch_one(pool.get_ref())
            .await?;
            let pend_30: i64 = sqlx::query_scalar(
                "SELECT COUNT(*) FROM orders
                 WHERE status = 'pending'
                   AND created_at < NOW() - INTERVAL '30 minutes'
                   AND user_id IN (
                       SELECT usa.user_id FROM user_school_assignments usa
                       JOIN schools s ON s.id = usa.school_id
                       WHERE s.campus_id = ANY($1)
                   )",
            )
            .bind(campus_ids.as_slice())
            .fetch_one(pool.get_ref())
            .await?;
            let recent = sqlx::query_as::<_, AdminOrderRow>(
                "SELECT o.id, o.user_id, u.username, o.status, o.total_cents,
                        o.shipping_fee_cents, o.points_earned, o.notes,
                        COUNT(oi.id) AS item_count,
                        o.created_at, o.updated_at
                 FROM orders o
                 JOIN users u ON u.id = o.user_id
                 LEFT JOIN order_items oi ON oi.order_id = o.id
                 WHERE o.user_id IN (
                     SELECT usa.user_id FROM user_school_assignments usa
                     JOIN schools s ON s.id = usa.school_id
                     WHERE s.campus_id = ANY($1)
                 )
                 GROUP BY o.id, u.username
                 ORDER BY o.created_at DESC
                 LIMIT 10",
            )
            .bind(campus_ids.as_slice())
            .fetch_all(pool.get_ref())
            .await?;
            (pending, confirmed, fulfilled, cancelled, pend_30, recent)
        }
    };

    let low_stock_products = sqlx::query_as::<_, DashboardLowStockRow>(
        "SELECT p.id AS product_id, p.name AS product_name, i.quantity, i.low_stock_threshold
         FROM inventory i
         JOIN products p ON p.id = i.product_id
         WHERE p.active = TRUE
           AND i.quantity < i.low_stock_threshold
         ORDER BY i.quantity ASC, p.name ASC
         LIMIT 20",
    )
    .fetch_all(pool.get_ref())
    .await?;

    Ok(HttpResponse::Ok().json(OrdersDashboardResponse {
        pending_orders,
        confirmed_orders,
        fulfilled_orders,
        cancelled_orders,
        pending_over_30_min,
        low_stock_products,
        recent_orders,
    }))
}

/// GET /api/v1/admin/orders/{id}
async fn admin_get_order(
    pool: web::Data<DbPool>,
    auth: AuthContext,
    path: web::Path<Uuid>,
) -> Result<HttpResponse, AppError> {
    auth.require_role("Administrator")?;
    let order_id = path.into_inner();
    let scope = get_admin_campus_scope(pool.get_ref(), auth.0.user_id).await?;
    assert_order_in_scope(pool.get_ref(), &scope, order_id).await?;
    let detail = fetch_order_detail(pool.get_ref(), order_id).await?;
    Ok(HttpResponse::Ok().json(detail))
}

/// POST /api/v1/admin/orders/{id}/status
async fn admin_update_order_status(
    pool: web::Data<DbPool>,
    auth: AuthContext,
    path: web::Path<Uuid>,
    body: web::Json<UpdateOrderStatusBody>,
) -> Result<HttpResponse, AppError> {
    auth.require_role("Administrator")?;
    let order_id = path.into_inner();
    let scope = get_admin_campus_scope(pool.get_ref(), auth.0.user_id).await?;
    assert_order_in_scope(pool.get_ref(), &scope, order_id).await?;

    let valid_statuses = ["pending", "confirmed", "fulfilled", "cancelled", "refunded"];
    if !valid_statuses.contains(&body.status.as_str()) {
        return Err(AppError::ValidationError(format!(
            "Invalid status '{}'. Must be one of: {}",
            body.status,
            valid_statuses.join(", ")
        )));
    }

    // Load current status.
    let (current_status, user_id): (String, Uuid) = sqlx::query_as(
        "SELECT status, user_id FROM orders WHERE id = $1",
    )
    .bind(order_id)
    .fetch_optional(pool.get_ref())
    .await?
    .ok_or_else(|| AppError::NotFound(format!("Order {} not found.", order_id)))?;

    if current_status == body.status {
        return Err(AppError::ConflictError(format!(
            "Order is already '{}'.",
            body.status
        )));
    }

    // Disallow moving out of terminal states.
    if matches!(current_status.as_str(), "cancelled" | "refunded") {
        return Err(AppError::ConflictError(format!(
            "Cannot change status of a '{}' order.",
            current_status
        )));
    }

    sqlx::query(
        "UPDATE orders SET status = $1, updated_at = NOW(),
         fulfilled_at = CASE WHEN $1 = 'fulfilled' THEN NOW() ELSE fulfilled_at END,
         fulfilled_by = CASE WHEN $1 = 'fulfilled' THEN $2 ELSE fulfilled_by END
         WHERE id = $3",
    )
    .bind(&body.status)
    .bind(auth.0.user_id)
    .bind(order_id)
    .execute(pool.get_ref())
    .await?;

    // Customer notification.
    let subject = format!("Order {} status update", &order_id.to_string()[..8]);
    let notif_body = format!(
        "Your order status has been updated to: {}.",
        body.status
    );
    let _ = sqlx::query(
        "INSERT INTO notifications
             (id, recipient_id, subject, body, notification_type, created_at)
         VALUES ($1, $2, $3, $4, 'order', NOW())",
    )
    .bind(Uuid::new_v4())
    .bind(user_id)
    .bind(&subject)
    .bind(&notif_body)
    .execute(pool.get_ref())
    .await;

    log_audit(
        pool.get_ref(),
        auth.0.user_id,
        "update_order_status",
        "order",
        &order_id.to_string(),
        Some(serde_json::json!({ "status": current_status })),
        Some(serde_json::json!({ "status": body.status })),
    )
    .await?;

    Ok(HttpResponse::Ok().json(serde_json::json!({
        "order_id": order_id,
        "old_status": current_status,
        "new_status": body.status,
        "message": "Order status updated."
    })))
}

// ---------------------------------------------------------------------------
// KPI dashboard handler
// ---------------------------------------------------------------------------

#[derive(Serialize)]
struct KpiResponse {
    /// Sum of total_cents for confirmed + fulfilled orders created today (UTC).
    daily_sales_cents: i64,
    /// Average total_cents for confirmed + fulfilled orders in last 30 days.
    average_order_value_cents: i64,
    /// Fraction of buyers who have placed more than one order in last 30 days.
    /// Expressed as a percentage (0–100), two decimal places.
    repeat_purchase_rate_pct: f64,
    /// Number of orders in last 30 days (confirmed + fulfilled).
    orders_last_30d: i64,
    /// Number of unique buyers in last 30 days.
    buyers_last_30d: i64,
    /// Number of repeat buyers (>1 order) in last 30 days.
    repeat_buyers_last_30d: i64,
}

/// GET /api/v1/admin/kpi
///
/// KPI definitions:
///   daily_sales_cents    – SUM(total_cents) for confirmed|fulfilled orders, UTC today
///   average_order_value  – AVG(total_cents) for confirmed|fulfilled orders, last 30 days
///   repeat_purchase_rate – (users with >1 order / users with any order) × 100, last 30 days
async fn admin_kpi(
    pool: web::Data<DbPool>,
    auth: AuthContext,
) -> Result<HttpResponse, AppError> {
    auth.require_role("Administrator")?;
    let scope = get_admin_campus_scope(pool.get_ref(), auth.0.user_id).await?;

    let (daily_sales_cents, average_order_value_cents, orders_last_30d, buyers_last_30d, repeat_buyers_last_30d) =
        match &scope {
            None => {
                let daily: i64 = sqlx::query_scalar(
                    "SELECT COALESCE(SUM(total_cents), 0)
                     FROM orders
                     WHERE status IN ('confirmed', 'fulfilled')
                       AND created_at >= date_trunc('day', NOW() AT TIME ZONE 'UTC')",
                )
                .fetch_one(pool.get_ref())
                .await?;
                let avg: i64 = sqlx::query_scalar(
                    "SELECT COALESCE(AVG(total_cents)::bigint, 0)
                     FROM orders
                     WHERE status IN ('confirmed', 'fulfilled')
                       AND created_at >= NOW() - INTERVAL '30 days'",
                )
                .fetch_one(pool.get_ref())
                .await?;
                let orders: i64 = sqlx::query_scalar(
                    "SELECT COUNT(*) FROM orders
                     WHERE status IN ('confirmed', 'fulfilled')
                       AND created_at >= NOW() - INTERVAL '30 days'",
                )
                .fetch_one(pool.get_ref())
                .await?;
                let buyers: i64 = sqlx::query_scalar(
                    "SELECT COUNT(DISTINCT user_id) FROM orders
                     WHERE status IN ('confirmed', 'fulfilled')
                       AND created_at >= NOW() - INTERVAL '30 days'",
                )
                .fetch_one(pool.get_ref())
                .await?;
                let repeat: i64 = sqlx::query_scalar(
                    "SELECT COUNT(*) FROM (
                         SELECT user_id FROM orders
                         WHERE status IN ('confirmed', 'fulfilled')
                           AND created_at >= NOW() - INTERVAL '30 days'
                         GROUP BY user_id HAVING COUNT(*) > 1
                     ) sub",
                )
                .fetch_one(pool.get_ref())
                .await?;
                (daily, avg, orders, buyers, repeat)
            }
            Some(campus_ids) => {
                let daily: i64 = sqlx::query_scalar(
                    "SELECT COALESCE(SUM(total_cents), 0)
                     FROM orders
                     WHERE status IN ('confirmed', 'fulfilled')
                       AND created_at >= date_trunc('day', NOW() AT TIME ZONE 'UTC')
                       AND user_id IN (
                           SELECT usa.user_id FROM user_school_assignments usa
                           JOIN schools s ON s.id = usa.school_id
                           WHERE s.campus_id = ANY($1)
                       )",
                )
                .bind(campus_ids.as_slice())
                .fetch_one(pool.get_ref())
                .await?;
                let avg: i64 = sqlx::query_scalar(
                    "SELECT COALESCE(AVG(total_cents)::bigint, 0)
                     FROM orders
                     WHERE status IN ('confirmed', 'fulfilled')
                       AND created_at >= NOW() - INTERVAL '30 days'
                       AND user_id IN (
                           SELECT usa.user_id FROM user_school_assignments usa
                           JOIN schools s ON s.id = usa.school_id
                           WHERE s.campus_id = ANY($1)
                       )",
                )
                .bind(campus_ids.as_slice())
                .fetch_one(pool.get_ref())
                .await?;
                let orders: i64 = sqlx::query_scalar(
                    "SELECT COUNT(*) FROM orders
                     WHERE status IN ('confirmed', 'fulfilled')
                       AND created_at >= NOW() - INTERVAL '30 days'
                       AND user_id IN (
                           SELECT usa.user_id FROM user_school_assignments usa
                           JOIN schools s ON s.id = usa.school_id
                           WHERE s.campus_id = ANY($1)
                       )",
                )
                .bind(campus_ids.as_slice())
                .fetch_one(pool.get_ref())
                .await?;
                let buyers: i64 = sqlx::query_scalar(
                    "SELECT COUNT(DISTINCT user_id) FROM orders
                     WHERE status IN ('confirmed', 'fulfilled')
                       AND created_at >= NOW() - INTERVAL '30 days'
                       AND user_id IN (
                           SELECT usa.user_id FROM user_school_assignments usa
                           JOIN schools s ON s.id = usa.school_id
                           WHERE s.campus_id = ANY($1)
                       )",
                )
                .bind(campus_ids.as_slice())
                .fetch_one(pool.get_ref())
                .await?;
                let repeat: i64 = sqlx::query_scalar(
                    "SELECT COUNT(*) FROM (
                         SELECT user_id FROM orders
                         WHERE status IN ('confirmed', 'fulfilled')
                           AND created_at >= NOW() - INTERVAL '30 days'
                           AND user_id IN (
                               SELECT usa.user_id FROM user_school_assignments usa
                               JOIN schools s ON s.id = usa.school_id
                               WHERE s.campus_id = ANY($1)
                           )
                         GROUP BY user_id HAVING COUNT(*) > 1
                     ) sub",
                )
                .bind(campus_ids.as_slice())
                .fetch_one(pool.get_ref())
                .await?;
                (daily, avg, orders, buyers, repeat)
            }
        };

    let repeat_purchase_rate_pct = if buyers_last_30d > 0 {
        let rate = (repeat_buyers_last_30d as f64 / buyers_last_30d as f64) * 100.0;
        (rate * 100.0).round() / 100.0
    } else {
        0.0
    };

    Ok(HttpResponse::Ok().json(KpiResponse {
        daily_sales_cents,
        average_order_value_cents,
        repeat_purchase_rate_pct,
        orders_last_30d,
        buyers_last_30d,
        repeat_buyers_last_30d,
    }))
}

// ---------------------------------------------------------------------------
// Audit log helper
// ---------------------------------------------------------------------------

async fn log_audit(
    pool: &DbPool,
    actor_id: Uuid,
    action: &str,
    entity_type: &str,
    entity_id: &str,
    old_data: Option<serde_json::Value>,
    new_data: Option<serde_json::Value>,
) -> Result<(), AppError> {
    sqlx::query(
        "INSERT INTO audit_logs (id, actor_id, action, entity_type, entity_id, old_data, new_data, created_at)
         VALUES ($1, $2, $3, $4, $5, $6, $7, NOW())",
    )
    .bind(Uuid::new_v4())
    .bind(actor_id)
    .bind(action)
    .bind(entity_type)
    .bind(entity_id)
    .bind(old_data)
    .bind(new_data)
    .execute(pool)
    .await?;
    Ok(())
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use actix_web::test::{call_service, init_service, read_body_json, TestRequest};
    use actix_web::{web, App};
    use chrono::Utc;
    use serde_json::{json, Value};
    use sqlx::postgres::PgPoolOptions;
    use sqlx::PgPool;

    // ── Helpers ───────────────────────────────────────────────────────────────

    async fn test_pool() -> PgPool {
        let url = std::env::var("DATABASE_URL")
            .expect("DATABASE_URL must be set to run admin integration tests");
        let pool = PgPoolOptions::new()
            .max_connections(5)
            .connect(&url)
            .await
            .expect("Failed to connect to test database");
        sqlx::migrate!("../migrations")
            .run(&pool)
            .await
            .expect("Migration failed");
        pool
    }

    async fn seed_user(pool: &PgPool, username: &str, role: &str) -> Uuid {
        let hash = crate::services::auth::hash_password("TestPass2024!!").expect("hash");
        sqlx::query(
            "INSERT INTO users (id, username, email, password_hash, account_state,
                                is_super_admin, created_at, updated_at)
             VALUES (gen_random_uuid(), $1, $2, $3, 'active', false, NOW(), NOW())
             ON CONFLICT (username) DO UPDATE
               SET password_hash = EXCLUDED.password_hash,
                   account_state = 'active',
                   is_super_admin = false,
                   updated_at = NOW()",
        )
        .bind(username)
        .bind(format!("{}@test.local", username))
        .bind(&hash)
        .execute(pool)
        .await
        .expect("seed_user failed");

        let id: Uuid = sqlx::query_scalar("SELECT id FROM users WHERE username = $1")
            .bind(username)
            .fetch_one(pool)
            .await
            .expect("user not found");

        let role_id: i32 = sqlx::query_scalar("SELECT id FROM roles WHERE name = $1")
            .bind(role)
            .fetch_one(pool)
            .await
            .expect("role not found");

        sqlx::query(
            "INSERT INTO user_roles (user_id, role_id) VALUES ($1, $2) ON CONFLICT DO NOTHING",
        )
        .bind(id)
        .bind(role_id)
        .execute(pool)
        .await
        .expect("user_roles failed");

        id
    }

    async fn make_super_admin(pool: &PgPool, user_id: Uuid) {
        sqlx::query("UPDATE users SET is_super_admin = true WHERE id = $1")
            .bind(user_id)
            .execute(pool)
            .await
            .expect("set is_super_admin failed");
    }

    async fn seed_campus(pool: &PgPool, suffix: &str) -> Uuid {
        let district_id = Uuid::new_v4();
        sqlx::query(
            "INSERT INTO districts (id, name, state, created_at) VALUES ($1, $2, 'TX', NOW())",
        )
        .bind(district_id)
        .bind(format!("District_{}", suffix))
        .execute(pool)
        .await
        .expect("seed district failed");

        let campus_id = Uuid::new_v4();
        sqlx::query(
            "INSERT INTO campuses (id, district_id, name, created_at) VALUES ($1, $2, $3, NOW())",
        )
        .bind(campus_id)
        .bind(district_id)
        .bind(format!("Campus_{}", suffix))
        .execute(pool)
        .await
        .expect("seed campus failed");

        campus_id
    }

    async fn seed_school(pool: &PgPool, campus_id: Uuid, suffix: &str) -> Uuid {
        let school_id = Uuid::new_v4();
        sqlx::query(
            "INSERT INTO schools (id, campus_id, name, school_type, created_at)
             VALUES ($1, $2, $3, 'general', NOW())",
        )
        .bind(school_id)
        .bind(campus_id)
        .bind(format!("School_{}", suffix))
        .execute(pool)
        .await
        .expect("seed school failed");
        school_id
    }

    async fn assign_user_to_school(pool: &PgPool, user_id: Uuid, school_id: Uuid) {
        sqlx::query(
            "INSERT INTO user_school_assignments (user_id, school_id, assignment_type, assigned_at)
             VALUES ($1, $2, 'student', NOW()) ON CONFLICT DO NOTHING",
        )
        .bind(user_id)
        .bind(school_id)
        .execute(pool)
        .await
        .expect("assign failed");
    }

    async fn assign_admin_to_campus(pool: &PgPool, admin_id: Uuid, campus_id: Uuid) {
        sqlx::query(
            "INSERT INTO admin_scope_assignments (admin_id, scope_type, scope_id)
             VALUES ($1, 'campus', $2) ON CONFLICT DO NOTHING",
        )
        .bind(admin_id)
        .bind(campus_id)
        .execute(pool)
        .await
        .expect("assign admin failed");
    }

    async fn seed_product_and_order(pool: &PgPool, user_id: Uuid, suffix: &str) -> Uuid {
        let product_id = Uuid::new_v4();
        sqlx::query(
            "INSERT INTO products (id, name, price_cents, sku, category, active)
             VALUES ($1, $2, 100, $3, 'Test', true) ON CONFLICT (sku) DO NOTHING",
        )
        .bind(product_id)
        .bind(format!("Prod_{}", suffix))
        .bind(format!("SKU-{}", suffix))
        .execute(pool)
        .await
        .expect("seed product failed");
        sqlx::query(
            "INSERT INTO inventory (product_id, quantity, low_stock_threshold)
             VALUES ($1, 50, 5) ON CONFLICT (product_id) DO UPDATE SET quantity = 50",
        )
        .bind(product_id)
        .execute(pool)
        .await
        .expect("seed inventory failed");

        let order_id = Uuid::new_v4();
        sqlx::query(
            "INSERT INTO orders (id, user_id, status, total_cents, points_earned,
                                 created_at, updated_at)
             VALUES ($1, $2, 'pending', 795, 1, NOW(), NOW())",
        )
        .bind(order_id)
        .bind(user_id)
        .execute(pool)
        .await
        .expect("seed order failed");
        sqlx::query(
            "INSERT INTO order_items (id, order_id, product_id, quantity, unit_price_cents)
             VALUES (gen_random_uuid(), $1, $2, 1, 100)",
        )
        .bind(order_id)
        .bind(product_id)
        .execute(pool)
        .await
        .expect("seed order_item failed");
        order_id
    }

    async fn login_as(
        app: &impl actix_web::dev::Service<
            actix_http::Request,
            Response = actix_web::dev::ServiceResponse,
            Error = actix_web::Error,
        >,
        username: &str,
    ) -> String {
        let req = TestRequest::post()
            .uri("/api/v1/auth/login")
            .set_json(json!({ "username": username, "password": "TestPass2024!!" }))
            .to_request();
        let resp = call_service(app, req).await;
        let body: Value = read_body_json(resp).await;
        body["token"]
            .as_str()
            .expect("login did not return token")
            .to_string()
    }

    // ── Tests ─────────────────────────────────────────────────────────────────

    /// Scoped admin gets 403 when calling set_user_state on a user outside their
    /// campus; gets 200 for a user inside their campus; super-admin gets 200 for both.
    #[actix_web::test]
    #[ignore = "requires DATABASE_URL"]
    async fn test_set_user_state_blocked_outside_scope() {
        let pool = test_pool().await;
        let s = Uuid::new_v4().to_string()[..8].to_string();

        let campus_a = seed_campus(&pool, &format!("a_{}", s)).await;
        let campus_b = seed_campus(&pool, &format!("b_{}", s)).await;
        let school_a = seed_school(&pool, campus_a, &format!("a_{}", s)).await;
        let school_b = seed_school(&pool, campus_b, &format!("b_{}", s)).await;

        let scoped = seed_user(&pool, &format!("scoped_{}", s), "Administrator").await;
        assign_admin_to_campus(&pool, scoped, campus_a).await;

        let supe = seed_user(&pool, &format!("super_{}", s), "Administrator").await;
        make_super_admin(&pool, supe).await;

        let target_a = seed_user(&pool, &format!("ta_{}", s), "Student").await;
        assign_user_to_school(&pool, target_a, school_a).await;

        let target_b = seed_user(&pool, &format!("tb_{}", s), "Student").await;
        assign_user_to_school(&pool, target_b, school_b).await;

        let app = init_service(
            App::new()
                .app_data(web::Data::new(pool.clone()))
                .configure(crate::routes::configure_routes),
        )
        .await;

        let scoped_tok = login_as(&app, &format!("scoped_{}", s)).await;
        let super_tok  = login_as(&app, &format!("super_{}", s)).await;

        // Scoped → in-scope user: 200.
        let resp = call_service(
            &app,
            TestRequest::post()
                .uri(&format!("/api/v1/admin/users/{}/set-state", target_a))
                .insert_header(("Authorization", format!("Bearer {}", scoped_tok)))
                .set_json(json!({ "state": "active" }))
                .to_request(),
        )
        .await;
        assert_eq!(resp.status(), 200, "scoped admin must succeed on in-scope user");

        // Scoped → out-of-scope user: 403.
        let resp = call_service(
            &app,
            TestRequest::post()
                .uri(&format!("/api/v1/admin/users/{}/set-state", target_b))
                .insert_header(("Authorization", format!("Bearer {}", scoped_tok)))
                .set_json(json!({ "state": "active" }))
                .to_request(),
        )
        .await;
        assert_eq!(resp.status(), 403, "scoped admin must be blocked for out-of-scope user");

        // Super-admin → out-of-scope user: 200.
        let resp = call_service(
            &app,
            TestRequest::post()
                .uri(&format!("/api/v1/admin/users/{}/set-state", target_b))
                .insert_header(("Authorization", format!("Bearer {}", super_tok)))
                .set_json(json!({ "state": "active" }))
                .to_request(),
        )
        .await;
        assert_eq!(resp.status(), 200, "super-admin must succeed on any user");
    }

    /// Scoped admin gets 403 when calling GET /admin/orders/{id} for an order
    /// whose owner is in another campus; super-admin gets 200.
    #[actix_web::test]
    #[ignore = "requires DATABASE_URL"]
    async fn test_get_order_blocked_outside_scope() {
        let pool = test_pool().await;
        let s = Uuid::new_v4().to_string()[..8].to_string();

        let campus_a = seed_campus(&pool, &format!("oa_{}", s)).await;
        let campus_b = seed_campus(&pool, &format!("ob_{}", s)).await;
        let school_a = seed_school(&pool, campus_a, &format!("oa_{}", s)).await;
        let school_b = seed_school(&pool, campus_b, &format!("ob_{}", s)).await;

        let scoped = seed_user(&pool, &format!("oadm_{}", s), "Administrator").await;
        assign_admin_to_campus(&pool, scoped, campus_a).await;

        let supe = seed_user(&pool, &format!("osuper_{}", s), "Administrator").await;
        make_super_admin(&pool, supe).await;

        let buyer_a = seed_user(&pool, &format!("buyer_a_{}", s), "Student").await;
        assign_user_to_school(&pool, buyer_a, school_a).await;
        let order_a = seed_product_and_order(&pool, buyer_a, &format!("oa_{}", s)).await;

        let buyer_b = seed_user(&pool, &format!("buyer_b_{}", s), "Student").await;
        assign_user_to_school(&pool, buyer_b, school_b).await;
        let order_b = seed_product_and_order(&pool, buyer_b, &format!("ob_{}", s)).await;

        let app = init_service(
            App::new()
                .app_data(web::Data::new(pool.clone()))
                .configure(crate::routes::configure_routes),
        )
        .await;

        let scoped_tok = login_as(&app, &format!("oadm_{}", s)).await;
        let super_tok  = login_as(&app, &format!("osuper_{}", s)).await;

        // Scoped → in-scope order: 200.
        let resp = call_service(
            &app,
            TestRequest::get()
                .uri(&format!("/api/v1/admin/orders/{}", order_a))
                .insert_header(("Authorization", format!("Bearer {}", scoped_tok)))
                .to_request(),
        )
        .await;
        assert_eq!(resp.status(), 200, "scoped admin must see in-scope order");

        // Scoped → out-of-scope order: 403.
        let resp = call_service(
            &app,
            TestRequest::get()
                .uri(&format!("/api/v1/admin/orders/{}", order_b))
                .insert_header(("Authorization", format!("Bearer {}", scoped_tok)))
                .to_request(),
        )
        .await;
        assert_eq!(resp.status(), 403, "scoped admin must be blocked for out-of-scope order");

        // Super-admin → out-of-scope order: 200.
        let resp = call_service(
            &app,
            TestRequest::get()
                .uri(&format!("/api/v1/admin/orders/{}", order_b))
                .insert_header(("Authorization", format!("Bearer {}", super_tok)))
                .to_request(),
        )
        .await;
        assert_eq!(resp.status(), 200, "super-admin must see any order");
    }

    /// An admin with is_super_admin=false and no scope rows gets an empty user
    /// list (scoped-by-default: zero access until explicitly assigned).
    #[actix_web::test]
    #[ignore = "requires DATABASE_URL"]
    async fn test_no_scope_rows_yields_empty_user_list() {
        let pool = test_pool().await;
        let s = Uuid::new_v4().to_string()[..8].to_string();

        // Admin with is_super_admin=false and no scope rows.
        let _adm = seed_user(&pool, &format!("zero_{}", s), "Administrator").await;

        let app = init_service(
            App::new()
                .app_data(web::Data::new(pool.clone()))
                .configure(crate::routes::configure_routes),
        )
        .await;

        let token = login_as(&app, &format!("zero_{}", s)).await;

        let resp = call_service(
            &app,
            TestRequest::get()
                .uri("/api/v1/admin/users")
                .insert_header(("Authorization", format!("Bearer {}", token)))
                .to_request(),
        )
        .await;
        assert_eq!(resp.status(), 200);
        let body: Value = read_body_json(resp).await;
        let users = body.as_array().expect("expected array");
        assert!(
            users.is_empty(),
            "is_super_admin=false + no scope rows must return empty list, got {}",
            users.len()
        );
    }

    /// Create a pending deletion request for `user_id` and return the request UUID.
    async fn seed_deletion_request(pool: &PgPool, user_id: Uuid) -> Uuid {
        let req_id = Uuid::new_v4();
        sqlx::query(
            "INSERT INTO account_deletion_requests (id, user_id, reason, status, requested_at)
             VALUES ($1, $2, 'integration test', 'pending', NOW())",
        )
        .bind(req_id)
        .bind(user_id)
        .execute(pool)
        .await
        .expect("seed_deletion_request failed");
        req_id
    }

    /// Scoped admin is blocked (403) when approving a deletion request for a user
    /// outside their campus; allowed (200) for a user inside their campus.
    #[actix_web::test]
    #[ignore = "requires DATABASE_URL"]
    async fn test_approve_deletion_blocked_outside_scope() {
        let pool = test_pool().await;
        let s = Uuid::new_v4().to_string()[..8].to_string();

        let campus_a = seed_campus(&pool, &format!("appdel_a_{}", s)).await;
        let campus_b = seed_campus(&pool, &format!("appdel_b_{}", s)).await;
        let school_a = seed_school(&pool, campus_a, &format!("appdel_a_{}", s)).await;
        let school_b = seed_school(&pool, campus_b, &format!("appdel_b_{}", s)).await;

        // Scoped admin assigned to campus_a only.
        let admin = seed_user(&pool, &format!("appdel_adm_{}", s), "Administrator").await;
        assign_admin_to_campus(&pool, admin, campus_a).await;

        // Target user lives in campus_b (out of scope).
        let target_out = seed_user(&pool, &format!("appdel_out_{}", s), "Student").await;
        assign_user_to_school(&pool, target_out, school_b).await;
        let req_out = seed_deletion_request(&pool, target_out).await;

        // Target user lives in campus_a (in scope).
        let target_in = seed_user(&pool, &format!("appdel_in_{}", s), "Student").await;
        assign_user_to_school(&pool, target_in, school_a).await;
        let req_in = seed_deletion_request(&pool, target_in).await;

        let app = init_service(
            App::new()
                .app_data(web::Data::new(pool.clone()))
                .configure(crate::routes::configure_routes),
        )
        .await;
        let token = login_as(&app, &format!("appdel_adm_{}", s)).await;

        // Out-of-scope → 403.
        let resp = call_service(
            &app,
            TestRequest::post()
                .uri(&format!("/api/v1/admin/deletion-requests/{}/approve", req_out))
                .insert_header(("Authorization", format!("Bearer {}", token)))
                .to_request(),
        )
        .await;
        assert_eq!(
            resp.status(), 403,
            "approve_deletion must return 403 for out-of-scope user"
        );

        // In-scope → 200.
        let resp = call_service(
            &app,
            TestRequest::post()
                .uri(&format!("/api/v1/admin/deletion-requests/{}/approve", req_in))
                .insert_header(("Authorization", format!("Bearer {}", token)))
                .to_request(),
        )
        .await;
        assert_eq!(
            resp.status(), 200,
            "approve_deletion must return 200 for in-scope user"
        );
    }

    /// Scoped admin is blocked (403) when rejecting a deletion request for a user
    /// outside their campus; allowed (200) for a user inside their campus.
    #[actix_web::test]
    #[ignore = "requires DATABASE_URL"]
    async fn test_reject_deletion_blocked_outside_scope() {
        let pool = test_pool().await;
        let s = Uuid::new_v4().to_string()[..8].to_string();

        let campus_a = seed_campus(&pool, &format!("rejdel_a_{}", s)).await;
        let campus_b = seed_campus(&pool, &format!("rejdel_b_{}", s)).await;
        let school_a = seed_school(&pool, campus_a, &format!("rejdel_a_{}", s)).await;
        let school_b = seed_school(&pool, campus_b, &format!("rejdel_b_{}", s)).await;

        // Scoped admin assigned to campus_a only.
        let admin = seed_user(&pool, &format!("rejdel_adm_{}", s), "Administrator").await;
        assign_admin_to_campus(&pool, admin, campus_a).await;

        // Target user lives in campus_b (out of scope).
        let target_out = seed_user(&pool, &format!("rejdel_out_{}", s), "Student").await;
        assign_user_to_school(&pool, target_out, school_b).await;
        let req_out = seed_deletion_request(&pool, target_out).await;

        // Target user lives in campus_a (in scope).
        let target_in = seed_user(&pool, &format!("rejdel_in_{}", s), "Student").await;
        assign_user_to_school(&pool, target_in, school_a).await;
        let req_in = seed_deletion_request(&pool, target_in).await;

        let app = init_service(
            App::new()
                .app_data(web::Data::new(pool.clone()))
                .configure(crate::routes::configure_routes),
        )
        .await;
        let token = login_as(&app, &format!("rejdel_adm_{}", s)).await;

        let reject_body = json!({ "reason": "test rejection" });

        // Out-of-scope → 403.
        let resp = call_service(
            &app,
            TestRequest::post()
                .uri(&format!("/api/v1/admin/deletion-requests/{}/reject", req_out))
                .insert_header(("Authorization", format!("Bearer {}", token)))
                .set_json(&reject_body)
                .to_request(),
        )
        .await;
        assert_eq!(
            resp.status(), 403,
            "reject_deletion must return 403 for out-of-scope user"
        );

        // In-scope → 200.
        let resp = call_service(
            &app,
            TestRequest::post()
                .uri(&format!("/api/v1/admin/deletion-requests/{}/reject", req_in))
                .insert_header(("Authorization", format!("Bearer {}", token)))
                .set_json(&reject_body)
                .to_request(),
        )
        .await;
        assert_eq!(
            resp.status(), 200,
            "reject_deletion must return 200 for in-scope user"
        );
    }

    /// A scoped admin only sees users whose campus is in their scope list.
    /// A user seeded in a different campus must not appear in the response.
    #[actix_web::test]
    #[ignore = "requires DATABASE_URL"]
    async fn test_list_users_scoped_by_campus() {
        let pool = test_pool().await;
        let s = Uuid::new_v4().to_string()[..8].to_string();

        let campus_a = seed_campus(&pool, &format!("luca_{}", s)).await;
        let campus_b = seed_campus(&pool, &format!("lucb_{}", s)).await;
        let school_a = seed_school(&pool, campus_a, &format!("luca_{}", s)).await;
        let school_b = seed_school(&pool, campus_b, &format!("lucb_{}", s)).await;

        let admin = seed_user(&pool, &format!("luadm_{}", s), "Administrator").await;
        assign_admin_to_campus(&pool, admin, campus_a).await;

        // One user in scope, one out of scope.
        let user_in  = seed_user(&pool, &format!("luin_{}", s),  "Student").await;
        let user_out = seed_user(&pool, &format!("luout_{}", s), "Student").await;
        assign_user_to_school(&pool, user_in,  school_a).await;
        assign_user_to_school(&pool, user_out, school_b).await;

        let app = init_service(
            App::new()
                .app_data(web::Data::new(pool.clone()))
                .configure(crate::routes::configure_routes),
        )
        .await;
        let token = login_as(&app, &format!("luadm_{}", s)).await;

        let resp = call_service(
            &app,
            TestRequest::get()
                .uri("/api/v1/admin/users")
                .insert_header(("Authorization", format!("Bearer {}", token)))
                .to_request(),
        )
        .await;
        assert_eq!(resp.status(), 200);
        let body: Value = read_body_json(resp).await;
        let ids: Vec<String> = body.as_array().unwrap()
            .iter()
            .filter_map(|u| u["id"].as_str().map(String::from))
            .collect();

        assert!(ids.contains(&user_in.to_string()),  "in-scope user must appear in list");
        assert!(!ids.contains(&user_out.to_string()), "out-of-scope user must NOT appear in list");
    }

    /// A scoped admin only sees pending deletion requests from users in their
    /// campus scope.  Requests from users in a different campus must be hidden.
    #[actix_web::test]
    #[ignore = "requires DATABASE_URL"]
    async fn test_list_deletion_requests_scoped_by_campus() {
        let pool = test_pool().await;
        let s = Uuid::new_v4().to_string()[..8].to_string();

        let campus_a = seed_campus(&pool, &format!("ldra_{}", s)).await;
        let campus_b = seed_campus(&pool, &format!("ldrb_{}", s)).await;
        let school_a = seed_school(&pool, campus_a, &format!("ldra_{}", s)).await;
        let school_b = seed_school(&pool, campus_b, &format!("ldrb_{}", s)).await;

        let admin = seed_user(&pool, &format!("ldradm_{}", s), "Administrator").await;
        assign_admin_to_campus(&pool, admin, campus_a).await;

        let user_in  = seed_user(&pool, &format!("ldrin_{}", s),  "Student").await;
        let user_out = seed_user(&pool, &format!("ldrout_{}", s), "Student").await;
        assign_user_to_school(&pool, user_in,  school_a).await;
        assign_user_to_school(&pool, user_out, school_b).await;

        let req_in  = seed_deletion_request(&pool, user_in).await;
        let req_out = seed_deletion_request(&pool, user_out).await;

        let app = init_service(
            App::new()
                .app_data(web::Data::new(pool.clone()))
                .configure(crate::routes::configure_routes),
        )
        .await;
        let token = login_as(&app, &format!("ldradm_{}", s)).await;

        let resp = call_service(
            &app,
            TestRequest::get()
                .uri("/api/v1/admin/deletion-requests")
                .insert_header(("Authorization", format!("Bearer {}", token)))
                .to_request(),
        )
        .await;
        assert_eq!(resp.status(), 200);
        let body: Value = read_body_json(resp).await;
        let ids: Vec<String> = body.as_array().unwrap()
            .iter()
            .filter_map(|r| r["id"].as_str().map(String::from))
            .collect();

        assert!(ids.contains(&req_in.to_string()),  "in-scope deletion request must appear");
        assert!(!ids.contains(&req_out.to_string()), "out-of-scope deletion request must NOT appear");
    }

    /// A scoped admin only sees orders placed by users within their campus scope.
    /// Orders from users in other campuses must be excluded.
    #[actix_web::test]
    #[ignore = "requires DATABASE_URL"]
    async fn test_list_orders_scoped_by_campus() {
        let pool = test_pool().await;
        let s = Uuid::new_v4().to_string()[..8].to_string();

        let campus_a = seed_campus(&pool, &format!("loa_{}", s)).await;
        let campus_b = seed_campus(&pool, &format!("lob_{}", s)).await;
        let school_a = seed_school(&pool, campus_a, &format!("loa_{}", s)).await;
        let school_b = seed_school(&pool, campus_b, &format!("lob_{}", s)).await;

        let admin = seed_user(&pool, &format!("loadm_{}", s), "Administrator").await;
        assign_admin_to_campus(&pool, admin, campus_a).await;

        let user_in  = seed_user(&pool, &format!("loin_{}", s),  "Student").await;
        let user_out = seed_user(&pool, &format!("loout_{}", s), "Student").await;
        assign_user_to_school(&pool, user_in,  school_a).await;
        assign_user_to_school(&pool, user_out, school_b).await;

        let order_in  = seed_product_and_order(&pool, user_in,  &format!("in_{}", s)).await;
        let order_out = seed_product_and_order(&pool, user_out, &format!("out_{}", s)).await;

        let app = init_service(
            App::new()
                .app_data(web::Data::new(pool.clone()))
                .configure(crate::routes::configure_routes),
        )
        .await;
        let token = login_as(&app, &format!("loadm_{}", s)).await;

        let resp = call_service(
            &app,
            TestRequest::get()
                .uri("/api/v1/admin/orders")
                .insert_header(("Authorization", format!("Bearer {}", token)))
                .to_request(),
        )
        .await;
        assert_eq!(resp.status(), 200);
        let body: Value = read_body_json(resp).await;
        let ids: Vec<String> = body.as_array().unwrap()
            .iter()
            .filter_map(|o| o["id"].as_str().map(String::from))
            .collect();

        assert!(ids.contains(&order_in.to_string()),  "in-scope order must appear");
        assert!(!ids.contains(&order_out.to_string()), "out-of-scope order must NOT appear");
    }

    // Keep the chrono import used above from triggering dead_code warnings.
    #[allow(dead_code)]
    fn _use_utc() -> chrono::DateTime<Utc> { Utc::now() }
}
