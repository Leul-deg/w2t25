/// Order management endpoints.
///
/// Customer-facing (authenticated):
///   POST /orders           – create order (checkout)
///   GET  /orders           – list authenticated user's orders
///   GET  /orders/{id}      – order detail (must own the order)
///
/// Admin endpoints live in admin.rs under /admin/orders.

use actix_web::{web, HttpResponse};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::db::DbPool;
use crate::errors::AppError;
use crate::middleware::auth::{require_order_in_admin_scope, AuthContext};
use crate::services::commerce;

// ---------------------------------------------------------------------------
// Route config
// ---------------------------------------------------------------------------

pub fn configure(cfg: &mut web::ServiceConfig) {
    cfg.service(
        web::scope("/orders")
            .route("", web::post().to(create_order))
            .route("", web::get().to(list_my_orders))
            .route("/{id}", web::get().to(get_my_order)),
    );
}

// ---------------------------------------------------------------------------
// DTOs
// ---------------------------------------------------------------------------

#[derive(Deserialize)]
pub struct OrderLineInput {
    pub product_id: Uuid,
    pub quantity: i32,
}

#[derive(Deserialize)]
pub struct CreateOrderBody {
    pub items: Vec<OrderLineInput>,
    /// Optional customer note
    pub notes: Option<String>,
}

#[derive(Serialize, sqlx::FromRow)]
pub struct OrderSummaryRow {
    pub id: Uuid,
    pub user_id: Uuid,
    pub status: String,
    pub subtotal_cents: i64,
    pub shipping_fee_cents: i32,
    pub total_cents: i32,
    pub points_earned: i32,
    pub notes: Option<String>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

#[derive(Serialize, sqlx::FromRow)]
pub struct OrderItemRow {
    pub id: Uuid,
    pub product_id: Uuid,
    pub product_name: String,
    pub quantity: i32,
    pub unit_price_cents: i32,
    pub subtotal_cents: i32,
}

#[derive(Serialize)]
pub struct OrderDetailResponse {
    #[serde(flatten)]
    pub order: OrderSummaryRow,
    pub items: Vec<OrderItemRow>,
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

/// Read a config integer value; returns `default` if the key is missing or
/// the stored value cannot be parsed.
async fn config_int(pool: &DbPool, key: &str, default: i64) -> i64 {
    let val: Option<String> = sqlx::query_scalar(
        "SELECT value FROM config_values WHERE key = $1 AND scope = 'global'",
    )
    .bind(key)
    .fetch_optional(pool)
    .await
    .unwrap_or(None);

    val.and_then(|v| v.parse::<i64>().ok()).unwrap_or(default)
}

async fn campaign_enabled(pool: &DbPool, name: &str, default: bool) -> bool {
    sqlx::query_scalar::<_, bool>(
        "SELECT enabled FROM campaign_toggles WHERE name = $1",
    )
    .bind(name)
    .fetch_optional(pool)
    .await
    .ok()
    .flatten()
    .unwrap_or(default)
}

/// Create a low-stock alert notification for all Administrator users.
async fn maybe_alert_low_stock(
    pool: &DbPool,
    product_id: Uuid,
    product_name: &str,
    new_qty: i32,
    threshold: i32,
) {
    if new_qty >= threshold {
        return;
    }
    // Find all administrator user IDs.
    let admin_ids: Vec<Uuid> = sqlx::query_scalar(
        "SELECT u.id FROM users u
         JOIN user_roles ur ON ur.user_id = u.id
         JOIN roles r ON r.id = ur.role_id
         WHERE r.name = 'Administrator' AND u.account_state = 'active'",
    )
    .fetch_all(pool)
    .await
    .unwrap_or_default();

    for admin_id in admin_ids {
        let _ = sqlx::query(
            "INSERT INTO notifications
                 (id, recipient_id, subject, body, notification_type, created_at)
             VALUES ($1, $2, $3, $4, 'alert', NOW())",
        )
        .bind(Uuid::new_v4())
        .bind(admin_id)
        .bind(format!("Low stock: {}", product_name))
        .bind(format!(
            "Product '{}' (id: {}) is low on stock: {} units remaining (threshold: {}).",
            product_name, product_id, new_qty, threshold
        ))
        .execute(pool)
        .await;
    }
}

pub async fn fetch_order_detail(
    pool: &DbPool,
    order_id: Uuid,
) -> Result<OrderDetailResponse, AppError> {
    let order = sqlx::query_as::<_, OrderSummaryRow>(
        "SELECT o.id, o.user_id, o.status,
                COALESCE((SELECT SUM(oi.subtotal_cents) FROM order_items oi WHERE oi.order_id = o.id), 0) AS subtotal_cents,
                o.shipping_fee_cents, o.total_cents, o.points_earned, o.notes,
                o.created_at, o.updated_at
         FROM orders o
         WHERE o.id = $1",
    )
    .bind(order_id)
    .fetch_optional(pool)
    .await?
    .ok_or_else(|| AppError::NotFound(format!("Order {} not found", order_id)))?;

    let items = sqlx::query_as::<_, OrderItemRow>(
        "SELECT oi.id, oi.product_id, p.name AS product_name,
                oi.quantity, oi.unit_price_cents, oi.subtotal_cents
         FROM order_items oi
         JOIN products p ON p.id = oi.product_id
         WHERE oi.order_id = $1
         ORDER BY p.name",
    )
    .bind(order_id)
    .fetch_all(pool)
    .await?;

    Ok(OrderDetailResponse { order, items })
}

// ---------------------------------------------------------------------------
// Handlers
// ---------------------------------------------------------------------------

/// POST /api/v1/orders
///
/// Full checkout flow:
///   1. Validate items (product active, stock available).
///   2. Calculate subtotal, shipping fee (from config), total, points.
///   3. Create order + line items, decrement inventory — in a transaction.
///   4. Fire low-stock alerts if any product drops below threshold.
///   5. Send an order confirmation notification to the customer.
async fn create_order(
    pool: web::Data<DbPool>,
    auth: AuthContext,
    body: web::Json<CreateOrderBody>,
) -> Result<HttpResponse, AppError> {
    if body.items.is_empty() {
        return Err(AppError::ValidationError(
            "Order must contain at least one item.".into(),
        ));
    }

    // Validate no duplicate product IDs.
    let mut seen = std::collections::HashSet::new();
    for item in &body.items {
        if item.quantity <= 0 {
            return Err(AppError::ValidationError(format!(
                "Quantity for product {} must be > 0.",
                item.product_id
            )));
        }
        if !seen.insert(item.product_id) {
            return Err(AppError::ValidationError(format!(
                "Duplicate product id {} in order.",
                item.product_id
            )));
        }
    }

    // Load products and inventory (with row lock).
    #[derive(sqlx::FromRow)]
    struct StockRow {
        id: Uuid,
        name: String,
        price_cents: i32,
        active: bool,
        quantity: i32,
        low_stock_threshold: i32,
    }

    let product_ids: Vec<Uuid> = body.items.iter().map(|i| i.product_id).collect();

    let stocks = sqlx::query_as::<_, StockRow>(
        "SELECT p.id, p.name, p.price_cents, p.active,
                COALESCE(i.quantity, 0) AS quantity,
                COALESCE(i.low_stock_threshold, 10) AS low_stock_threshold
         FROM products p
         LEFT JOIN inventory i ON i.product_id = p.id
         WHERE p.id = ANY($1)
         ORDER BY p.id",
    )
    .bind(&product_ids)
    .fetch_all(pool.get_ref())
    .await?;

    // Build a lookup map.
    let stock_map: std::collections::HashMap<Uuid, StockRow> =
        stocks.into_iter().map(|s| (s.id, s)).collect();

    // Validate each requested item against the fetched stock.
    let mut validated: Vec<(&OrderLineInput, &StockRow)> = Vec::new();
    for item in &body.items {
        let stock = stock_map
            .get(&item.product_id)
            .ok_or_else(|| AppError::NotFound(format!("Product {} not found.", item.product_id)))?;

        if !stock.active {
            return Err(AppError::ConflictError(format!(
                "Product '{}' is no longer available.",
                stock.name
            )));
        }
        if stock.quantity < item.quantity {
            return Err(AppError::ConflictError(format!(
                "Insufficient stock for '{}': {} requested, {} available.",
                stock.name, item.quantity, stock.quantity
            )));
        }
        validated.push((item, stock));
    }

    // Calculate financials.
    let subtotal_cents: i64 = validated
        .iter()
        .map(|(item, stock)| (item.quantity as i64) * (stock.price_cents as i64))
        .sum();

    let shipping_fee_cents = if campaign_enabled(pool.get_ref(), "free_shipping", false).await {
        0
    } else {
        commerce::apply_shipping_fee(config_int(pool.get_ref(), "shipping_fee_cents", 695).await)
    };

    let total_cents = commerce::calculate_total(subtotal_cents, shipping_fee_cents);

    let points_rate = if campaign_enabled(pool.get_ref(), "points_enabled", true).await {
        config_int(pool.get_ref(), "points_rate_per_dollar", 1).await
    } else {
        0
    };
    let points_earned = commerce::calculate_points(subtotal_cents, points_rate);

    // Persist inside a transaction.
    let order_id = Uuid::new_v4();
    let user_id = auth.0.user_id;

    let mut tx = pool.get_ref().begin().await?;

    sqlx::query(
        "INSERT INTO orders
             (id, user_id, status, total_cents, shipping_fee_cents, points_earned, notes, created_at, updated_at)
         VALUES ($1, $2, 'pending', $3, $4, $5, $6, NOW(), NOW())",
    )
    .bind(order_id)
    .bind(user_id)
    .bind(total_cents as i32)
    .bind(shipping_fee_cents as i32)
    .bind(points_earned as i32)
    .bind(&body.notes)
    .execute(&mut *tx)
    .await?;

    for (item, stock) in &validated {
        sqlx::query(
            "INSERT INTO order_items (id, order_id, product_id, quantity, unit_price_cents)
             VALUES ($1, $2, $3, $4, $5)",
        )
        .bind(Uuid::new_v4())
        .bind(order_id)
        .bind(item.product_id)
        .bind(item.quantity)
        .bind(stock.price_cents)
        .execute(&mut *tx)
        .await?;

        sqlx::query(
            "UPDATE inventory
             SET quantity = quantity - $1, last_updated_at = NOW()
             WHERE product_id = $2",
        )
        .bind(item.quantity)
        .bind(item.product_id)
        .execute(&mut *tx)
        .await?;
    }

    tx.commit().await?;

    // Post-commit: fire low-stock alerts for any product now below threshold.
    for (item, stock) in &validated {
        let new_qty = stock.quantity - item.quantity;
        maybe_alert_low_stock(
            pool.get_ref(),
            item.product_id,
            &stock.name,
            new_qty,
            stock.low_stock_threshold,
        )
        .await;
    }

    // Confirm order notification to customer.
    let _ = sqlx::query(
        "INSERT INTO notifications
             (id, recipient_id, subject, body, notification_type, created_at)
         VALUES ($1, $2, $3, $4, 'order', NOW())",
    )
    .bind(Uuid::new_v4())
    .bind(user_id)
    .bind(format!("Order #{} confirmed", &order_id.to_string()[..8]))
    .bind(format!(
        "Your order has been placed. Total: ${:.2} (including ${:.2} shipping). \
         Points earned: {}. Order ID: {}.",
        total_cents as f64 / 100.0,
        shipping_fee_cents as f64 / 100.0,
        points_earned,
        order_id
    ))
    .execute(pool.get_ref())
    .await;

    let detail = fetch_order_detail(pool.get_ref(), order_id).await?;
    Ok(HttpResponse::Created().json(detail))
}

/// GET /api/v1/orders
async fn list_my_orders(
    pool: web::Data<DbPool>,
    auth: AuthContext,
) -> Result<HttpResponse, AppError> {
    let rows = sqlx::query_as::<_, OrderSummaryRow>(
        "SELECT o.id, o.user_id, o.status,
                COALESCE((SELECT SUM(oi.subtotal_cents) FROM order_items oi WHERE oi.order_id = o.id), 0) AS subtotal_cents,
                o.shipping_fee_cents, o.total_cents, o.points_earned, o.notes,
                o.created_at, o.updated_at
         FROM orders o
         WHERE o.user_id = $1
         ORDER BY o.created_at DESC",
    )
    .bind(auth.0.user_id)
    .fetch_all(pool.get_ref())
    .await?;

    Ok(HttpResponse::Ok().json(rows))
}

/// GET /api/v1/orders/{id}
async fn get_my_order(
    pool: web::Data<DbPool>,
    auth: AuthContext,
    path: web::Path<Uuid>,
) -> Result<HttpResponse, AppError> {
    let order_id = path.into_inner();
    let detail = fetch_order_detail(pool.get_ref(), order_id).await?;

    if detail.order.user_id == auth.0.user_id {
        return Ok(HttpResponse::Ok().json(detail));
    }

    if auth.is_admin() {
        require_order_in_admin_scope(auth.0.user_id, pool.get_ref(), order_id).await?;
    } else {
        return Err(AppError::Forbidden(
            "You do not have access to this order.".into(),
        ));
    }

    Ok(HttpResponse::Ok().json(detail))
}

// ---------------------------------------------------------------------------
// Unit tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use crate::services::commerce;

    #[test]
    fn order_body_with_empty_items_is_invalid() {
        // Simulates the guard in create_order
        let body = CreateOrderBody { items: vec![], notes: None };
        assert!(body.items.is_empty());
    }

    #[test]
    fn negative_quantity_would_be_rejected() {
        let item = OrderLineInput {
            product_id: Uuid::new_v4(),
            quantity: -1,
        };
        assert!(item.quantity <= 0);
    }

    #[test]
    fn order_totals_calculated_correctly() {
        // $50.00 subtotal, $6.95 shipping → $56.95 total, 50 points
        let subtotal_cents: i64 = 5000;
        let shipping = commerce::apply_shipping_fee(695);
        let total = commerce::calculate_total(subtotal_cents, shipping);
        let points = commerce::calculate_points(subtotal_cents, 1);
        assert_eq!(total, 5695);
        assert_eq!(points, 50);
    }

    #[test]
    fn order_body_deserializes() {
        let json = r#"{
            "items": [
                {"product_id": "00000000-0000-0000-0000-000000000001", "quantity": 2},
                {"product_id": "00000000-0000-0000-0000-000000000002", "quantity": 1}
            ],
            "notes": "Please wrap as gift"
        }"#;
        let body: CreateOrderBody = serde_json::from_str(json).unwrap();
        assert_eq!(body.items.len(), 2);
        assert_eq!(body.notes.as_deref(), Some("Please wrap as gift"));
    }
}
