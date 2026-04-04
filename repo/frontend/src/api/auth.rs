use serde::{Deserialize, Serialize};
use crate::state::{LoginError, UserPublic};
use super::client::{get, post, post_no_body, ApiError};

#[derive(Serialize)]
pub struct LoginRequest {
    pub username: String,
    pub password: String,
}

#[derive(Deserialize)]
pub struct LoginResponse {
    pub token: String,
    pub user: UserPublic,
}

pub async fn login(username: String, password: String) -> Result<LoginResponse, LoginError> {
    let body = LoginRequest { username, password };
    match post::<LoginRequest, LoginResponse>("/auth/login", &body, None).await {
        Ok(resp) => Ok(resp),
        Err(ApiError::Http { status, message }) => {
            // Parse the server's JSON error body to extract the "error" field
            let server_msg = serde_json::from_str::<serde_json::Value>(&message)
                .ok()
                .and_then(|v| v["error"].as_str().map(|s| s.to_string()))
                .unwrap_or_else(|| message.clone());

            match status {
                401 => Err(LoginError::InvalidCredentials),
                429 => Err(LoginError::TooManyAttempts(server_msg)),
                403 => Err(LoginError::AccountBlocked(server_msg)),
                422 => Err(LoginError::ValidationError(server_msg)),
                _ => Err(LoginError::NetworkError(format!("HTTP {}: {}", status, server_msg))),
            }
        }
        Err(ApiError::Network(e)) => Err(LoginError::NetworkError(e)),
        Err(ApiError::Deserialize(e)) => Err(LoginError::NetworkError(e)),
    }
}

pub async fn me(token: &str) -> Result<UserPublic, ApiError> {
    get::<UserPublic>("/auth/me", Some(token)).await
}

pub async fn logout(token: &str) -> Result<(), ApiError> {
    post_no_body("/auth/logout", Some(token)).await
}

/// Verify the current user's password without creating a new session.
/// Used by QuickLock to re-authenticate after inactivity.
pub async fn verify_password(password: &str, token: &str) -> Result<(), ApiError> {
    #[derive(serde::Serialize)]
    struct Body<'a> {
        password: &'a str,
    }
    post::<_, serde_json::Value>("/auth/verify", &Body { password }, Some(token))
        .await
        .map(|_| ())
}
