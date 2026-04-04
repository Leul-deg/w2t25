use serde::{Deserialize, Serialize};
use super::client::{get, patch, ApiError};

#[derive(Deserialize, Clone, PartialEq)]
pub struct Preferences {
    pub notif_checkin: bool,
    pub notif_order: bool,
    pub notif_general: bool,
    pub dnd_enabled: bool,
    /// "HH:MM" string, e.g. "21:00"
    pub dnd_start: String,
    /// "HH:MM" string, e.g. "06:00"
    pub dnd_end: String,
    /// "immediate" | "daily" | "weekly"
    pub inbox_frequency: String,
}

/// Partial update body — only set fields that should change.
#[derive(Serialize, Default)]
pub struct PatchPreferences {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub notif_checkin: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub notif_order: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub notif_general: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub dnd_enabled: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub dnd_start: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub dnd_end: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub inbox_frequency: Option<String>,
}

pub async fn get_preferences(token: &str) -> Result<Preferences, ApiError> {
    get::<Preferences>("/preferences", Some(token)).await
}

pub async fn update_preferences(
    patch_body: &PatchPreferences,
    token: &str,
) -> Result<Preferences, ApiError> {
    patch::<PatchPreferences, Preferences>("/preferences", patch_body, Some(token)).await
}
