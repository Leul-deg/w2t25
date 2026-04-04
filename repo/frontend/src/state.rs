use gloo_storage::Storage;
use serde::{Deserialize, Serialize};
use yew::UseStateHandle;

/// Classified login error for richer UX feedback.
#[derive(Debug, Clone, PartialEq)]
pub enum LoginError {
    /// Wrong credentials (401) — generic, don't reveal username existence
    InvalidCredentials,
    /// Rate limited / locked out (429)
    TooManyAttempts(String),
    /// Account blocked — includes specific message from server (403)
    AccountBlocked(String),
    /// Validation error (422)
    ValidationError(String),
    /// Network or unexpected error
    NetworkError(String),
}

impl LoginError {
    /// Returns the user-visible message for this error.
    pub fn display_message(&self) -> &str {
        match self {
            LoginError::InvalidCredentials => "Invalid username or password.",
            LoginError::TooManyAttempts(msg) => msg.as_str(),
            LoginError::AccountBlocked(msg) => msg.as_str(),
            LoginError::ValidationError(msg) => msg.as_str(),
            LoginError::NetworkError(_) => "A connection error occurred. Please try again.",
        }
    }

    /// CSS class suffix for styling the error banner
    pub fn css_class(&self) -> &str {
        match self {
            LoginError::InvalidCredentials => "error-invalid",
            LoginError::TooManyAttempts(_) => "error-lockout",
            LoginError::AccountBlocked(_) => "error-blocked",
            LoginError::ValidationError(_) => "error-validation",
            LoginError::NetworkError(_) => "error-network",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct UserPublic {
    pub id: String,
    pub username: String,
    pub email: String,
    pub display_name: Option<String>,
    pub account_state: String,
    pub roles: Vec<String>,
    pub created_at: String,
}

#[derive(Debug, Clone, PartialEq)]
pub struct AppState {
    pub user: Option<UserPublic>,
    pub token: Option<String>,
    pub loading: bool,
}

impl Default for AppState {
    fn default() -> Self {
        // Try to restore token from localStorage
        let token = gloo_storage::LocalStorage::get::<String>("meridian_token").ok();
        Self {
            user: None,
            token,
            loading: false,
        }
    }
}

impl AppState {
    pub fn is_authenticated(&self) -> bool {
        self.user.is_some() && self.token.is_some()
    }

    pub fn primary_role(&self) -> Option<&str> {
        self.user.as_ref()?.roles.first().map(|s| s.as_str())
    }

    pub fn has_role(&self, role: &str) -> bool {
        self.user
            .as_ref()
            .map(|u| u.roles.iter().any(|r| r == role))
            .unwrap_or(false)
    }

    pub fn login(&mut self, token: String, user: UserPublic) {
        let _ = gloo_storage::LocalStorage::set("meridian_token", &token);
        self.token = Some(token);
        self.user = Some(user);
    }

    pub fn logout(&mut self) {
        gloo_storage::LocalStorage::delete("meridian_token");
        self.token = None;
        self.user = None;
    }
}

// Context types
pub type AppStateContext = UseStateHandle<AppState>;

/// Shared lock state: `true` means the screen is locked and requires password
/// re-entry. Provided at App root so it persists across navigation.
pub type LockContext = UseStateHandle<bool>;
