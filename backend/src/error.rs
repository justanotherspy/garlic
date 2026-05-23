//! Error types for the storage layer and HTTP surface.

use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use axum::Json;
use serde_json::json;

/// Errors raised by the storage layer.
#[derive(Debug, thiserror::Error)]
pub enum StoreError {
    #[error("redis error: {0}")]
    Redis(#[from] redis::RedisError),

    #[error("serialization error: {0}")]
    Serde(#[from] serde_json::Error),

    #[error("could not acquire the per-user lock in time")]
    LockTimeout,
}

/// Errors surfaced to HTTP clients.
#[derive(Debug, thiserror::Error)]
pub enum AppError {
    #[error("missing or malformed Authorization header")]
    MissingToken,

    #[error("invalid token")]
    InvalidToken,

    #[error("{0}")]
    BadRequest(String),

    #[error("the service is busy; retry shortly")]
    Busy,

    #[error("internal storage error")]
    Storage,
}

impl From<StoreError> for AppError {
    fn from(err: StoreError) -> Self {
        match err {
            StoreError::LockTimeout => AppError::Busy,
            StoreError::Redis(e) => {
                tracing::error!(error = %e, "redis storage error");
                AppError::Storage
            }
            StoreError::Serde(e) => {
                tracing::error!(error = %e, "state serialization error");
                AppError::Storage
            }
        }
    }
}

impl IntoResponse for AppError {
    fn into_response(self) -> Response {
        let status = match self {
            AppError::MissingToken | AppError::InvalidToken => StatusCode::UNAUTHORIZED,
            AppError::BadRequest(_) => StatusCode::BAD_REQUEST,
            AppError::Busy => StatusCode::SERVICE_UNAVAILABLE,
            AppError::Storage => StatusCode::INTERNAL_SERVER_ERROR,
        };
        let body = Json(json!({ "error": self.to_string() }));
        (status, body).into_response()
    }
}
