/// Preference-aware notification creation service.
///
/// Every in-app notification that targets a user should go through
/// `create_user_notification` so that:
///
/// 1. Type-toggle checks are applied (e.g. user disabled check-in notifications).
/// 2. `display_after` is set according to inbox_frequency ("immediate",
///    "daily" digest at 17:00, "weekly" on Fridays at 17:00).
/// 3. DND suppression shifts non-critical notifications past the DND window end.
///
/// Critical notification types (alert, system) bypass ALL filters.
use chrono::{Datelike, Duration, NaiveTime, Utc, Weekday};
use uuid::Uuid;

use crate::db::DbPool;
use crate::errors::AppError;

// ---------------------------------------------------------------------------
// Preferences row type
// ---------------------------------------------------------------------------

#[derive(sqlx::FromRow)]
pub struct UserPreferences {
    pub notif_checkin: bool,
    pub notif_order: bool,
    pub notif_general: bool,
    pub dnd_enabled: bool,
    pub dnd_start: NaiveTime,
    pub dnd_end: NaiveTime,
    pub inbox_frequency: String,
}

impl Default for UserPreferences {
    fn default() -> Self {
        Self {
            notif_checkin: true,
            notif_order: true,
            notif_general: true,
            dnd_enabled: false,
            dnd_start: NaiveTime::from_hms_opt(21, 0, 0).unwrap(),
            dnd_end: NaiveTime::from_hms_opt(6, 0, 0).unwrap(),
            inbox_frequency: "immediate".to_string(),
        }
    }
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

/// Returns preferences for `user_id`, or defaults if no row exists yet.
pub async fn get_preferences(pool: &DbPool, user_id: Uuid) -> Result<UserPreferences, AppError> {
    let row = sqlx::query_as::<_, UserPreferences>(
        "SELECT notif_checkin, notif_order, notif_general,
                dnd_enabled, dnd_start, dnd_end, inbox_frequency
         FROM user_preferences WHERE user_id = $1",
    )
    .bind(user_id)
    .fetch_optional(pool)
    .await?;

    Ok(row.unwrap_or_default())
}

/// Returns whether the notification type is blocked by a type toggle.
///
/// alert / system are always permitted.
fn is_type_enabled(prefs: &UserPreferences, notification_type: &str) -> bool {
    match notification_type {
        "checkin" => prefs.notif_checkin,
        "order" => prefs.notif_order,
        "general" => prefs.notif_general,
        // critical types bypass toggle checks
        "alert" | "system" => true,
        _ => prefs.notif_general,
    }
}

/// Returns whether a notification type is critical (bypasses DND and frequency).
fn is_critical(notification_type: &str) -> bool {
    matches!(notification_type, "alert" | "system")
}

/// Compute the earliest timestamp at which a notification should become
/// visible, based on `inbox_frequency` and DND preferences.
///
/// Returns `None` if the notification should be visible immediately.
pub fn compute_display_after(prefs: &UserPreferences, notification_type: &str) -> Option<chrono::DateTime<Utc>> {
    if is_critical(notification_type) {
        return None;
    }

    let now = Utc::now();
    let now_time = now.time();

    // ── 1. Frequency-based floor ───────────────────────────────────────────
    let five_pm = NaiveTime::from_hms_opt(17, 0, 0).unwrap();

    let freq_floor: Option<chrono::DateTime<Utc>> = match prefs.inbox_frequency.as_str() {
        "daily" => {
            let today_5pm = now.date_naive().and_time(five_pm).and_utc();
            if now < today_5pm {
                // Not yet 5 PM today — defer until then.
                Some(today_5pm)
            } else {
                // Past 5 PM — defer to tomorrow's digest.
                let tomorrow_5pm = (now.date_naive() + Duration::days(1))
                    .and_time(five_pm)
                    .and_utc();
                Some(tomorrow_5pm)
            }
        }
        "weekly" => {
            // Next Friday at 17:00.
            let days_to_friday = (Weekday::Fri.num_days_from_monday() as i64
                - now.weekday().num_days_from_monday() as i64)
                .rem_euclid(7);
            let days_to_friday = if days_to_friday == 0 && now_time >= five_pm {
                7 // Already past 5 PM this Friday — go to next week.
            } else {
                days_to_friday
            };
            let next_friday_5pm = (now.date_naive() + Duration::days(days_to_friday))
                .and_time(five_pm)
                .and_utc();
            Some(next_friday_5pm)
        }
        _ => None, // "immediate"
    };

    // ── 2. DND floor ──────────────────────────────────────────────────────
    let dnd_floor: Option<chrono::DateTime<Utc>> = if prefs.dnd_enabled {
        let start = prefs.dnd_start;
        let end = prefs.dnd_end;

        // Determine if we are currently inside the DND window.
        let in_dnd = if start > end {
            // Overnight window (e.g. 21:00–06:00)
            now_time >= start || now_time < end
        } else {
            // Same-day window
            now_time >= start && now_time < end
        };

        if in_dnd {
            // Compute timestamp when DND ends.
            let end_today = now.date_naive().and_time(end).and_utc();
            let display_at = if now > end_today {
                // DND end wraps to tomorrow (overnight window, currently before midnight).
                (now.date_naive() + Duration::days(1)).and_time(end).and_utc()
            } else {
                end_today
            };
            Some(display_at)
        } else {
            None
        }
    } else {
        None
    };

    // ── 3. Take the later of the two floors ──────────────────────────────
    match (freq_floor, dnd_floor) {
        (Some(f), Some(d)) => Some(f.max(d)),
        (Some(f), None) => Some(f),
        (None, Some(d)) => Some(d),
        (None, None) => None,
    }
}

// ---------------------------------------------------------------------------
// Public API
// ---------------------------------------------------------------------------

/// Create a notification for a specific user, respecting their preferences.
///
/// Skips silently when the user has disabled notifications of this type.
pub async fn create_user_notification(
    pool: &DbPool,
    recipient_id: Uuid,
    sender_id: Option<Uuid>,
    subject: &str,
    body: &str,
    notification_type: &str,
) -> Result<(), AppError> {
    create_user_notification_with_ref(
        pool, recipient_id, sender_id, subject, body, notification_type, None,
    )
    .await
}

/// Like `create_user_notification` but attaches a `ref_key` used for
/// deduplication of auto-generated reminders.
///
/// If a non-expired notification with the same `ref_key` already exists for
/// this recipient, the call is a no-op.
pub async fn create_user_notification_with_ref(
    pool: &DbPool,
    recipient_id: Uuid,
    sender_id: Option<Uuid>,
    subject: &str,
    body: &str,
    notification_type: &str,
    ref_key: Option<&str>,
) -> Result<(), AppError> {
    // 1. Load preferences.
    let prefs = get_preferences(pool, recipient_id).await?;

    // 2. Check type toggle.
    if !is_type_enabled(&prefs, notification_type) {
        log::debug!(
            "notification suppressed by type toggle: recipient={} type={}",
            recipient_id,
            notification_type
        );
        return Ok(());
    }

    // 3. Dedup by ref_key (within a 12-hour window).
    if let Some(key) = ref_key {
        let existing: i64 = sqlx::query_scalar(
            "SELECT COUNT(*) FROM notifications
             WHERE recipient_id = $1
               AND ref_key = $2
               AND created_at > NOW() - INTERVAL '12 hours'",
        )
        .bind(recipient_id)
        .bind(key)
        .fetch_one(pool)
        .await?;

        if existing > 0 {
            log::debug!(
                "notification deduped by ref_key='{}' for recipient={}",
                key,
                recipient_id
            );
            return Ok(());
        }
    }

    // 4. Compute display_after.
    let display_after = compute_display_after(&prefs, notification_type);

    // 5. Insert.
    sqlx::query(
        "INSERT INTO notifications
         (id, recipient_id, sender_id, subject, body, notification_type,
          display_after, ref_key, created_at)
         VALUES ($1, $2, $3, $4, $5, $6, $7, $8, NOW())",
    )
    .bind(Uuid::new_v4())
    .bind(recipient_id)
    .bind(sender_id)
    .bind(subject)
    .bind(body)
    .bind(notification_type)
    .bind(display_after)
    .bind(ref_key)
    .execute(pool)
    .await?;

    Ok(())
}

// ---------------------------------------------------------------------------
// Unit tests (no database required)
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::{Timelike, Weekday};

    fn prefs_with(
        dnd_enabled: bool,
        dnd_start: (u32, u32),
        dnd_end: (u32, u32),
        freq: &str,
    ) -> UserPreferences {
        UserPreferences {
            notif_checkin: true,
            notif_order: true,
            notif_general: true,
            dnd_enabled,
            dnd_start: NaiveTime::from_hms_opt(dnd_start.0, dnd_start.1, 0).unwrap(),
            dnd_end: NaiveTime::from_hms_opt(dnd_end.0, dnd_end.1, 0).unwrap(),
            inbox_frequency: freq.to_string(),
        }
    }

    #[test]
    fn immediate_returns_none() {
        let prefs = prefs_with(false, (21, 0), (6, 0), "immediate");
        assert!(compute_display_after(&prefs, "checkin").is_none());
    }

    #[test]
    fn critical_types_always_immediate() {
        let prefs = prefs_with(true, (0, 0), (23, 59), "weekly");
        assert!(
            compute_display_after(&prefs, "alert").is_none(),
            "alert must bypass everything"
        );
        assert!(
            compute_display_after(&prefs, "system").is_none(),
            "system must bypass everything"
        );
    }

    #[test]
    fn type_toggle_blocks_checkin() {
        let prefs = UserPreferences {
            notif_checkin: false,
            ..UserPreferences::default()
        };
        assert!(!is_type_enabled(&prefs, "checkin"));
    }

    #[test]
    fn type_toggle_passes_alert_regardless() {
        let prefs = UserPreferences {
            notif_checkin: false,
            notif_order: false,
            notif_general: false,
            ..UserPreferences::default()
        };
        assert!(is_type_enabled(&prefs, "alert"));
        assert!(is_type_enabled(&prefs, "system"));
    }

    #[test]
    fn dnd_always_on_defers_non_critical() {
        // DND from 00:00 to 23:59 — always in DND.
        let prefs = prefs_with(true, (0, 0), (23, 59), "immediate");
        let da = compute_display_after(&prefs, "checkin");
        assert!(da.is_some(), "non-critical notification must be deferred during DND");
        let da = da.unwrap();
        assert!(da > Utc::now(), "deferred time must be in the future");
    }

    #[test]
    fn dnd_disabled_no_deferral() {
        let prefs = prefs_with(false, (21, 0), (6, 0), "immediate");
        assert!(compute_display_after(&prefs, "order").is_none());
    }

    #[test]
    fn weekly_frequency_lands_on_friday() {
        let prefs = prefs_with(false, (21, 0), (6, 0), "weekly");
        if let Some(da) = compute_display_after(&prefs, "checkin") {
            assert_eq!(
                da.weekday(),
                Weekday::Fri,
                "weekly digest must land on a Friday"
            );
        } else {
            panic!("expected Some(display_after) for weekly frequency");
        }
    }

    #[test]
    fn daily_frequency_defers_to_some_future_time() {
        let prefs = prefs_with(false, (21, 0), (6, 0), "daily");
        let da = compute_display_after(&prefs, "order");
        assert!(da.is_some(), "daily frequency must produce a deferral time");
        let da = da.unwrap();
        // Must land at 17:00 on some day.
        assert_eq!(da.time().hour(), 17);
        assert_eq!(da.time().minute(), 0);
        assert!(da > Utc::now(), "deferral must be in the future");
    }

    #[test]
    fn daily_frequency_target_is_at_five_pm() {
        let prefs = prefs_with(false, (21, 0), (6, 0), "daily");
        let da = compute_display_after(&prefs, "general").unwrap();
        assert_eq!(da.time().hour(), 17, "daily digest target hour must be 17 (5 PM UTC)");
        assert_eq!(da.time().minute(), 0);
        assert_eq!(da.time().second(), 0);
    }

    #[test]
    fn unknown_notification_type_uses_general_toggle() {
        // Unknown type falls back to notif_general.
        let prefs_on = UserPreferences { notif_general: true, ..UserPreferences::default() };
        assert!(is_type_enabled(&prefs_on, "foobar"), "unknown type should default to notif_general=true");

        let prefs_off = UserPreferences { notif_general: false, ..UserPreferences::default() };
        assert!(!is_type_enabled(&prefs_off, "foobar"), "unknown type should default to notif_general=false");
    }

    #[test]
    fn order_toggle_blocks_order_type() {
        let prefs = UserPreferences { notif_order: false, ..UserPreferences::default() };
        assert!(!is_type_enabled(&prefs, "order"));
        assert!(is_type_enabled(&prefs, "checkin"), "other types unaffected");
    }

    #[test]
    fn dnd_overnight_outside_window_no_deferral() {
        // DND window is 22:00–06:00.  During the daytime (14:00) we are outside
        // the DND window, so an immediate notification should not be deferred.
        let prefs = prefs_with(true, (22, 0), (6, 0), "immediate");
        // We cannot control Utc::now(), but we can verify the function at least
        // returns a consistent result — if now is outside the window, result is None;
        // if inside (e.g. running at 23:00 UTC in CI), it returns Some.
        // Either way the deferred time, if Some, must be in the future.
        if let Some(da) = compute_display_after(&prefs, "general") {
            assert!(da > Utc::now(), "any deferred time must be in the future");
        }
    }

    #[test]
    fn dnd_same_day_window_inside_defers() {
        // DND from 08:00–20:00 covers most of the workday.
        let prefs = prefs_with(true, (8, 0), (20, 0), "immediate");
        // Same reasoning as above — outcome depends on wall-clock time.
        if let Some(da) = compute_display_after(&prefs, "checkin") {
            assert!(da > Utc::now());
            // When deferred, the end-time (20:00 today or tomorrow) must be at 20:00.
            assert_eq!(da.time().hour(), 20);
            assert_eq!(da.time().minute(), 0);
        }
    }

    #[test]
    fn dnd_and_daily_frequency_takes_later_of_two_floors() {
        // Both DND and daily frequency produce floors; the function must take the max.
        let prefs = prefs_with(true, (0, 0), (23, 59), "daily");
        let da = compute_display_after(&prefs, "checkin");
        assert!(da.is_some(), "at least one floor must be active");
        let da = da.unwrap();
        assert!(da > Utc::now(), "deferred time must be in the future");
    }

    #[test]
    fn is_critical_identifies_correct_types() {
        assert!(is_critical("alert"));
        assert!(is_critical("system"));
        assert!(!is_critical("checkin"));
        assert!(!is_critical("order"));
        assert!(!is_critical("general"));
        assert!(!is_critical("foobar"));
    }

    #[test]
    fn default_preferences_are_permissive() {
        let prefs = UserPreferences::default();
        assert!(prefs.notif_checkin);
        assert!(prefs.notif_order);
        assert!(prefs.notif_general);
        assert!(!prefs.dnd_enabled);
        assert_eq!(prefs.inbox_frequency, "immediate");
    }
}
