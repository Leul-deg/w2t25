/// Pure commerce calculation functions.
///
/// These functions contain no I/O; they are the canonical definitions used by
/// both the order-creation handler and the test suite.

// ---------------------------------------------------------------------------
// Shipping fee
// ---------------------------------------------------------------------------

/// Apply the configured shipping fee to an order.
///
/// `shipping_fee_cents` is the value stored in config_values for the key
/// `shipping_fee_cents` (default: 695 = $6.95).
///
/// Returns the fee to charge (currently a flat rate — no threshold logic).
pub fn apply_shipping_fee(shipping_fee_cents: i64) -> i64 {
    shipping_fee_cents.max(0)
}

// ---------------------------------------------------------------------------
// Points calculation
// ---------------------------------------------------------------------------

/// Calculate points earned on an order.
///
/// Points are awarded on the **subtotal** (before shipping), whole dollars
/// only.  Fractional cents do not earn partial points.
///
/// `points_rate_per_dollar` is stored in config_values under the key
/// `points_rate_per_dollar` (default: 1 point per $1.00).
pub fn calculate_points(subtotal_cents: i64, points_rate_per_dollar: i64) -> i64 {
    // floor(subtotal_cents / 100) * rate
    let whole_dollars = subtotal_cents / 100;
    whole_dollars * points_rate_per_dollar.max(0)
}

/// Calculate the order grand total.
///
/// total = subtotal + shipping_fee
pub fn calculate_total(subtotal_cents: i64, shipping_fee_cents: i64) -> i64 {
    subtotal_cents + shipping_fee_cents
}

// ---------------------------------------------------------------------------
// Unit tests — no database required
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    // ── shipping fee ──────────────────────────────────────────────────────
    #[test]
    fn shipping_fee_default_695() {
        assert_eq!(apply_shipping_fee(695), 695);
    }

    #[test]
    fn shipping_fee_zero_allowed() {
        assert_eq!(apply_shipping_fee(0), 0);
    }

    #[test]
    fn shipping_fee_negative_clamped_to_zero() {
        assert_eq!(apply_shipping_fee(-100), 0);
    }

    // ── points calculation ────────────────────────────────────────────────
    #[test]
    fn points_zero_for_zero_subtotal() {
        assert_eq!(calculate_points(0, 1), 0);
    }

    #[test]
    fn points_one_per_dollar_default_rate() {
        // $10.00 subtotal → 10 points
        assert_eq!(calculate_points(1000, 1), 10);
    }

    #[test]
    fn points_two_per_dollar_promotional_rate() {
        // $10.00 subtotal, rate=2 → 20 points
        assert_eq!(calculate_points(1000, 2), 20);
    }

    #[test]
    fn points_fractional_dollars_truncated() {
        // $10.99 subtotal → 10 whole dollars → 10 points at rate 1
        assert_eq!(calculate_points(1099, 1), 10);
    }

    #[test]
    fn points_rate_zero_earns_nothing() {
        assert_eq!(calculate_points(5000, 0), 0);
    }

    #[test]
    fn points_rate_negative_clamped() {
        // negative rate behaves as zero
        assert_eq!(calculate_points(5000, -1), 0);
    }

    // ── total calculation ─────────────────────────────────────────────────
    #[test]
    fn total_is_subtotal_plus_shipping() {
        // $15.00 subtotal + $6.95 shipping = $21.95
        assert_eq!(calculate_total(1500, 695), 2195);
    }

    #[test]
    fn total_free_shipping_scenario() {
        assert_eq!(calculate_total(2000, 0), 2000);
    }
}
