use serde::{Deserialize, Serialize};
use uuid::Uuid;

use super::client::{get, post, ApiError};

// ── Product types ─────────────────────────────────────────────────────────────

#[derive(Debug, Clone, Deserialize, PartialEq)]
pub struct Product {
    pub id: Uuid,
    pub name: String,
    pub description: Option<String>,
    pub price_cents: i32,
    pub sku: Option<String>,
    pub category: Option<String>,
    pub image_url: Option<String>,
    pub active: bool,
    pub quantity: Option<i32>,
    pub low_stock_threshold: Option<i32>,
}

impl Product {
    pub fn price_display(&self) -> String {
        format!("${:.2}", self.price_cents as f64 / 100.0)
    }

    pub fn in_stock(&self) -> bool {
        self.quantity.unwrap_or(0) > 0
    }

    pub fn low_stock(&self) -> bool {
        let qty = self.quantity.unwrap_or(0);
        let threshold = self.low_stock_threshold.unwrap_or(10);
        qty > 0 && qty < threshold
    }
}

// ── Order types ───────────────────────────────────────────────────────────────

#[derive(Debug, Clone, Deserialize, PartialEq)]
pub struct OrderSummary {
    pub id: Uuid,
    pub user_id: Uuid,
    pub status: String,
    pub subtotal_cents: i64,
    pub shipping_fee_cents: i32,
    pub total_cents: i32,
    pub points_earned: i32,
    pub notes: Option<String>,
    pub created_at: String,
    pub updated_at: String,
}

impl OrderSummary {
    pub fn total_display(&self) -> String {
        format!("${:.2}", self.total_cents as f64 / 100.0)
    }

    pub fn shipping_display(&self) -> String {
        format!("${:.2}", self.shipping_fee_cents as f64 / 100.0)
    }
}

#[derive(Debug, Clone, Deserialize, PartialEq)]
pub struct OrderItem {
    pub id: Uuid,
    pub product_id: Uuid,
    pub product_name: String,
    pub quantity: i32,
    pub unit_price_cents: i32,
    pub subtotal_cents: i32,
}

impl OrderItem {
    pub fn subtotal_display(&self) -> String {
        format!("${:.2}", self.subtotal_cents as f64 / 100.0)
    }
}

#[derive(Debug, Clone, Deserialize, PartialEq)]
pub struct OrderDetail {
    pub id: Uuid,
    pub user_id: Uuid,
    pub status: String,
    pub subtotal_cents: i64,
    pub shipping_fee_cents: i32,
    pub total_cents: i32,
    pub points_earned: i32,
    pub notes: Option<String>,
    pub created_at: String,
    pub updated_at: String,
    pub items: Vec<OrderItem>,
}

// ── Cart types (local state only — not persisted to API) ─────────────────────

#[derive(Debug, Clone, PartialEq)]
pub struct CartItem {
    pub product_id: Uuid,
    pub product_name: String,
    pub quantity: i32,
    pub unit_price_cents: i32,
}

impl CartItem {
    pub fn subtotal_cents(&self) -> i32 {
        self.quantity * self.unit_price_cents
    }

    pub fn subtotal_display(&self) -> String {
        format!("${:.2}", self.subtotal_cents() as f64 / 100.0)
    }
}

// ── Request bodies ────────────────────────────────────────────────────────────

#[derive(Serialize)]
pub struct OrderLineInput {
    pub product_id: Uuid,
    pub quantity: i32,
}

#[derive(Serialize)]
pub struct CreateOrderRequest {
    pub items: Vec<OrderLineInput>,
    pub notes: Option<String>,
}

// ── Admin product types ───────────────────────────────────────────────────────

#[derive(Serialize)]
pub struct CreateProductRequest {
    pub name: String,
    pub description: Option<String>,
    pub price_cents: i32,
    pub sku: Option<String>,
    pub category: Option<String>,
    pub image_url: Option<String>,
    pub initial_quantity: Option<i32>,
}

#[derive(Serialize)]
pub struct UpdateProductRequest {
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

// ── Admin order types ─────────────────────────────────────────────────────────

#[derive(Debug, Clone, Deserialize, PartialEq)]
pub struct AdminOrder {
    pub id: Uuid,
    pub user_id: Uuid,
    pub username: String,
    pub status: String,
    pub total_cents: i32,
    pub shipping_fee_cents: i32,
    pub points_earned: i32,
    pub notes: Option<String>,
    pub item_count: i64,
    pub created_at: String,
    pub updated_at: String,
}

impl AdminOrder {
    pub fn total_display(&self) -> String {
        format!("${:.2}", self.total_cents as f64 / 100.0)
    }
}

#[derive(Serialize)]
pub struct UpdateOrderStatusRequest {
    pub status: String,
}

// ── Admin config types ────────────────────────────────────────────────────────

#[derive(Debug, Clone, Deserialize, PartialEq)]
pub struct ConfigValue {
    pub id: Uuid,
    pub key: String,
    pub value: Option<String>,
    pub value_type: String,
    pub description: Option<String>,
    pub scope: String,
}

#[derive(Debug, Clone, Deserialize, PartialEq)]
pub struct ConfigHistoryEntry {
    pub id: Uuid,
    pub config_key: String,
    pub old_value: Option<String>,
    pub new_value: Option<String>,
    pub changed_by_username: Option<String>,
    pub changed_at: String,
    pub reason: Option<String>,
}

#[derive(Debug, Clone, Deserialize, PartialEq)]
pub struct CampaignToggle {
    pub id: Uuid,
    pub name: String,
    pub description: Option<String>,
    pub enabled: bool,
    pub starts_at: Option<String>,
    pub ends_at: Option<String>,
}

#[derive(Serialize)]
pub struct UpdateConfigRequest {
    pub value: String,
    pub reason: Option<String>,
}

#[derive(Serialize)]
pub struct UpdateCampaignRequest {
    pub enabled: bool,
}

// ── KPI types ─────────────────────────────────────────────────────────────────

#[derive(Debug, Clone, Deserialize, PartialEq)]
pub struct KpiData {
    pub daily_sales_cents: i64,
    pub average_order_value_cents: i64,
    pub repeat_purchase_rate_pct: f64,
    pub orders_last_30d: i64,
    pub buyers_last_30d: i64,
    pub repeat_buyers_last_30d: i64,
}

#[derive(Debug, Clone, Deserialize, PartialEq)]
pub struct CommerceConfigSummary {
    pub shipping_fee_cents: i64,
    pub shipping_fee_display: String,
    pub points_rate_per_dollar: i64,
    pub campaigns: Vec<CampaignStatus>,
}

#[derive(Debug, Clone, Deserialize, PartialEq)]
pub struct CampaignStatus {
    pub name: String,
    pub enabled: bool,
}

#[derive(Debug, Clone, Deserialize, PartialEq)]
pub struct OrdersDashboard {
    pub pending_orders: i64,
    pub confirmed_orders: i64,
    pub fulfilled_orders: i64,
    pub cancelled_orders: i64,
    pub pending_over_30_min: i64,
    pub low_stock_products: Vec<DashboardLowStockProduct>,
    pub recent_orders: Vec<AdminOrder>,
}

#[derive(Debug, Clone, Deserialize, PartialEq)]
pub struct DashboardLowStockProduct {
    pub product_id: Uuid,
    pub product_name: String,
    pub quantity: i32,
    pub low_stock_threshold: i32,
}

impl KpiData {
    pub fn daily_sales_display(&self) -> String {
        format!("${:.2}", self.daily_sales_cents as f64 / 100.0)
    }

    pub fn avg_order_display(&self) -> String {
        format!("${:.2}", self.average_order_value_cents as f64 / 100.0)
    }

    pub fn repeat_rate_display(&self) -> String {
        format!("{:.1}%", self.repeat_purchase_rate_pct)
    }
}

// ── API call functions ────────────────────────────────────────────────────────

pub async fn get_products(token: &str) -> Result<Vec<Product>, ApiError> {
    get("/products", Some(token)).await
}

pub async fn get_product(token: &str, id: &Uuid) -> Result<Product, ApiError> {
    get(&format!("/products/{}", id), Some(token)).await
}

pub async fn create_order(
    token: &str,
    req: &CreateOrderRequest,
) -> Result<OrderDetail, ApiError> {
    post("/orders", req, Some(token)).await
}

pub async fn get_my_orders(token: &str) -> Result<Vec<OrderSummary>, ApiError> {
    get("/orders", Some(token)).await
}

pub async fn get_my_order_detail(token: &str, id: &Uuid) -> Result<OrderDetail, ApiError> {
    get(&format!("/orders/{}", id), Some(token)).await
}

pub async fn get_commerce_config(token: &str) -> Result<CommerceConfigSummary, ApiError> {
    get("/config/commerce", Some(token)).await
}

// ── Admin API calls ───────────────────────────────────────────────────────────

pub async fn admin_get_products(token: &str) -> Result<Vec<Product>, ApiError> {
    get("/admin/products", Some(token)).await
}

pub async fn admin_create_product(
    token: &str,
    req: &CreateProductRequest,
) -> Result<Product, ApiError> {
    post("/admin/products", req, Some(token)).await
}

pub async fn admin_deactivate_product(
    token: &str,
    id: &Uuid,
) -> Result<serde_json::Value, ApiError> {
    post(
        &format!("/admin/products/{}/deactivate", id),
        &serde_json::json!({}),
        Some(token),
    )
    .await
}

pub async fn admin_update_product(
    token: &str,
    id: &Uuid,
    req: &UpdateProductRequest,
) -> Result<Product, ApiError> {
    post(&format!("/admin/products/{}/update", id), req, Some(token)).await
}

pub async fn admin_get_orders(token: &str) -> Result<Vec<AdminOrder>, ApiError> {
    get("/admin/orders", Some(token)).await
}

pub async fn admin_get_orders_dashboard(token: &str) -> Result<OrdersDashboard, ApiError> {
    get("/admin/orders/dashboard", Some(token)).await
}

pub async fn admin_get_order_detail(token: &str, id: &Uuid) -> Result<OrderDetail, ApiError> {
    get(&format!("/admin/orders/{}", id), Some(token)).await
}

pub async fn admin_update_order_status(
    token: &str,
    id: &Uuid,
    req: &UpdateOrderStatusRequest,
) -> Result<serde_json::Value, ApiError> {
    post(&format!("/admin/orders/{}/status", id), req, Some(token)).await
}

pub async fn admin_get_kpi(token: &str) -> Result<KpiData, ApiError> {
    get("/admin/kpi", Some(token)).await
}

pub async fn admin_get_config(token: &str) -> Result<Vec<ConfigValue>, ApiError> {
    get("/admin/config", Some(token)).await
}

pub async fn admin_get_config_history(
    token: &str,
) -> Result<Vec<ConfigHistoryEntry>, ApiError> {
    get("/admin/config/history", Some(token)).await
}

pub async fn admin_get_campaigns(token: &str) -> Result<Vec<CampaignToggle>, ApiError> {
    get("/admin/config/campaigns", Some(token)).await
}

pub async fn admin_update_config(
    token: &str,
    key: &str,
    req: &UpdateConfigRequest,
) -> Result<serde_json::Value, ApiError> {
    post(&format!("/admin/config/values/{}", key), req, Some(token)).await
}

pub async fn admin_update_campaign(
    token: &str,
    name: &str,
    req: &UpdateCampaignRequest,
) -> Result<serde_json::Value, ApiError> {
    post(&format!("/admin/config/campaigns/{}", name), req, Some(token)).await
}
