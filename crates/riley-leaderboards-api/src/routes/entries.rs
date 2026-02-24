use std::sync::Arc;

use axum::extract::{Path, Query, State};
use axum::http::StatusCode;
use axum::response::IntoResponse;
use axum::Json;

use riley_leaderboards_core::models::{CreateEntry, LimitParam, Nullable, PaginationParams, UpdateEntry};
use riley_leaderboards_core::repo::{boards, entries};

use crate::AppState;
use crate::error::ApiResult;

use super::check_metadata_size;

pub async fn create(
    State(state): State<Arc<AppState>>,
    Path(board_slug): Path<String>,
    Json(input): Json<CreateEntry>,
) -> ApiResult<impl IntoResponse> {
    let limits = state.config.effective_limits();
    check_metadata_size(input.metadata.as_ref(), limits.max_metadata_size_bytes)?;
    let board = boards::get_by_slug(&state.pool, &board_slug).await?;
    let entry = entries::create(&state.pool, board.id, &input).await?;
    Ok((StatusCode::CREATED, Json(entry)))
}

pub async fn list(
    State(state): State<Arc<AppState>>,
    Path(board_slug): Path<String>,
    Query(params): Query<PaginationParams>,
) -> ApiResult<impl IntoResponse> {
    let board = boards::get_by_slug(&state.pool, &board_slug).await?;
    let page = entries::list_paginated(&state.pool, board.id, &params).await?;
    Ok(Json(page))
}

pub async fn get(
    State(state): State<Arc<AppState>>,
    Path((board_slug, entry_slug)): Path<(String, String)>,
) -> ApiResult<impl IntoResponse> {
    let board = boards::get_by_slug(&state.pool, &board_slug).await?;
    let entry = entries::get_by_slug(&state.pool, board.id, &entry_slug).await?;
    Ok(Json(entry))
}

pub async fn update(
    State(state): State<Arc<AppState>>,
    Path((board_slug, entry_slug)): Path<(String, String)>,
    Json(input): Json<UpdateEntry>,
) -> ApiResult<impl IntoResponse> {
    let limits = state.config.effective_limits();
    if let Nullable::Value(ref meta) = input.metadata {
        check_metadata_size(Some(meta), limits.max_metadata_size_bytes)?;
    }
    let board = boards::get_by_slug(&state.pool, &board_slug).await?;
    let entry = entries::update(&state.pool, board.id, &entry_slug, &input).await?;
    Ok(Json(entry))
}

pub async fn delete(
    State(state): State<Arc<AppState>>,
    Path((board_slug, entry_slug)): Path<(String, String)>,
) -> ApiResult<impl IntoResponse> {
    let board = boards::get_by_slug(&state.pool, &board_slug).await?;
    entries::delete(&state.pool, board.id, &entry_slug).await?;
    Ok(StatusCode::NO_CONTENT)
}

pub async fn history(
    State(state): State<Arc<AppState>>,
    Path((board_slug, entry_slug)): Path<(String, String)>,
    Query(params): Query<LimitParam>,
) -> ApiResult<impl IntoResponse> {
    let board = boards::get_by_slug(&state.pool, &board_slug).await?;
    let items =
        entries::history(&state.pool, board.id, &entry_slug, params.effective_limit()).await?;
    Ok(Json(items))
}
