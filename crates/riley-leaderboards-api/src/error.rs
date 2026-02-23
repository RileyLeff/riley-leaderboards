use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use axum::Json;
use serde_json::json;

use riley_leaderboards_core::error::Error as CoreError;

pub struct ApiError(CoreError);

impl From<CoreError> for ApiError {
    fn from(err: CoreError) -> Self {
        Self(err)
    }
}

impl IntoResponse for ApiError {
    fn into_response(self) -> Response {
        let (status, message) = match &self.0 {
            CoreError::NotFound(msg) => (StatusCode::NOT_FOUND, msg.clone()),
            CoreError::Conflict(msg) => (StatusCode::CONFLICT, msg.clone()),
            CoreError::Validation(msg) => (StatusCode::BAD_REQUEST, msg.clone()),
            CoreError::Config(msg) => (StatusCode::INTERNAL_SERVER_ERROR, msg.clone()),
            CoreError::Database(e) => {
                tracing::error!("database error: {e}");
                (
                    StatusCode::INTERNAL_SERVER_ERROR,
                    "internal server error".to_string(),
                )
            }
            CoreError::Migration(e) => {
                tracing::error!("migration error: {e}");
                (
                    StatusCode::INTERNAL_SERVER_ERROR,
                    "internal server error".to_string(),
                )
            }
            CoreError::Internal(e) => {
                tracing::error!("internal error: {e}");
                (
                    StatusCode::INTERNAL_SERVER_ERROR,
                    "internal server error".to_string(),
                )
            }
        };

        (status, Json(json!({ "error": message }))).into_response()
    }
}

pub type ApiResult<T> = std::result::Result<T, ApiError>;
