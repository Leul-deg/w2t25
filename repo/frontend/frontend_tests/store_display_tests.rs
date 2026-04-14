/// Store display and calculation logic tests.
///
/// Tests pure-Rust formatting and arithmetic methods on Product, OrderSummary,
/// OrderItem, CartItem, and KpiData — no browser or WASM runtime required.
///
/// Run with:
///   cd frontend && cargo test --test store_display_tests
use meridian_frontend::api::store::{CartItem, KpiData, OrderItem, OrderSummary, Product};
use uuid::Uuid;

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

fn make_product(price_cents: i32, quantity: Option<i32>, threshold: Option<i32>) -> Product {
    Product {
        id: Uuid::new_v4(),
        name: "Test Product".into(),
        description: None,
        price_cents,
        sku: None,
        category: None,
        image_url: None,
        active: true,
        quantity,
        low_stock_threshold: threshold,
    }
}

fn make_order_summary(total_cents: i32, shipping_fee_cents: i32) -> OrderSummary {
    OrderSummary {
        id: Uuid::new_v4(),
        user_id: Uuid::new_v4(),
        status: "pending".into(),
        subtotal_cents: (total_cents - shipping_fee_cents) as i64,
        shipping_fee_cents,
        total_cents,
        points_earned: 0,
        notes: None,
        created_at: "2024-01-01T00:00:00Z".into(),
        updated_at: "2024-01-01T00:00:00Z".into(),
    }
}

fn make_order_item(subtotal_cents: i32) -> OrderItem {
    OrderItem {
        id: Uuid::new_v4(),
        product_id: Uuid::new_v4(),
        product_name: "Widget".into(),
        quantity: 1,
        unit_price_cents: subtotal_cents,
        subtotal_cents,
    }
}

fn make_cart_item(quantity: i32, unit_price_cents: i32) -> CartItem {
    CartItem {
        product_id: Uuid::new_v4(),
        product_name: "Widget".into(),
        quantity,
        unit_price_cents,
    }
}

fn make_kpi(daily_sales_cents: i64, avg_order_cents: i64, repeat_rate: f64) -> KpiData {
    KpiData {
        daily_sales_cents,
        average_order_value_cents: avg_order_cents,
        repeat_purchase_rate_pct: repeat_rate,
        orders_last_30d: 10,
        buyers_last_30d: 8,
        repeat_buyers_last_30d: 3,
    }
}

// ---------------------------------------------------------------------------
// Product::price_display
// ---------------------------------------------------------------------------

#[test]
fn price_display_zero_cents() {
    assert_eq!(make_product(0, None, None).price_display(), "$0.00");
}

#[test]
fn price_display_one_cent() {
    assert_eq!(make_product(1, None, None).price_display(), "$0.01");
}

#[test]
fn price_display_exact_dollar() {
    assert_eq!(make_product(100, None, None).price_display(), "$1.00");
}

#[test]
fn price_display_dollars_and_cents() {
    assert_eq!(make_product(999, None, None).price_display(), "$9.99");
}

#[test]
fn price_display_large_amount() {
    assert_eq!(make_product(100000, None, None).price_display(), "$1000.00");
}

// ---------------------------------------------------------------------------
// Product::in_stock / Product::low_stock
// ---------------------------------------------------------------------------

#[test]
fn in_stock_true_when_quantity_positive() {
    assert!(make_product(100, Some(5), None).in_stock());
}

#[test]
fn in_stock_false_when_quantity_zero() {
    assert!(!make_product(100, Some(0), None).in_stock());
}

#[test]
fn in_stock_false_when_quantity_none() {
    assert!(!make_product(100, None, None).in_stock());
}

#[test]
fn low_stock_true_when_qty_below_threshold() {
    // qty=3, threshold=10 → low stock
    assert!(make_product(100, Some(3), Some(10)).low_stock());
}

#[test]
fn low_stock_false_when_qty_at_threshold() {
    // qty == threshold — NOT low stock (condition is qty < threshold)
    assert!(!make_product(100, Some(10), Some(10)).low_stock());
}

#[test]
fn low_stock_false_when_qty_above_threshold() {
    assert!(!make_product(100, Some(15), Some(10)).low_stock());
}

#[test]
fn low_stock_false_when_qty_zero() {
    // zero quantity is out-of-stock, not low-stock
    assert!(!make_product(100, Some(0), Some(10)).low_stock());
}

#[test]
fn low_stock_uses_default_threshold_of_ten() {
    // No threshold supplied — default is 10
    let p = make_product(100, Some(5), None);
    assert!(p.low_stock(), "qty 5 < default threshold 10 should be low stock");

    let p2 = make_product(100, Some(10), None);
    assert!(!p2.low_stock(), "qty 10 == default threshold 10 should not be low stock");
}

// ---------------------------------------------------------------------------
// OrderSummary display methods
// ---------------------------------------------------------------------------

#[test]
fn order_total_display_formats_correctly() {
    assert_eq!(make_order_summary(1500, 695).total_display(), "$15.00");
}

#[test]
fn order_shipping_display_formats_correctly() {
    assert_eq!(make_order_summary(1500, 695).shipping_display(), "$6.95");
}

#[test]
fn order_total_display_zero() {
    assert_eq!(make_order_summary(0, 0).total_display(), "$0.00");
}

#[test]
fn order_shipping_display_zero() {
    assert_eq!(make_order_summary(695, 0).shipping_display(), "$0.00");
}

// ---------------------------------------------------------------------------
// OrderItem::subtotal_display
// ---------------------------------------------------------------------------

#[test]
fn order_item_subtotal_display_one_cent() {
    assert_eq!(make_order_item(1).subtotal_display(), "$0.01");
}

#[test]
fn order_item_subtotal_display_whole_dollars() {
    assert_eq!(make_order_item(2000).subtotal_display(), "$20.00");
}

#[test]
fn order_item_subtotal_display_mixed() {
    assert_eq!(make_order_item(349).subtotal_display(), "$3.49");
}

// ---------------------------------------------------------------------------
// CartItem::subtotal_cents / CartItem::subtotal_display
// ---------------------------------------------------------------------------

#[test]
fn cart_item_subtotal_cents_multiplies_qty_by_price() {
    let item = make_cart_item(3, 499);
    assert_eq!(item.subtotal_cents(), 1497);
}

#[test]
fn cart_item_subtotal_cents_zero_qty() {
    assert_eq!(make_cart_item(0, 499).subtotal_cents(), 0);
}

#[test]
fn cart_item_subtotal_display_formats_correctly() {
    let item = make_cart_item(2, 995);
    assert_eq!(item.subtotal_display(), "$19.90");
}

#[test]
fn cart_item_subtotal_display_single_unit() {
    let item = make_cart_item(1, 100);
    assert_eq!(item.subtotal_display(), "$1.00");
}

// ---------------------------------------------------------------------------
// KpiData display methods
// ---------------------------------------------------------------------------

#[test]
fn kpi_daily_sales_display_formats_cents() {
    assert_eq!(make_kpi(15099, 0, 0.0).daily_sales_display(), "$150.99");
}

#[test]
fn kpi_avg_order_display_formats_cents() {
    assert_eq!(make_kpi(0, 2350, 0.0).avg_order_display(), "$23.50");
}

#[test]
fn kpi_repeat_rate_display_one_decimal() {
    assert_eq!(make_kpi(0, 0, 37.5).repeat_rate_display(), "37.5%");
}

#[test]
fn kpi_repeat_rate_display_zero() {
    assert_eq!(make_kpi(0, 0, 0.0).repeat_rate_display(), "0.0%");
}

#[test]
fn kpi_repeat_rate_display_rounds_to_one_decimal() {
    // 33.333... should format as "33.3%"
    assert_eq!(make_kpi(0, 0, 33.333_333).repeat_rate_display(), "33.3%");
}

#[test]
fn kpi_daily_sales_display_zero() {
    assert_eq!(make_kpi(0, 0, 0.0).daily_sales_display(), "$0.00");
}
