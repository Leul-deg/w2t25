use actix_web::{HttpResponse, ResponseError};
use serde_json::json;
use thiserror::Error;

#[derive(Debug, Error)]
pub enum AppError {
    #[error("Database error: {0}")]
    DatabaseError(#[from] sqlx::Error),

    #[error("Not found: {0}")]
    NotFound(String),

    #[error("Unauthorized: {0}")]
    Unauthorized(String),

    #[error("Forbidden: {0}")]
    Forbidden(String),

    #[error("Validation error: {0}")]
    ValidationError(String),

    #[error("Internal error: {0}")]
    InternalError(String),

    #[error("Conflict: {0}")]
    ConflictError(String),

    #[error("Too many requests: {0}")]
    TooManyRequests(String),
}

impl AppError {
    /// Returns a message safe to send to the client (no internal details)
    pub fn client_message(&self) -> &str {
        match self {
            AppError::Unauthorized(_) => "Invalid username or password.",
            AppError::TooManyRequests(_) => {
                "Too many failed login attempts. Please wait before trying again."
            }
            AppError::Forbidden(msg) => msg.as_str(),
            AppError::NotFound(_) => "Resource not found.",
            AppError::ValidationError(msg) => msg.as_str(),
            AppError::ConflictError(msg) => msg.as_str(),
            AppError::DatabaseError(_) | AppError::InternalError(_) => {
                "An internal error occurred."
            }
        }
    }
}

impl ResponseError for AppError {
    fn error_response(&self) -> HttpResponse {
        let body = json!({ "error": self.client_message() });

        match self {
            AppError::DatabaseError(_) => HttpResponse::InternalServerError().json(body),
            AppError::NotFound(_) => HttpResponse::NotFound().json(body),
            AppError::Unauthorized(_) => HttpResponse::Unauthorized().json(body),
            AppError::Forbidden(_) => HttpResponse::Forbidden().json(body),
            AppError::ValidationError(_) => HttpResponse::UnprocessableEntity().json(body),
            AppError::InternalError(_) => HttpResponse::InternalServerError().json(body),
            AppError::ConflictError(_) => HttpResponse::Conflict().json(body),
            AppError::TooManyRequests(_) => HttpResponse::TooManyRequests().json(body),
        }
    }
}

impl From<argon2::password_hash::Error> for AppError {
    fn from(e: argon2::password_hash::Error) -> Self {
        match e {
            argon2::password_hash::Error::Password => {
                AppError::Unauthorized("Invalid credentials".into())
            }
            other => AppError::InternalError(other.to_string()),
        }
    }
}
