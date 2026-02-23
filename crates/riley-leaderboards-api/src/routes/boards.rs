use std::sync::Arc;

use axum::extract::{Path, State};
use axum::http::StatusCode;
use axum::response::IntoResponse;
use axum::Json;

use riley_leaderboards_core::models::{CreateBoard, UpdateBoard};
use riley_leaderboards_core::repo::boards;

use crate::AppState;
use crate::error::ApiResult;

pub async fn create(
    State(state): State<Arc<AppState>>,
    Json(input): Json<CreateBoard>,
) -> ApiResult<impl IntoResponse> {
    let board = boards::create(&state.pool, &input).await?;
    Ok((StatusCode::CREATED, Json(board)))
}

pub async fn list(State(state): State<Arc<AppState>>) -> ApiResult<impl IntoResponse> {
    let boards = boards::list(&state.pool).await?;
    Ok(Json(boards))
}

pub async fn get(
    State(state): State<Arc<AppState>>,
    Path(slug): Path<String>,
) -> ApiResult<impl IntoResponse> {
    let summary = boards::get_summary(&state.pool, &slug).await?;
    Ok(Json(summary))
}

pub async fn update(
    State(state): State<Arc<AppState>>,
    Path(slug): Path<String>,
    Json(input): Json<UpdateBoard>,
) -> ApiResult<impl IntoResponse> {
    let board = boards::update(&state.pool, &slug, &input).await?;
    Ok(Json(board))
}

pub async fn delete(
    State(state): State<Arc<AppState>>,
    Path(slug): Path<String>,
) -> ApiResult<impl IntoResponse> {
    boards::delete(&state.pool, &slug).await?;
    Ok(StatusCode::NO_CONTENT)
}
