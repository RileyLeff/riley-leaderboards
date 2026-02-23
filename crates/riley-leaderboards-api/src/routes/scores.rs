use std::sync::Arc;

use axum::extract::{Path, State};
use axum::http::StatusCode;
use axum::response::IntoResponse;
use axum::Json;

use riley_leaderboards_core::models::{SnapshotInput, SubmitScore};
use riley_leaderboards_core::repo::{boards, scores};

use crate::AppState;
use crate::error::ApiResult;

pub async fn submit(
    State(state): State<Arc<AppState>>,
    Path(board_slug): Path<String>,
    Json(input): Json<SubmitScore>,
) -> ApiResult<impl IntoResponse> {
    let board = boards::get_by_slug(&state.pool, &board_slug).await?;
    let score = scores::submit(&state.pool, &board, &input).await?;
    Ok((StatusCode::OK, Json(score)))
}

pub async fn snapshot(
    State(state): State<Arc<AppState>>,
    Path(board_slug): Path<String>,
    Json(input): Json<SnapshotInput>,
) -> ApiResult<impl IntoResponse> {
    let board = boards::get_by_slug(&state.pool, &board_slug).await?;
    let version = scores::snapshot(&state.pool, &board, input.note.as_deref()).await?;
    Ok((StatusCode::CREATED, Json(version)))
}
