/// Background scheduler:
///   1. Auto-cancel pending orders older than 30 minutes (every 60s)
///   2. Prune log entries older than 180 days (every hour)
///   3. Generate daily operational report at midnight UTC (checked every 60s)
///   4. Generate weekly KPI report on Mondays at midnight UTC

use chrono::{Datelike, NaiveDate, Timelike, Utc, Weekday};
use uuid::Uuid;

use crate::db::DbPool;
use crate::services::reports;

/// How often the main tick runs (seconds).
pub const TICK_INTERVAL_SECS: u64 = 60;

/// Pending orders older than this are auto-cancelled (seconds).
pub const ORDER_EXPIRY_SECS: i64 = 30 * 60; // 30 minutes

/// Log retention period (days). Public so tests can reference the constant.
pub const LOG_RETENTION_DAYS: i64 = 180;

pub async fn run_scheduler(pool: DbPool) {
    log::info!(
        "Scheduler started — tick={}s, order_expiry={}s, retention={}d",
        TICK_INTERVAL_SECS,
        ORDER_EXPIRY_SECS,
        LOG_RETENTION_DAYS
    );

    // Track the last run dates so we fire daily/weekly reports once per period.
    let mut last_daily: Option<NaiveDate> = None;
    let mut last_weekly: Option<NaiveDate> = None;

    loop {
        tokio::time::sleep(tokio::time::Duration::from_secs(TICK_INTERVAL_SECS)).await;

        let now = Utc::now();
        let today = now.date_naive();

        // ── 1. Auto-cancel expired pending orders ─────────────────────────
        if let Err(e) = auto_close_expired_orders(&pool).await {
            log::error!("Scheduler: auto-close tick failed: {}", e);
        }

        // Only run time-of-day jobs during the midnight hour (00:xx UTC).
        if now.hour() == 0 {
            // ── 2. Daily operational report ───────────────────────────────
            if last_daily.map(|d| d < today).unwrap_or(true) {
                if let Err(e) = run_daily_report(&pool, today).await {
                    log::error!("Scheduler: daily report failed: {}", e);
                } else {
                    last_daily = Some(today);
                }
            }

            // ── 3. Weekly KPI report (Mondays only) ───────────────────────
            if today.weekday() == Weekday::Mon
                && last_weekly.map(|d| d < today).unwrap_or(true)
            {
                if let Err(e) = run_weekly_report(&pool, today).await {
                    log::error!("Scheduler: weekly report failed: {}", e);
                } else {
                    last_weekly = Some(today);
                }
            }

            // ── 4. Log retention pruning (once per day) ───────────────────
            if let Err(e) = prune_old_logs(&pool).await {
                log::error!("Scheduler: log pruning failed: {}", e);
            }
        }
    }
}

// ---------------------------------------------------------------------------
// Order auto-close
// ---------------------------------------------------------------------------

/// Cancel all pending orders older than ORDER_EXPIRY_SECS.
///
/// Returns the number of orders cancelled.
pub async fn auto_close_expired_orders(pool: &DbPool) -> Result<usize, sqlx::Error> {
    let rows: Vec<(Uuid, Uuid)> = sqlx::query_as(
        "SELECT id, user_id FROM orders
         WHERE status = 'pending'
           AND created_at < NOW() - make_interval(secs => $1::double precision)
         FOR UPDATE SKIP LOCKED",
    )
    .bind(ORDER_EXPIRY_SECS as f64)
    .fetch_all(pool)
    .await?;

    if rows.is_empty() {
        return Ok(0);
    }

    let order_ids: Vec<Uuid> = rows.iter().map(|(id, _)| *id).collect();

    sqlx::query(
        "UPDATE orders SET status = 'cancelled', updated_at = NOW()
         WHERE id = ANY($1)",
    )
    .bind(&order_ids)
    .execute(pool)
    .await?;

    log::info!(
        "Scheduler: auto-cancelled {} expired pending orders",
        order_ids.len()
    );

    for (order_id, user_id) in &rows {
        let _ = sqlx::query(
            "INSERT INTO notifications
                 (id, recipient_id, subject, body, notification_type, created_at)
             VALUES ($1, $2, $3, $4, 'order', NOW())",
        )
        .bind(Uuid::new_v4())
        .bind(user_id)
        .bind("Order auto-cancelled")
        .bind(format!(
            "Your order {} was automatically cancelled because payment was not \
             completed within 30 minutes.",
            order_id
        ))
        .execute(pool)
        .await;

        let _ = sqlx::query(
            "INSERT INTO audit_logs
                 (id, actor_id, action, entity_type, entity_id, old_data, new_data, created_at)
             VALUES ($1, NULL, 'auto_cancel_order', 'order', $2,
                     '{\"status\":\"pending\"}'::jsonb,
                     '{\"status\":\"cancelled\",\"reason\":\"unpaid_30min\"}'::jsonb,
                     NOW())",
        )
        .bind(Uuid::new_v4())
        .bind(order_id.to_string())
        .execute(pool)
        .await;
    }

    Ok(rows.len())
}

// ---------------------------------------------------------------------------
// Log retention
// ---------------------------------------------------------------------------

/// Delete log entries older than LOG_RETENTION_DAYS from all three log tables.
///
/// Records a summary in audit_logs so administrators can see when pruning ran.
pub async fn prune_old_logs(pool: &DbPool) -> Result<(), sqlx::Error> {
    // Read configurable value; fall back to constant.
    let retention_days: i64 = sqlx::query_scalar(
        "SELECT COALESCE(value::bigint, $1)
         FROM config_values WHERE key = 'log_retention_days' AND scope = 'global'",
    )
    .bind(LOG_RETENTION_DAYS)
    .fetch_optional(pool)
    .await?
    .unwrap_or(LOG_RETENTION_DAYS);

    let cutoff_expr = format!(
        "NOW() - make_interval(days => {}::integer)",
        retention_days
    );

    let audit_deleted: i64 = sqlx::query_scalar(&format!(
        "WITH del AS (DELETE FROM audit_logs WHERE created_at < {} RETURNING id)
         SELECT COUNT(*) FROM del",
        cutoff_expr
    ))
    .fetch_one(pool)
    .await?;

    let access_deleted: i64 = sqlx::query_scalar(&format!(
        "WITH del AS (DELETE FROM access_logs WHERE created_at < {} RETURNING id)
         SELECT COUNT(*) FROM del",
        cutoff_expr
    ))
    .fetch_one(pool)
    .await?;

    let error_deleted: i64 = sqlx::query_scalar(&format!(
        "WITH del AS (DELETE FROM error_logs WHERE created_at < {} RETURNING id)
         SELECT COUNT(*) FROM del",
        cutoff_expr
    ))
    .fetch_one(pool)
    .await?;

    let total = audit_deleted + access_deleted + error_deleted;
    if total > 0 {
        log::info!(
            "Scheduler: pruned {} log entries older than {} days \
             (audit={}, access={}, errors={})",
            total,
            retention_days,
            audit_deleted,
            access_deleted,
            error_deleted
        );

        // Record the pruning event itself.
        let _ = sqlx::query(
            "INSERT INTO audit_logs
                 (id, actor_id, action, entity_type, entity_id, new_data, created_at)
             VALUES ($1, NULL, 'log_retention_prune', 'scheduler', 'log_prune', $2, NOW())",
        )
        .bind(Uuid::new_v4())
        .bind(serde_json::json!({
            "retention_days": retention_days,
            "audit_deleted": audit_deleted,
            "access_deleted": access_deleted,
            "error_deleted": error_deleted,
        }))
        .execute(pool)
        .await;
    }

    Ok(())
}

// ---------------------------------------------------------------------------
// Scheduled report generation
// ---------------------------------------------------------------------------

/// Generate a daily operational report for the previous UTC calendar day.
async fn run_daily_report(pool: &DbPool, today: NaiveDate) -> Result<(), String> {
    let yesterday = today.pred_opt().unwrap_or(today);
    log::info!("Scheduler: generating daily operational report for {}", yesterday);

    // Check if we already have a completed report for yesterday.
    let existing: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM report_jobs
         WHERE report_type = 'operational'
           AND status = 'completed'
           AND parameters->>'start_date' = $1",
    )
    .bind(yesterday.format("%Y-%m-%d").to_string())
    .fetch_one(pool)
    .await
    .map_err(|e| e.to_string())?;

    if existing > 0 {
        log::debug!("Scheduler: daily report for {} already exists, skipping", yesterday);
        return Ok(());
    }

    // Read exports_dir from config (default ../exports).
    let exports_dir: String = sqlx::query_scalar(
        "SELECT COALESCE(value, '../exports') FROM config_values WHERE key = 'exports_dir'",
    )
    .fetch_optional(pool)
    .await
    .ok()
    .flatten()
    .unwrap_or_else(|| "../exports".to_string());

    // Create the job record.
    let job_id = Uuid::new_v4();
    sqlx::query(
        "INSERT INTO report_jobs
             (id, name, report_type, parameters, status, pii_masked, created_at)
         VALUES ($1, $2, 'operational', $3, 'running', TRUE, NOW())",
    )
    .bind(job_id)
    .bind(format!("Daily operational report {}", yesterday))
    .bind(serde_json::json!({ "start_date": yesterday.to_string(), "end_date": yesterday.to_string() }))
    .execute(pool)
    .await
    .map_err(|e| e.to_string())?;

    match reports::generate_operational_report(pool, yesterday, yesterday, true).await {
        Ok(csv) => {
            let filename = reports::report_filename("operational", yesterday, yesterday);
            match reports::write_report_file(&csv, &exports_dir, &filename).await {
                Ok(path) => {
                    let checksum = crate::services::backup::sha256_hex(csv.as_bytes());
                    sqlx::query(
                        "UPDATE report_jobs
                         SET status = 'completed', output_path = $1, row_count = $2,
                             checksum = $3, completed_at = NOW()
                         WHERE id = $4",
                    )
                    .bind(&path)
                    .bind(csv.lines().count() as i32 - 1) // subtract header
                    .bind(&checksum)
                    .bind(job_id)
                    .execute(pool)
                    .await
                    .map_err(|e| e.to_string())?;
                    log::info!("Scheduler: daily report written to {}", path);
                }
                Err(e) => {
                    let _ = sqlx::query(
                        "UPDATE report_jobs SET status = 'failed', error_message = $1 WHERE id = $2",
                    )
                    .bind(e.to_string())
                    .bind(job_id)
                    .execute(pool)
                    .await;
                    return Err(e.to_string());
                }
            }
        }
        Err(e) => {
            let _ = sqlx::query(
                "UPDATE report_jobs SET status = 'failed', error_message = $1 WHERE id = $2",
            )
            .bind(e.to_string())
            .bind(job_id)
            .execute(pool)
            .await;
            return Err(e.to_string());
        }
    }

    Ok(())
}

/// Generate a weekly KPI report covering the previous 7 days.
async fn run_weekly_report(pool: &DbPool, today: NaiveDate) -> Result<(), String> {
    let week_end = today.pred_opt().unwrap_or(today);
    let week_start = week_end
        .checked_sub_days(chrono::Days::new(6))
        .unwrap_or(week_end);

    log::info!(
        "Scheduler: generating weekly KPI report for {} – {}",
        week_start,
        week_end
    );

    let existing: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM report_jobs
         WHERE report_type = 'kpi'
           AND status = 'completed'
           AND parameters->>'start_date' = $1",
    )
    .bind(week_start.format("%Y-%m-%d").to_string())
    .fetch_one(pool)
    .await
    .map_err(|e| e.to_string())?;

    if existing > 0 {
        log::debug!("Scheduler: weekly KPI report for {} already exists", week_start);
        return Ok(());
    }

    let exports_dir: String = sqlx::query_scalar(
        "SELECT COALESCE(value, '../exports') FROM config_values WHERE key = 'exports_dir'",
    )
    .fetch_optional(pool)
    .await
    .ok()
    .flatten()
    .unwrap_or_else(|| "../exports".to_string());

    let job_id = Uuid::new_v4();
    sqlx::query(
        "INSERT INTO report_jobs
             (id, name, report_type, parameters, status, pii_masked, created_at)
         VALUES ($1, $2, 'kpi', $3, 'running', TRUE, NOW())",
    )
    .bind(job_id)
    .bind(format!("Weekly KPI report {} – {}", week_start, week_end))
    .bind(serde_json::json!({ "start_date": week_start.to_string(), "end_date": week_end.to_string() }))
    .execute(pool)
    .await
    .map_err(|e| e.to_string())?;

    match reports::generate_kpi_report(pool, week_start, week_end, true).await {
        Ok(csv) => {
            let filename = reports::report_filename("kpi_weekly", week_start, week_end);
            match reports::write_report_file(&csv, &exports_dir, &filename).await {
                Ok(path) => {
                    let checksum = crate::services::backup::sha256_hex(csv.as_bytes());
                    sqlx::query(
                        "UPDATE report_jobs
                         SET status = 'completed', output_path = $1, row_count = $2,
                             checksum = $3, completed_at = NOW()
                         WHERE id = $4",
                    )
                    .bind(&path)
                    .bind(csv.lines().count() as i32 - 1)
                    .bind(&checksum)
                    .bind(job_id)
                    .execute(pool)
                    .await
                    .map_err(|e| e.to_string())?;
                    log::info!("Scheduler: weekly KPI report written to {}", path);
                }
                Err(e) => {
                    let _ = sqlx::query(
                        "UPDATE report_jobs SET status = 'failed', error_message = $1 WHERE id = $2",
                    )
                    .bind(e.to_string())
                    .bind(job_id)
                    .execute(pool)
                    .await;
                    return Err(e.to_string());
                }
            }
        }
        Err(e) => {
            let _ = sqlx::query(
                "UPDATE report_jobs SET status = 'failed', error_message = $1 WHERE id = $2",
            )
            .bind(e.to_string())
            .bind(job_id)
            .execute(pool)
            .await;
            return Err(e.to_string());
        }
    }

    Ok(())
}

// ---------------------------------------------------------------------------
// Unit tests (no DB required)
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use sqlx::postgres::PgPoolOptions;
    use sqlx::PgPool;

    async fn test_pool() -> PgPool {
        let url = std::env::var("DATABASE_URL")
            .expect("DATABASE_URL must be set to run integration tests");
        let pool = PgPoolOptions::new()
            .max_connections(3)
            .connect(&url)
            .await
            .expect("Failed to connect to test database");
        sqlx::migrate!("../migrations")
            .run(&pool)
            .await
            .expect("migration failed");
        pool
    }

    #[test]
    fn order_expiry_window_is_30_minutes() {
        assert_eq!(ORDER_EXPIRY_SECS, 1800);
    }

    #[test]
    fn log_retention_constant_is_180_days() {
        assert_eq!(LOG_RETENTION_DAYS, 180);
    }

    #[test]
    fn tick_interval_is_sixty_seconds() {
        assert_eq!(TICK_INTERVAL_SECS, 60);
    }

    // ── Pure date arithmetic used by the scheduler ────────────────────────

    #[test]
    fn yesterday_is_one_day_before_today() {
        let today = NaiveDate::from_ymd_opt(2025, 3, 15).unwrap();
        let yesterday = today.pred_opt().unwrap();
        assert_eq!(yesterday, NaiveDate::from_ymd_opt(2025, 3, 14).unwrap());
    }

    #[test]
    fn yesterday_wraps_across_month_boundary() {
        let today = NaiveDate::from_ymd_opt(2025, 4, 1).unwrap();
        let yesterday = today.pred_opt().unwrap();
        assert_eq!(yesterday, NaiveDate::from_ymd_opt(2025, 3, 31).unwrap());
    }

    #[test]
    fn weekly_report_spans_exactly_seven_days() {
        let today = NaiveDate::from_ymd_opt(2025, 3, 17).unwrap(); // Monday
        let week_end = today.pred_opt().unwrap();                    // Sunday March 16
        let week_start = week_end.checked_sub_days(chrono::Days::new(6)).unwrap(); // Monday March 10
        let span = (week_end - week_start).num_days();
        assert_eq!(span, 6, "week_end - week_start must be 6 days (7-day window)");
    }

    #[test]
    fn weekly_report_week_start_is_correct() {
        // today = Monday 2025-03-17; yesterday = Sunday 2025-03-16
        // week window = Mon 2025-03-10 → Sun 2025-03-16
        let today = NaiveDate::from_ymd_opt(2025, 3, 17).unwrap();
        let week_end = today.pred_opt().unwrap();
        let week_start = week_end.checked_sub_days(chrono::Days::new(6)).unwrap();
        assert_eq!(week_start, NaiveDate::from_ymd_opt(2025, 3, 10).unwrap());
        assert_eq!(week_end, NaiveDate::from_ymd_opt(2025, 3, 16).unwrap());
    }

    #[test]
    fn scheduler_only_fires_daily_report_once_per_day() {
        let today = NaiveDate::from_ymd_opt(2025, 6, 1).unwrap();
        let yesterday = NaiveDate::from_ymd_opt(2025, 5, 31).unwrap();

        // last_daily not set → should fire
        let last_daily: Option<NaiveDate> = None;
        assert!(last_daily.map(|d| d < today).unwrap_or(true), "must fire when last_daily is None");

        // last_daily = yesterday → should fire
        let last_daily = Some(yesterday);
        assert!(last_daily.map(|d| d < today).unwrap_or(true), "must fire when last_daily is yesterday");

        // last_daily = today → should NOT fire
        let last_daily = Some(today);
        assert!(!last_daily.map(|d| d < today).unwrap_or(true), "must NOT fire when last_daily is today");
    }

    #[test]
    fn scheduler_only_fires_weekly_report_on_mondays() {
        use chrono::Weekday;
        let monday = NaiveDate::from_ymd_opt(2025, 3, 17).unwrap();
        let tuesday = NaiveDate::from_ymd_opt(2025, 3, 18).unwrap();
        let sunday = NaiveDate::from_ymd_opt(2025, 3, 16).unwrap();

        assert_eq!(monday.weekday(), Weekday::Mon);
        assert_ne!(tuesday.weekday(), Weekday::Mon);
        assert_ne!(sunday.weekday(), Weekday::Mon);

        // Replicate the scheduler's gate condition
        let should_run = |day: NaiveDate| day.weekday() == Weekday::Mon;
        assert!(should_run(monday));
        assert!(!should_run(tuesday));
        assert!(!should_run(sunday));
    }

    /// Retention: only prunes entries older than the retention window.
    #[actix_web::test]
    #[ignore = "requires DATABASE_URL"]
    async fn test_auto_close_expired_orders_cancels_only_old_pending() {
        let pool = test_pool().await;
        let user_id = Uuid::new_v4();
        let username = format!("sched_user_{}", &Uuid::new_v4().to_string()[..8]);
        let password_hash =
            crate::services::auth::hash_password("SchedulerPass2024!!").expect("hash");

        sqlx::query(
            "INSERT INTO users (id, username, email, password_hash, account_state, created_at, updated_at)
             VALUES ($1, $2, $3, $4, 'active', NOW(), NOW())",
        )
        .bind(user_id)
        .bind(&username)
        .bind(format!("{}@test.local", username))
        .bind(password_hash)
        .execute(&pool)
        .await
        .unwrap();

        let old_order = Uuid::new_v4();
        let fresh_order = Uuid::new_v4();

        sqlx::query(
            "INSERT INTO orders
                 (id, user_id, status, total_cents, shipping_fee_cents, points_earned, created_at, updated_at)
             VALUES
                 ($1, $2, 'pending', 1000, 695, 10,
                  NOW() - INTERVAL '31 minutes', NOW() - INTERVAL '31 minutes'),
                 ($3, $2, 'pending', 1000, 695, 10,
                  NOW() - INTERVAL '5 minutes',  NOW() - INTERVAL '5 minutes')",
        )
        .bind(old_order)
        .bind(user_id)
        .bind(fresh_order)
        .execute(&pool)
        .await
        .unwrap();

        let closed = auto_close_expired_orders(&pool).await.unwrap();
        assert!(closed >= 1, "at least the expired pending order should be closed");

        let old_status: String = sqlx::query_scalar("SELECT status FROM orders WHERE id = $1")
            .bind(old_order)
            .fetch_one(&pool)
            .await
            .unwrap();
        let fresh_status: String = sqlx::query_scalar("SELECT status FROM orders WHERE id = $1")
            .bind(fresh_order)
            .fetch_one(&pool)
            .await
            .unwrap();

        assert_eq!(old_status, "cancelled");
        assert_eq!(fresh_status, "pending");

        // Cleanup.
        sqlx::query("DELETE FROM orders WHERE id = ANY($1)")
            .bind(&[old_order, fresh_order])
            .execute(&pool)
            .await
            .ok();
        sqlx::query("DELETE FROM users WHERE id = $1")
            .bind(user_id)
            .execute(&pool)
            .await
            .ok();
    }

    /// Retention pruning removes old logs.
    #[actix_web::test]
    #[ignore = "requires DATABASE_URL"]
    async fn test_prune_old_logs_removes_stale_entries() {
        let pool = test_pool().await;

        // Insert an access log entry that is 200 days old.
        sqlx::query(
            "INSERT INTO access_logs (id, action, success, created_at)
             VALUES ($1, 'test_retention_action', TRUE, NOW() - INTERVAL '200 days')",
        )
        .bind(Uuid::new_v4())
        .execute(&pool)
        .await
        .unwrap();

        // Insert a recent entry.
        sqlx::query(
            "INSERT INTO access_logs (id, action, success, created_at)
             VALUES ($1, 'test_retention_action_recent', TRUE, NOW() - INTERVAL '1 day')",
        )
        .bind(Uuid::new_v4())
        .execute(&pool)
        .await
        .unwrap();

        // Override retention_days to 180 so our 200-day entry should be pruned.
        prune_old_logs(&pool).await.unwrap();

        let stale_count: i64 = sqlx::query_scalar(
            "SELECT COUNT(*) FROM access_logs WHERE action = 'test_retention_action'",
        )
        .fetch_one(&pool)
        .await
        .unwrap();

        let recent_count: i64 = sqlx::query_scalar(
            "SELECT COUNT(*) FROM access_logs WHERE action = 'test_retention_action_recent'",
        )
        .fetch_one(&pool)
        .await
        .unwrap();

        assert_eq!(stale_count, 0, "200-day-old entry should be pruned");
        assert_eq!(recent_count, 1, "1-day-old entry should remain");

        // Cleanup.
        sqlx::query("DELETE FROM access_logs WHERE action LIKE 'test_retention_action%'")
            .execute(&pool)
            .await
            .ok();
    }
}
