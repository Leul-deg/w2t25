/// Product catalogue endpoints.
///
/// Public (authenticated):
///   GET  /products           – list active products with current inventory
///   GET  /products/{id}      – single product detail (404 if inactive or missing)
///
/// Admin only (within /admin scope in admin.rs):
///   GET  /admin/products             – all products (including inactive) + inventory
///   POST /admin/products             – create product + initial inventory entry
///   POST /admin/products/{id}/update – edit name/desc/price/sku/category/image/active
///   POST /admin/products/{id}/deactivate – soft-delete (active = false)

use actix_web::{web, HttpResponse};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::db::DbPool;
use crate::errors::AppError;
use crate::middleware::auth::{require_global_admin_scope, AuthContext};

// ---------------------------------------------------------------------------
// Route config
// ---------------------------------------------------------------------------

pub fn configure(cfg: &mut web::ServiceConfig) {
    cfg.service(
        web::scope("/products")
            .route("", web::get().to(list_products))
            .route("/{id}", web::get().to(get_product)),
    );
}

// ---------------------------------------------------------------------------
// Shared DTOs
// ---------------------------------------------------------------------------

#[derive(Serialize, sqlx::FromRow)]
pub struct ProductRow {
    pub id: Uuid,
    pub name: String,
    pub description: Option<String>,
    pub price_cents: i32,
    pub sku: Option<String>,
    pub category: Option<String>,
    pub image_url: Option<String>,
    pub active: bool,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
    // joined from inventory
    pub quantity: Option<i32>,
    pub low_stock_threshold: Option<i32>,
}

/// The query used for listing — joins inventory so callers get stock information.
pub const PRODUCT_DETAIL_QUERY: &str = "
    SELECT p.id, p.name, p.description, p.price_cents, p.sku, p.category,
           p.image_url, p.active, p.created_at, p.updated_at,
           i.quantity, i.low_stock_threshold
    FROM products p
    LEFT JOIN inventory i ON i.product_id = p.id";

// ---------------------------------------------------------------------------
// Handlers
// ---------------------------------------------------------------------------

/// GET /api/v1/products
///
/// Returns all **active** products with current inventory counts.
/// Accessible to any authenticated user.
async fn list_products(
    pool: web::Data<DbPool>,
    auth: AuthContext,
) -> Result<HttpResponse, AppError> {
    let _ = auth; // authentication required, role not restricted

    let rows = sqlx::query_as::<_, ProductRow>(&format!(
        "{} WHERE p.active = TRUE ORDER BY p.name",
        PRODUCT_DETAIL_QUERY
    ))
    .fetch_all(pool.get_ref())
    .await?;

    Ok(HttpResponse::Ok().json(rows))
}

/// GET /api/v1/products/{id}
///
/// Returns a single product.  Returns 404 if the product does not exist OR is
/// inactive (prevents direct enumeration of unlisted items).
async fn get_product(
    pool: web::Data<DbPool>,
    auth: AuthContext,
    path: web::Path<Uuid>,
) -> Result<HttpResponse, AppError> {
    let _ = auth;
    let product_id = path.into_inner();

    let row = sqlx::query_as::<_, ProductRow>(&format!(
        "{} WHERE p.id = $1 AND p.active = TRUE",
        PRODUCT_DETAIL_QUERY
    ))
    .bind(product_id)
    .fetch_optional(pool.get_ref())
    .await?
    .ok_or_else(|| AppError::NotFound(format!("Product {} not found", product_id)))?;

    Ok(HttpResponse::Ok().json(row))
}

// ---------------------------------------------------------------------------
// Request bodies used by admin handlers (defined here, called from admin.rs)
// ---------------------------------------------------------------------------

#[derive(Deserialize)]
pub struct CreateProductBody {
    pub name: String,
    pub description: Option<String>,
    /// Price in cents (e.g. 1999 = $19.99)
    pub price_cents: i32,
    pub sku: Option<String>,
    pub category: Option<String>,
    pub image_url: Option<String>,
    /// Initial inventory count (defaults to 0)
    pub initial_quantity: Option<i32>,
}

#[derive(Deserialize)]
pub struct UpdateProductBody {
    pub name: Option<String>,
    pub description: Option<String>,
    pub price_cents: Option<i32>,
    pub sku: Option<String>,
    pub category: Option<String>,
    pub image_url: Option<String>,
    pub active: Option<bool>,
    pub quantity: Option<i32>,
    pub low_stock_threshold: Option<i32>,
}

// ---------------------------------------------------------------------------
// Admin handler implementations (called from admin.rs)
// ---------------------------------------------------------------------------

/// GET /api/v1/admin/products
pub async fn admin_list_products(
    pool: web::Data<DbPool>,
    auth: AuthContext,
) -> Result<HttpResponse, AppError> {
    auth.require_role("Administrator")?;
    require_global_admin_scope(auth.0.user_id, pool.get_ref()).await?;

    let rows = sqlx::query_as::<_, ProductRow>(&format!(
        "{} ORDER BY p.created_at DESC",
        PRODUCT_DETAIL_QUERY
    ))
    .fetch_all(pool.get_ref())
    .await?;

    Ok(HttpResponse::Ok().json(rows))
}

/// POST /api/v1/admin/products
pub async fn admin_create_product(
    pool: web::Data<DbPool>,
    auth: AuthContext,
    body: web::Json<CreateProductBody>,
) -> Result<HttpResponse, AppError> {
    auth.require_role("Administrator")?;
    require_global_admin_scope(auth.0.user_id, pool.get_ref()).await?;

    if body.name.trim().is_empty() {
        return Err(AppError::ValidationError("Product name is required.".into()));
    }
    if body.price_cents < 0 {
        return Err(AppError::ValidationError(
            "price_cents must be >= 0.".into(),
        ));
    }

    // Check SKU uniqueness if provided.
    if let Some(sku) = &body.sku {
        let existing: i64 = sqlx::query_scalar(
            "SELECT COUNT(*) FROM products WHERE sku = $1",
        )
        .bind(sku)
        .fetch_one(pool.get_ref())
        .await?;

        if existing > 0 {
            return Err(AppError::ConflictError(format!(
                "A product with SKU '{}' already exists.",
                sku
            )));
        }
    }

    let product_id = Uuid::new_v4();

    let row = sqlx::query_as::<_, ProductRow>(&format!(
        "WITH ins AS (
             INSERT INTO products (id, name, description, price_cents, sku, category,
                                   image_url, active, created_at, updated_at)
             VALUES ($1, $2, $3, $4, $5, $6, $7, TRUE, NOW(), NOW())
             RETURNING *
         )
         SELECT ins.id, ins.name, ins.description, ins.price_cents, ins.sku,
                ins.category, ins.image_url, ins.active, ins.created_at, ins.updated_at,
                i.quantity, i.low_stock_threshold
         FROM ins
         LEFT JOIN inventory i ON i.product_id = ins.id"
    ))
    .bind(product_id)
    .bind(body.name.trim())
    .bind(&body.description)
    .bind(body.price_cents)
    .bind(&body.sku)
    .bind(&body.category)
    .bind(&body.image_url)
    .fetch_one(pool.get_ref())
    .await?;

    // Create inventory row.
    let qty = body.initial_quantity.unwrap_or(0).max(0);
    sqlx::query(
        "INSERT INTO inventory (id, product_id, quantity, low_stock_threshold, last_updated_at)
         VALUES ($1, $2, $3, 10, NOW())",
    )
    .bind(Uuid::new_v4())
    .bind(product_id)
    .bind(qty)
    .execute(pool.get_ref())
    .await?;

    // Audit log.
    log_audit(
        pool.get_ref(),
        auth.0.user_id,
        "create_product",
        "product",
        &product_id.to_string(),
        None,
        Some(serde_json::json!({ "name": body.name, "price_cents": body.price_cents })),
    )
    .await?;

    Ok(HttpResponse::Created().json(row))
}

/// POST /api/v1/admin/products/{id}/update
pub async fn admin_update_product(
    pool: web::Data<DbPool>,
    auth: AuthContext,
    path: web::Path<Uuid>,
    body: web::Json<UpdateProductBody>,
) -> Result<HttpResponse, AppError> {
    auth.require_role("Administrator")?;
    require_global_admin_scope(auth.0.user_id, pool.get_ref()).await?;
    let product_id = path.into_inner();

    // Verify it exists.
    let existing = sqlx::query_as::<_, ProductRow>(&format!(
        "{} WHERE p.id = $1",
        PRODUCT_DETAIL_QUERY
    ))
    .bind(product_id)
    .fetch_optional(pool.get_ref())
    .await?
    .ok_or_else(|| AppError::NotFound(format!("Product {} not found", product_id)))?;

    // Validate price if provided.
    if let Some(p) = body.price_cents {
        if p < 0 {
            return Err(AppError::ValidationError(
                "price_cents must be >= 0.".into(),
            ));
        }
    }
    if let Some(q) = body.quantity {
        if q < 0 {
            return Err(AppError::ValidationError(
                "quantity must be >= 0.".into(),
            ));
        }
    }
    if let Some(threshold) = body.low_stock_threshold {
        if threshold < 1 {
            return Err(AppError::ValidationError(
                "low_stock_threshold must be >= 1.".into(),
            ));
        }
    }

    // Apply updates (only fields provided).
    sqlx::query(
        "UPDATE products SET
             name        = COALESCE($2, name),
             description = COALESCE($3, description),
             price_cents = COALESCE($4, price_cents),
             sku         = COALESCE($5, sku),
             category    = COALESCE($6, category),
             image_url   = COALESCE($7, image_url),
             active      = COALESCE($8, active),
             updated_at  = NOW()
         WHERE id = $1",
    )
    .bind(product_id)
    .bind(&body.name)
    .bind(&body.description)
    .bind(body.price_cents)
    .bind(&body.sku)
    .bind(&body.category)
    .bind(&body.image_url)
    .bind(body.active)
    .execute(pool.get_ref())
    .await?;

    if body.quantity.is_some() || body.low_stock_threshold.is_some() {
        sqlx::query(
            "UPDATE inventory
             SET quantity = COALESCE($2, quantity),
                 low_stock_threshold = COALESCE($3, low_stock_threshold),
                 last_updated_at = NOW()
             WHERE product_id = $1",
        )
        .bind(product_id)
        .bind(body.quantity)
        .bind(body.low_stock_threshold)
        .execute(pool.get_ref())
        .await?;
    }

    let updated = sqlx::query_as::<_, ProductRow>(&format!(
        "{} WHERE p.id = $1",
        PRODUCT_DETAIL_QUERY
    ))
    .bind(product_id)
    .fetch_one(pool.get_ref())
    .await?;

    log_audit(
        pool.get_ref(),
        auth.0.user_id,
        "update_product",
        "product",
        &product_id.to_string(),
        Some(serde_json::json!({
            "name": existing.name,
            "price_cents": existing.price_cents,
            "active": existing.active,
            "quantity": existing.quantity,
            "low_stock_threshold": existing.low_stock_threshold
        })),
        Some(serde_json::json!({
            "name": updated.name,
            "price_cents": updated.price_cents,
            "active": updated.active,
            "quantity": updated.quantity,
            "low_stock_threshold": updated.low_stock_threshold
        })),
    )
    .await?;

    Ok(HttpResponse::Ok().json(updated))
}

/// POST /api/v1/admin/products/{id}/deactivate
///
/// Soft-deletes: sets active = false.  Active orders referencing this product
/// are unaffected.
pub async fn admin_deactivate_product(
    pool: web::Data<DbPool>,
    auth: AuthContext,
    path: web::Path<Uuid>,
) -> Result<HttpResponse, AppError> {
    auth.require_role("Administrator")?;
    require_global_admin_scope(auth.0.user_id, pool.get_ref()).await?;
    let product_id = path.into_inner();

    let rows_affected = sqlx::query(
        "UPDATE products SET active = FALSE, updated_at = NOW() WHERE id = $1 AND active = TRUE",
    )
    .bind(product_id)
    .execute(pool.get_ref())
    .await?
    .rows_affected();

    if rows_affected == 0 {
        return Err(AppError::NotFound(format!(
            "Product {} not found or already inactive.",
            product_id
        )));
    }

    log_audit(
        pool.get_ref(),
        auth.0.user_id,
        "deactivate_product",
        "product",
        &product_id.to_string(),
        Some(serde_json::json!({ "active": true })),
        Some(serde_json::json!({ "active": false })),
    )
    .await?;

    Ok(HttpResponse::Ok().json(serde_json::json!({
        "product_id": product_id,
        "message": "Product deactivated."
    })))
}

// ---------------------------------------------------------------------------
// Helpers
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
        "INSERT INTO audit_logs
             (id, actor_id, action, entity_type, entity_id, old_data, new_data, created_at)
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
// Unit tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn create_product_body_deserializes() {
        let json = r#"{
            "name": "Meridian Hoodie",
            "price_cents": 4999,
            "sku": "HOO-001",
            "category": "apparel",
            "initial_quantity": 25
        }"#;
        let body: CreateProductBody = serde_json::from_str(json).unwrap();
        assert_eq!(body.name, "Meridian Hoodie");
        assert_eq!(body.price_cents, 4999);
        assert_eq!(body.initial_quantity, Some(25));
    }

    #[test]
    fn update_product_body_all_optional() {
        // Empty body is valid (no-op update)
        let json = "{}";
        let body: UpdateProductBody = serde_json::from_str(json).unwrap();
        assert!(body.name.is_none());
        assert!(body.price_cents.is_none());
        assert!(body.active.is_none());
    }

    #[test]
    fn update_product_body_deserializes_inventory_fields() {
        let json = r#"{
            "price_cents": 2599,
            "quantity": 12,
            "low_stock_threshold": 10
        }"#;
        let body: UpdateProductBody = serde_json::from_str(json).unwrap();
        assert_eq!(body.price_cents, Some(2599));
        assert_eq!(body.quantity, Some(12));
        assert_eq!(body.low_stock_threshold, Some(10));
    }
}
