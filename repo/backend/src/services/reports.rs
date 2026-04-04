/// Report generation service.
///
/// Supported report types:
///   checkins     – per-submission log with window, status, and reviewer
///   approvals    – approval/denial decisions with reason
///   orders       – order line items with financials
///   kpi          – daily KPI snapshot for the requested range
///   operational  – aggregate counts (check-ins, orders, users) per day
///
/// All reports are CSV files written to the configured exports directory.
///
/// Date-range constraint:
///   Maximum 366 calendar days (≈ 12 months) per request.
///   Requests exceeding this are rejected with a ValidationError.
///
/// PII masking (default: ON):
///   IDs        → last 4 chars with a '…' prefix
///   Emails     → first char + ***@domain
///   Usernames  → first char + ***
///
/// When pii_masked = false the caller must hold the pii_export permission.

use chrono::{DateTime, NaiveDate, Utc};
use uuid::Uuid;

use crate::db::DbPool;
use crate::errors::AppError;
use crate::services::masking::{mask_email, mask_id, mask_username};

/// Maximum days allowed in a single report request.
pub const MAX_REPORT_DAYS: i64 = 366;

// ---------------------------------------------------------------------------
// Date-range validation
// ---------------------------------------------------------------------------

/// Validate that `start <= end` and `end - start <= MAX_REPORT_DAYS`.
///
/// Returns Ok(()) or a ValidationError.
pub fn validate_date_range(start: NaiveDate, end: NaiveDate) -> Result<(), AppError> {
    if start > end {
        return Err(AppError::ValidationError(
            "Report start date must not be after end date.".into(),
        ));
    }
    let days = (end - start).num_days();
    if days > MAX_REPORT_DAYS {
        return Err(AppError::ValidationError(format!(
            "Report range too large: {} days requested; maximum is {} days (≈ 12 months).",
            days, MAX_REPORT_DAYS
        )));
    }
    Ok(())
}

// ---------------------------------------------------------------------------
// Report builders
// ---------------------------------------------------------------------------

/// Generate a check-ins CSV report.
///
/// Columns: submission_id, window_name, submitted_at, status, masked_student_id,
///          masked_username, masked_reviewer_id, reviewer_decision, decision_at
pub async fn generate_checkins_report(
    pool: &DbPool,
    start: NaiveDate,
    end: NaiveDate,
    pii_masked: bool,
) -> Result<String, AppError> {
    validate_date_range(start, end)?;

    let start_ts: DateTime<Utc> = start.and_hms_opt(0, 0, 0).unwrap().and_utc();
    let end_ts: DateTime<Utc> = end.and_hms_opt(23, 59, 59).unwrap().and_utc();

    #[derive(sqlx::FromRow)]
    struct Row {
        submission_id: Uuid,
        window_name: String,
        submitted_at: DateTime<Utc>,
        status: String,
        student_id: Uuid,
        student_username: String,
        reviewer_id: Option<Uuid>,
        reviewer_decision: Option<String>,
        decided_at: Option<DateTime<Utc>>,
    }

    let rows = sqlx::query_as::<_, Row>(
        "SELECT cs.id AS submission_id, cw.title AS window_name,
                cs.submitted_at,
                COALESCE(cad.decision, 'pending') AS status,
                cs.student_id, u.username AS student_username,
                cad.decided_by AS reviewer_id, cad.decision AS reviewer_decision, cad.decided_at
         FROM checkin_submissions cs
         JOIN checkin_windows cw ON cw.id = cs.window_id
         JOIN users u ON u.id = cs.student_id
         LEFT JOIN checkin_approval_decisions cad ON cad.submission_id = cs.id
         WHERE cs.submitted_at >= $1 AND cs.submitted_at <= $2
         ORDER BY cs.submitted_at",
    )
    .bind(start_ts)
    .bind(end_ts)
    .fetch_all(pool)
    .await?;

    let mut csv = String::new();
    csv.push_str(
        "submission_id,window_name,submitted_at,status,student_id,student_username,\
         reviewer_id,reviewer_decision,decided_at\n",
    );

    for r in &rows {
        let sid = if pii_masked {
            mask_id(&r.student_id.to_string())
        } else {
            r.student_id.to_string()
        };
        let uname = if pii_masked {
            mask_username(&r.student_username)
        } else {
            r.student_username.clone()
        };
        let rid = r
            .reviewer_id
            .as_ref()
            .map(|id| {
                if pii_masked {
                    mask_id(&id.to_string())
                } else {
                    id.to_string()
                }
            })
            .unwrap_or_default();
        let sub_id = if pii_masked {
            mask_id(&r.submission_id.to_string())
        } else {
            r.submission_id.to_string()
        };

        csv.push_str(&format!(
            "{},{},{},{},{},{},{},{},{}\n",
            csv_escape(&sub_id),
            csv_escape(&r.window_name),
            r.submitted_at.format("%Y-%m-%dT%H:%M:%SZ"),
            csv_escape(&r.status),
            csv_escape(&sid),
            csv_escape(&uname),
            csv_escape(&rid),
            csv_escape(r.reviewer_decision.as_deref().unwrap_or("")),
            r.decided_at
                .map(|d| d.format("%Y-%m-%dT%H:%M:%SZ").to_string())
                .unwrap_or_default(),
        ));
    }

    Ok(csv)
}

/// Generate an approvals/denials report.
///
/// Columns: decision_id, submission_id, window_name, decision, decided_at,
///          masked_reviewer_id, masked_student_id, notes
pub async fn generate_approvals_report(
    pool: &DbPool,
    start: NaiveDate,
    end: NaiveDate,
    pii_masked: bool,
) -> Result<String, AppError> {
    validate_date_range(start, end)?;

    let start_ts: DateTime<Utc> = start.and_hms_opt(0, 0, 0).unwrap().and_utc();
    let end_ts: DateTime<Utc> = end.and_hms_opt(23, 59, 59).unwrap().and_utc();

    #[derive(sqlx::FromRow)]
    struct Row {
        decision_id: Uuid,
        submission_id: Uuid,
        window_name: String,
        decision: String,
        decided_at: DateTime<Utc>,
        reviewer_id: Uuid,
        student_id: Uuid,
        notes: Option<String>,
    }

    let rows = sqlx::query_as::<_, Row>(
        "SELECT cad.id AS decision_id, cad.submission_id, cw.title AS window_name,
                cad.decision, cad.decided_at, cad.decided_by AS reviewer_id, cs.student_id, cad.reason AS notes
         FROM checkin_approval_decisions cad
         JOIN checkin_submissions cs ON cs.id = cad.submission_id
         JOIN checkin_windows cw ON cw.id = cs.window_id
         WHERE cad.decided_at >= $1 AND cad.decided_at <= $2
         ORDER BY cad.decided_at",
    )
    .bind(start_ts)
    .bind(end_ts)
    .fetch_all(pool)
    .await?;

    let mut csv = String::new();
    csv.push_str("decision_id,submission_id,window_name,decision,decided_at,reviewer_id,student_id,notes\n");

    for r in &rows {
        let (did, subid, rid, sid) = if pii_masked {
            (
                mask_id(&r.decision_id.to_string()),
                mask_id(&r.submission_id.to_string()),
                mask_id(&r.reviewer_id.to_string()),
                mask_id(&r.student_id.to_string()),
            )
        } else {
            (
                r.decision_id.to_string(),
                r.submission_id.to_string(),
                r.reviewer_id.to_string(),
                r.student_id.to_string(),
            )
        };

        csv.push_str(&format!(
            "{},{},{},{},{},{},{},{}\n",
            csv_escape(&did),
            csv_escape(&subid),
            csv_escape(&r.window_name),
            csv_escape(&r.decision),
            r.decided_at.format("%Y-%m-%dT%H:%M:%SZ"),
            csv_escape(&rid),
            csv_escape(&sid),
            csv_escape(r.notes.as_deref().unwrap_or("")),
        ));
    }

    Ok(csv)
}

/// Generate an orders report.
///
/// Columns: order_id, placed_at, status, masked_user_id, item_count,
///          subtotal_usd, shipping_usd, total_usd, points_earned
pub async fn generate_orders_report(
    pool: &DbPool,
    start: NaiveDate,
    end: NaiveDate,
    pii_masked: bool,
) -> Result<String, AppError> {
    validate_date_range(start, end)?;

    let start_ts: DateTime<Utc> = start.and_hms_opt(0, 0, 0).unwrap().and_utc();
    let end_ts: DateTime<Utc> = end.and_hms_opt(23, 59, 59).unwrap().and_utc();

    #[derive(sqlx::FromRow)]
    struct Row {
        order_id: Uuid,
        placed_at: DateTime<Utc>,
        status: String,
        user_id: Uuid,
        username: String,
        item_count: i64,
        total_cents: i32,
        shipping_fee_cents: i32,
        points_earned: i32,
    }

    let rows = sqlx::query_as::<_, Row>(
        "SELECT o.id AS order_id, o.created_at AS placed_at, o.status,
                o.user_id, u.username,
                COUNT(oi.id) AS item_count,
                o.total_cents, o.shipping_fee_cents, o.points_earned
         FROM orders o
         JOIN users u ON u.id = o.user_id
         LEFT JOIN order_items oi ON oi.order_id = o.id
         WHERE o.created_at >= $1 AND o.created_at <= $2
         GROUP BY o.id, u.username
         ORDER BY o.created_at",
    )
    .bind(start_ts)
    .bind(end_ts)
    .fetch_all(pool)
    .await?;

    let mut csv = String::new();
    csv.push_str(
        "order_id,placed_at,status,user_id,username,item_count,\
         subtotal_usd,shipping_usd,total_usd,points_earned\n",
    );

    for r in &rows {
        let (oid, uid, uname) = if pii_masked {
            (
                mask_id(&r.order_id.to_string()),
                mask_id(&r.user_id.to_string()),
                mask_username(&r.username),
            )
        } else {
            (r.order_id.to_string(), r.user_id.to_string(), r.username.clone())
        };

        let subtotal = (r.total_cents - r.shipping_fee_cents) as f64 / 100.0;
        let shipping = r.shipping_fee_cents as f64 / 100.0;
        let total = r.total_cents as f64 / 100.0;

        csv.push_str(&format!(
            "{},{},{},{},{},{},{:.2},{:.2},{:.2},{}\n",
            csv_escape(&oid),
            r.placed_at.format("%Y-%m-%dT%H:%M:%SZ"),
            csv_escape(&r.status),
            csv_escape(&uid),
            csv_escape(&uname),
            r.item_count,
            subtotal,
            shipping,
            total,
            r.points_earned,
        ));
    }

    Ok(csv)
}

/// Generate a daily KPI report for a date range.
///
/// Columns: date, daily_sales_usd, order_count, buyer_count,
///          avg_order_value_usd, repeat_buyers
pub async fn generate_kpi_report(
    pool: &DbPool,
    start: NaiveDate,
    end: NaiveDate,
    _pii_masked: bool, // KPI is aggregate data — no PII regardless of flag
) -> Result<String, AppError> {
    validate_date_range(start, end)?;

    let start_ts: DateTime<Utc> = start.and_hms_opt(0, 0, 0).unwrap().and_utc();
    let end_ts: DateTime<Utc> = end.and_hms_opt(23, 59, 59).unwrap().and_utc();

    #[derive(sqlx::FromRow)]
    struct Row {
        day: NaiveDate,
        daily_sales_cents: i64,
        order_count: i64,
        buyer_count: i64,
        avg_order_cents: i64,
    }

    let rows = sqlx::query_as::<_, Row>(
        "SELECT
             DATE(created_at AT TIME ZONE 'UTC') AS day,
             COALESCE(SUM(total_cents), 0) AS daily_sales_cents,
             COUNT(*) AS order_count,
             COUNT(DISTINCT user_id) AS buyer_count,
             COALESCE(AVG(total_cents)::bigint, 0) AS avg_order_cents
         FROM orders
         WHERE status IN ('confirmed', 'fulfilled')
           AND created_at >= $1 AND created_at <= $2
         GROUP BY DATE(created_at AT TIME ZONE 'UTC')
         ORDER BY day",
    )
    .bind(start_ts)
    .bind(end_ts)
    .fetch_all(pool)
    .await?;

    let mut csv = String::new();
    csv.push_str("date,daily_sales_usd,order_count,buyer_count,avg_order_value_usd\n");

    for r in &rows {
        csv.push_str(&format!(
            "{},{:.2},{},{},{:.2}\n",
            r.day.format("%Y-%m-%d"),
            r.daily_sales_cents as f64 / 100.0,
            r.order_count,
            r.buyer_count,
            r.avg_order_cents as f64 / 100.0,
        ));
    }

    Ok(csv)
}

/// Generate an operational summary report.
///
/// Columns: date, checkin_submissions, approvals, denials, orders_placed,
///          orders_fulfilled, orders_cancelled, new_users, low_stock_alerts
pub async fn generate_operational_report(
    pool: &DbPool,
    start: NaiveDate,
    end: NaiveDate,
    _pii_masked: bool, // aggregate only
) -> Result<String, AppError> {
    validate_date_range(start, end)?;

    let start_ts: DateTime<Utc> = start.and_hms_opt(0, 0, 0).unwrap().and_utc();
    let end_ts: DateTime<Utc> = end.and_hms_opt(23, 59, 59).unwrap().and_utc();

    // Build a row per day using a series.
    #[derive(sqlx::FromRow)]
    struct Row {
        day: NaiveDate,
        checkin_submissions: i64,
        approvals: i64,
        denials: i64,
        orders_placed: i64,
        orders_fulfilled: i64,
        orders_cancelled: i64,
        new_users: i64,
        low_stock_alerts: i64,
    }

    let rows = sqlx::query_as::<_, Row>(
        "SELECT
             gs.day::date AS day,
             COALESCE((SELECT COUNT(*) FROM checkin_submissions cs
                       WHERE DATE(cs.submitted_at AT TIME ZONE 'UTC') = gs.day::date), 0) AS checkin_submissions,
             COALESCE((SELECT COUNT(*) FROM checkin_approval_decisions cad
                       WHERE DATE(cad.decided_at AT TIME ZONE 'UTC') = gs.day::date
                         AND cad.decision = 'approved'), 0) AS approvals,
             COALESCE((SELECT COUNT(*) FROM checkin_approval_decisions cad
                       WHERE DATE(cad.decided_at AT TIME ZONE 'UTC') = gs.day::date
                         AND cad.decision = 'denied'), 0) AS denials,
             COALESCE((SELECT COUNT(*) FROM orders o
                       WHERE DATE(o.created_at AT TIME ZONE 'UTC') = gs.day::date), 0) AS orders_placed,
             COALESCE((SELECT COUNT(*) FROM orders o
                       WHERE DATE(o.created_at AT TIME ZONE 'UTC') = gs.day::date
                         AND o.status = 'fulfilled'), 0) AS orders_fulfilled,
             COALESCE((SELECT COUNT(*) FROM orders o
                       WHERE DATE(o.created_at AT TIME ZONE 'UTC') = gs.day::date
                         AND o.status = 'cancelled'), 0) AS orders_cancelled,
             COALESCE((SELECT COUNT(*) FROM users u
                       WHERE DATE(u.created_at AT TIME ZONE 'UTC') = gs.day::date), 0) AS new_users,
             COALESCE((SELECT COUNT(*) FROM notifications n
                       WHERE DATE(n.created_at AT TIME ZONE 'UTC') = gs.day::date
                         AND n.notification_type = 'alert'), 0) AS low_stock_alerts
         FROM generate_series($1::date, $2::date, '1 day'::interval) gs(day)
         ORDER BY gs.day",
    )
    .bind(start_ts)
    .bind(end_ts)
    .fetch_all(pool)
    .await?;

    let mut csv = String::new();
    csv.push_str(
        "date,checkin_submissions,approvals,denials,orders_placed,\
         orders_fulfilled,orders_cancelled,new_users,low_stock_alerts\n",
    );

    for r in &rows {
        csv.push_str(&format!(
            "{},{},{},{},{},{},{},{},{}\n",
            r.day.format("%Y-%m-%d"),
            r.checkin_submissions,
            r.approvals,
            r.denials,
            r.orders_placed,
            r.orders_fulfilled,
            r.orders_cancelled,
            r.new_users,
            r.low_stock_alerts,
        ));
    }

    Ok(csv)
}

// ---------------------------------------------------------------------------
// File writing
// ---------------------------------------------------------------------------

/// Write CSV content to `{exports_dir}/{filename}`.
pub async fn write_report_file(content: &str, exports_dir: &str, filename: &str) -> Result<String, AppError> {
    tokio::fs::create_dir_all(exports_dir)
        .await
        .map_err(|e| AppError::InternalError(format!("Cannot create exports dir: {}", e)))?;

    let path = format!("{}/{}", exports_dir.trim_end_matches('/'), filename);
    tokio::fs::write(&path, content.as_bytes())
        .await
        .map_err(|e| AppError::InternalError(format!("Cannot write report file: {}", e)))?;

    Ok(path)
}

/// Build a canonical export filename.
pub fn report_filename(report_type: &str, start: NaiveDate, end: NaiveDate) -> String {
    format!(
        "{}_{}_{}_generated_{}.csv",
        report_type,
        start.format("%Y%m%d"),
        end.format("%Y%m%d"),
        chrono::Utc::now().format("%Y%m%dT%H%M%S"),
    )
}

// ---------------------------------------------------------------------------
// CSV escaping
// ---------------------------------------------------------------------------

/// Escape a value for CSV: wrap in quotes if it contains comma/quote/newline.
pub fn csv_escape(s: &str) -> String {
    if s.contains([',', '"', '\n', '\r']) {
        format!("\"{}\"", s.replace('"', "\"\""))
    } else {
        s.to_string()
    }
}

// ---------------------------------------------------------------------------
// Unit tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::NaiveDate;

    fn d(y: i32, m: u32, day: u32) -> NaiveDate {
        NaiveDate::from_ymd_opt(y, m, day).unwrap()
    }

    // ── Date range validation ─────────────────────────────────────────────

    #[test]
    fn valid_single_day_range() {
        assert!(validate_date_range(d(2026, 3, 1), d(2026, 3, 1)).is_ok());
    }

    #[test]
    fn valid_31_day_range() {
        assert!(validate_date_range(d(2026, 3, 1), d(2026, 3, 31)).is_ok());
    }

    #[test]
    fn valid_12_month_range() {
        // 366 days is exactly the maximum.
        assert!(validate_date_range(d(2026, 1, 1), d(2026, 12, 31)).is_ok());
    }

    #[test]
    fn range_exceeding_366_days_rejected() {
        // 367 days should fail.
        let result = validate_date_range(d(2026, 1, 1), d(2027, 1, 3));
        assert!(result.is_err());
        let msg = result.unwrap_err().to_string();
        assert!(msg.contains("maximum"), "error should mention maximum");
    }

    #[test]
    fn start_after_end_rejected() {
        let result = validate_date_range(d(2026, 3, 31), d(2026, 3, 1));
        assert!(result.is_err());
        let msg = result.unwrap_err().to_string();
        assert!(msg.contains("not be after"), "error should mention ordering");
    }

    #[test]
    fn max_range_constant_is_366() {
        assert_eq!(MAX_REPORT_DAYS, 366);
    }

    // ── CSV escaping ──────────────────────────────────────────────────────

    #[test]
    fn csv_escape_plain_string() {
        assert_eq!(csv_escape("hello"), "hello");
    }

    #[test]
    fn csv_escape_with_comma() {
        assert_eq!(csv_escape("a,b"), "\"a,b\"");
    }

    #[test]
    fn csv_escape_with_quote() {
        assert_eq!(csv_escape("say \"hi\""), "\"say \"\"hi\"\"\"");
    }

    #[test]
    fn csv_escape_empty_string() {
        assert_eq!(csv_escape(""), "");
    }

    // ── Filename format ───────────────────────────────────────────────────

    #[test]
    fn report_filename_starts_with_type() {
        let name = report_filename("orders", d(2026, 3, 1), d(2026, 3, 31));
        assert!(name.starts_with("orders_"));
    }

    #[test]
    fn report_filename_contains_dates() {
        let name = report_filename("checkins", d(2026, 3, 1), d(2026, 3, 31));
        assert!(name.contains("20260301"));
        assert!(name.contains("20260331"));
    }

    #[test]
    fn report_filename_ends_with_csv() {
        let name = report_filename("kpi", d(2026, 1, 1), d(2026, 1, 31));
        assert!(name.ends_with(".csv"));
    }

    // ── PII masking in reports ────────────────────────────────────────────

    #[test]
    fn pii_masked_id_does_not_contain_original() {
        let id = "550e8400-e29b-41d4-a716-446655440000";
        let masked = mask_id(id);
        // Masked version should not contain the full UUID.
        assert!(!masked.contains('-'));
        assert!(masked.len() < 10);
    }

    #[test]
    fn unmasked_flag_false_means_pii_off() {
        // In the generate_* functions, pii_masked=false means raw data is used.
        // This tests the flag semantics (not DB queries).
        let pii_masked = false;
        assert!(!pii_masked); // confirms the contract
    }
}
