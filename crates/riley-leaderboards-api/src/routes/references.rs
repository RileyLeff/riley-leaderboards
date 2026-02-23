use std::sync::Arc;

use axum::extract::{Path, Query, State};
use axum::http::StatusCode;
use axum::response::IntoResponse;
use axum::Json;
use uuid::Uuid;

use riley_leaderboards_core::models::{CreateReference, PaginationParams};
use riley_leaderboards_core::repo::{boards, references};

use crate::AppState;
use crate::error::ApiResult;

pub async fn create(
    State(state): State<Arc<AppState>>,
    Path(board_slug): Path<String>,
    Json(input): Json<CreateReference>,
) -> ApiResult<impl IntoResponse> {
    let board = boards::get_by_slug(&state.pool, &board_slug).await?;
    let reference = references::create(&state.pool, board.id, &input).await?;
    Ok((StatusCode::CREATED, Json(reference)))
}

pub async fn list(
    State(state): State<Arc<AppState>>,
    Path(board_slug): Path<String>,
    Query(params): Query<PaginationParams>,
) -> ApiResult<impl IntoResponse> {
    let board = boards::get_by_slug(&state.pool, &board_slug).await?;
    let page = references::list_paginated(&state.pool, board.id, &params).await?;
    Ok(Json(page))
}

pub async fn delete(
    State(state): State<Arc<AppState>>,
    Path((board_slug, reference_id)): Path<(String, Uuid)>,
) -> ApiResult<impl IntoResponse> {
    let board = boards::get_by_slug(&state.pool, &board_slug).await?;
    references::delete(&state.pool, board.id, reference_id).await?;
    Ok(StatusCode::NO_CONTENT)
}
