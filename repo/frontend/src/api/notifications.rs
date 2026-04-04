use serde::Deserialize;

use super::client::{get, post_no_body, ApiError};

// ---------------------------------------------------------------------------
// Response types
// ---------------------------------------------------------------------------

#[derive(Deserialize, Clone, PartialEq)]
pub struct Notification {
    pub id: String,
    pub subject: String,
    pub body: String,
    pub notification_type: String,
    pub read_at: Option<String>,
    pub created_at: String,
    pub sender_username: Option<String>,
}

#[derive(Deserialize, Clone, PartialEq)]
pub struct UnreadCount {
    pub unread: i64,
}

// ---------------------------------------------------------------------------
// API functions
// ---------------------------------------------------------------------------

/// Fetch the calling user's inbox (newest first, up to 50).
pub async fn list_notifications(token: &str) -> Result<Vec<Notification>, ApiError> {
    get::<Vec<Notification>>("/notifications", Some(token)).await
}

/// Returns the unread notification count for the calling user.
pub async fn unread_count(token: &str) -> Result<UnreadCount, ApiError> {
    get::<UnreadCount>("/notifications/unread-count", Some(token)).await
}

/// Mark a notification as read.
pub async fn mark_read(notification_id: &str, token: &str) -> Result<(), ApiError> {
    post_no_body(
        &format!("/notifications/{}/read", notification_id),
        Some(token),
    )
    .await
}

/// Triggers server-side reminder generation for the current Student or Parent.
/// Safe to call repeatedly; backend deduplicates reminder notifications.
pub async fn generate_reminders(token: &str) -> Result<(), ApiError> {
    post_no_body("/notifications/reminders/generate", Some(token)).await
}
