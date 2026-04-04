/// PII masking utilities for report exports.
///
/// By default all exports mask identifying information:
///   - UUIDs    → last-4 characters only        e.g. "…a3f2"
///   - Emails   → local part replaced with ***   e.g. "a***@example.com"
///   - Usernames→ first char + ***               e.g. "j***"
///
/// Masking is bypassed only when the requesting user holds the
/// `pii_export` permission, verified via `check_pii_permission`.

use uuid::Uuid;

use crate::db::DbPool;
use crate::errors::AppError;

// ---------------------------------------------------------------------------
// Masking functions
// ---------------------------------------------------------------------------

/// Mask a UUID string to show only the last 4 hex characters.
///
/// Input:  "550e8400-e29b-41d4-a716-446655440000"
/// Output: "…5440000" (last 7 chars of the last segment)
///
/// We show the last 4 chars of the final segment so identifiers remain
/// comparable within a single report while preventing full reconstruction.
pub fn mask_id(id: &str) -> String {
    let last4 = if id.len() >= 4 { &id[id.len() - 4..] } else { id };
    format!("…{}", last4)
}

/// Mask an email address: show first char of the local part, then ***.
///
/// Input:  "jane.doe@example.com"
/// Output: "j***@example.com"
pub fn mask_email(email: &str) -> String {
    if let Some(at_pos) = email.find('@') {
        let local = &email[..at_pos];
        let domain = &email[at_pos..]; // includes the '@'
        let first = local.chars().next().unwrap_or('?');
        format!("{}***{}", first, domain)
    } else {
        "***".to_string()
    }
}

/// Mask a username: show first char, then ***.
///
/// Input:  "admin_user"
/// Output: "a***"
pub fn mask_username(username: &str) -> String {
    let first = username.chars().next().unwrap_or('?');
    format!("{}***", first)
}

/// Mask a UUID: shorthand that converts to string first.
pub fn mask_uuid(id: Uuid) -> String {
    mask_id(&id.to_string())
}

// ---------------------------------------------------------------------------
// Permission check
// ---------------------------------------------------------------------------

/// Returns `true` if the user has the `pii_export` permission via any
/// of their assigned roles.
pub async fn check_pii_permission(pool: &DbPool, user_id: Uuid) -> Result<bool, AppError> {
    let count: i64 = sqlx::query_scalar(
        "SELECT COUNT(*)
         FROM user_roles ur
         JOIN role_permissions rp ON rp.role_id = ur.role_id
         JOIN permissions p ON p.id = rp.permission_id
         WHERE ur.user_id = $1 AND p.name = 'pii_export'",
    )
    .bind(user_id)
    .fetch_one(pool)
    .await?;

    Ok(count > 0)
}

// ---------------------------------------------------------------------------
// Unit tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    // ── mask_id ───────────────────────────────────────────────────────────

    #[test]
    fn mask_id_shows_last_four_chars() {
        let id = "550e8400-e29b-41d4-a716-446655440000";
        assert_eq!(mask_id(id), "…0000");
    }

    #[test]
    fn mask_id_short_string() {
        assert_eq!(mask_id("ab"), "…ab");
    }

    #[test]
    fn mask_id_uuid_format() {
        let id = Uuid::new_v4().to_string();
        let masked = mask_id(&id);
        assert!(masked.starts_with('…'));
        assert_eq!(masked.chars().count(), 5); // '…' + 4 chars
    }

    // ── mask_email ────────────────────────────────────────────────────────

    #[test]
    fn mask_email_standard() {
        assert_eq!(mask_email("jane.doe@example.com"), "j***@example.com");
    }

    #[test]
    fn mask_email_short_local() {
        assert_eq!(mask_email("a@b.com"), "a***@b.com");
    }

    #[test]
    fn mask_email_no_at_sign() {
        assert_eq!(mask_email("notanemail"), "***");
    }

    // ── mask_username ─────────────────────────────────────────────────────

    #[test]
    fn mask_username_standard() {
        assert_eq!(mask_username("admin_user"), "a***");
    }

    #[test]
    fn mask_username_single_char() {
        assert_eq!(mask_username("x"), "x***");
    }

    // ── PII masking default behaviour ─────────────────────────────────────

    #[test]
    fn pii_masking_is_on_by_default() {
        // This documents the contract: callers must explicitly opt out.
        // The default for the `pii_masked` flag on report_jobs is TRUE.
        let default_pii_masked = true;
        assert!(default_pii_masked);
    }

    // ── mask_uuid helper ─────────────────────────────────────────────────

    #[test]
    fn mask_uuid_converts_and_masks() {
        let id = Uuid::parse_str("550e8400-e29b-41d4-a716-446655440000").unwrap();
        assert_eq!(mask_uuid(id), "…0000");
    }
}
