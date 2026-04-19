/// Store display and calculation logic tests.
///
/// Tests pure-Rust formatting and arithmetic methods on Product, OrderSummary,
/// OrderItem, CartItem, and KpiData — no browser or WASM runtime required.
/// Also covers PatchPreferences serialization and Notification field contracts.
///
/// Run with:
///   cd frontend && cargo test --test store_display_tests
use meridian_frontend::api::preferences::{PatchPreferences, Preferences};
use meridian_frontend::api::notifications::Notification;
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

// ---------------------------------------------------------------------------
// PatchPreferences — partial-update serialisation
// ---------------------------------------------------------------------------

#[test]
fn patch_preferences_default_serializes_to_empty_object() {
    let patch = PatchPreferences::default();
    let json = serde_json::to_string(&patch).expect("serialise");
    let val: serde_json::Value = serde_json::from_str(&json).expect("parse");
    assert_eq!(val, serde_json::json!({}), "all-None patch must serialise to {{}}");
}

#[test]
fn patch_preferences_only_set_fields_are_serialized() {
    let patch = PatchPreferences {
        inbox_frequency: Some("daily".into()),
        dnd_enabled: Some(true),
        ..PatchPreferences::default()
    };
    let val: serde_json::Value =
        serde_json::from_str(&serde_json::to_string(&patch).unwrap()).unwrap();
    assert_eq!(val["inbox_frequency"], "daily");
    assert_eq!(val["dnd_enabled"], true);
    // Fields not set must be absent from the JSON.
    assert!(val.get("notif_checkin").is_none(), "unset fields must be omitted");
    assert!(val.get("dnd_start").is_none());
}

#[test]
fn patch_preferences_all_fields_set_serializes_all() {
    let patch = PatchPreferences {
        notif_checkin: Some(false),
        notif_order: Some(true),
        notif_general: Some(false),
        dnd_enabled: Some(true),
        dnd_start: Some("22:00".into()),
        dnd_end: Some("07:00".into()),
        inbox_frequency: Some("weekly".into()),
    };
    let val: serde_json::Value =
        serde_json::from_str(&serde_json::to_string(&patch).unwrap()).unwrap();
    assert_eq!(val["notif_checkin"], false);
    assert_eq!(val["notif_order"], true);
    assert_eq!(val["notif_general"], false);
    assert_eq!(val["dnd_enabled"], true);
    assert_eq!(val["dnd_start"], "22:00");
    assert_eq!(val["dnd_end"], "07:00");
    assert_eq!(val["inbox_frequency"], "weekly");
}

#[test]
fn patch_preferences_false_bool_is_not_skipped() {
    // None is skipped; Some(false) must NOT be skipped.
    let patch = PatchPreferences {
        notif_checkin: Some(false),
        ..PatchPreferences::default()
    };
    let val: serde_json::Value =
        serde_json::from_str(&serde_json::to_string(&patch).unwrap()).unwrap();
    assert_eq!(val["notif_checkin"], false, "Some(false) must appear in JSON");
}

// ---------------------------------------------------------------------------
// Preferences — field types and round-trip
// ---------------------------------------------------------------------------

#[test]
fn preferences_deserializes_from_server_json() {
    let raw = r#"{
        "notif_checkin": true,
        "notif_order": false,
        "notif_general": true,
        "dnd_enabled": false,
        "dnd_start": "21:00",
        "dnd_end": "06:00",
        "inbox_frequency": "immediate"
    }"#;
    let prefs: Preferences = serde_json::from_str(raw).expect("deserialise");
    assert!(prefs.notif_checkin);
    assert!(!prefs.notif_order);
    assert_eq!(prefs.dnd_start, "21:00");
    assert_eq!(prefs.dnd_end, "06:00");
    assert_eq!(prefs.inbox_frequency, "immediate");
}

// ---------------------------------------------------------------------------
// Notification — field contracts
// ---------------------------------------------------------------------------

fn make_notification(notification_type: &str, read_at: Option<&str>) -> Notification {
    Notification {
        id: "00000000-0000-0000-0000-000000000001".into(),
        subject: "Test Notification".into(),
        body: "This is a test.".into(),
        notification_type: notification_type.into(),
        read_at: read_at.map(String::from),
        created_at: "2025-01-01T10:00:00Z".into(),
        sender_username: None,
    }
}

#[test]
fn notification_is_read_when_read_at_is_some() {
    let n = make_notification("general", Some("2025-01-01T11:00:00Z"));
    assert!(n.read_at.is_some(), "notification with read_at must be read");
}

#[test]
fn notification_is_unread_when_read_at_is_none() {
    let n = make_notification("order", None);
    assert!(n.read_at.is_none(), "notification without read_at must be unread");
}

#[test]
fn notification_type_field_matches_what_was_set() {
    assert_eq!(make_notification("checkin", None).notification_type, "checkin");
    assert_eq!(make_notification("alert", None).notification_type, "alert");
    assert_eq!(make_notification("system", None).notification_type, "system");
}

#[test]
fn notification_sender_username_is_optional() {
    let mut n = make_notification("general", None);
    assert!(n.sender_username.is_none());
    n.sender_username = Some("admin_user".into());
    assert_eq!(n.sender_username.as_deref(), Some("admin_user"));
}

#[test]
fn notification_deserializes_from_server_json() {
    let raw = r#"{
        "id": "aaaaaaaa-0000-0000-0000-000000000001",
        "subject": "Your check-in was approved",
        "body": "Check-in for Window 1 has been approved.",
        "notification_type": "checkin",
        "read_at": null,
        "created_at": "2025-06-01T09:00:00Z",
        "sender_username": "teacher_jane"
    }"#;
    let n: Notification = serde_json::from_str(raw).expect("deserialise");
    assert_eq!(n.notification_type, "checkin");
    assert!(n.read_at.is_none());
    assert_eq!(n.sender_username.as_deref(), Some("teacher_jane"));
}
