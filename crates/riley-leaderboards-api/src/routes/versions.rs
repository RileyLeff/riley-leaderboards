use std::collections::HashMap;
use std::sync::Arc;

use axum::extract::{Path, Query, State};
use axum::http::StatusCode;
use axum::response::IntoResponse;
use axum::Json;

use riley_leaderboards_core::models::{CreateVersion, PaginationParams};
use riley_leaderboards_core::repo::{boards, versions};

use crate::AppState;
use crate::error::ApiResult;

pub async fn create(
    State(state): State<Arc<AppState>>,
    Path(board_slug): Path<String>,
    Json(input): Json<CreateVersion>,
) -> ApiResult<impl IntoResponse> {
    let board = boards::get_by_slug(&state.pool, &board_slug).await?;
    let version = versions::create(&state.pool, &board, &input).await?;
    Ok((StatusCode::CREATED, Json(version)))
}

pub async fn list(
    State(state): State<Arc<AppState>>,
    Path(board_slug): Path<String>,
    Query(params): Query<PaginationParams>,
) -> ApiResult<impl IntoResponse> {
    let board = boards::get_by_slug(&state.pool, &board_slug).await?;
    let page = versions::list_paginated(&state.pool, board.id, &params).await?;
    Ok(Json(page))
}

pub async fn get(
    State(state): State<Arc<AppState>>,
    Path((board_slug, version_number)): Path<(String, i32)>,
) -> ApiResult<impl IntoResponse> {
    let board = boards::get_by_slug(&state.pool, &board_slug).await?;
    let version = versions::get_by_number(&state.pool, board.id, version_number).await?;
    Ok(Json(version))
}

pub async fn latest(
    State(state): State<Arc<AppState>>,
    Path(board_slug): Path<String>,
) -> ApiResult<impl IntoResponse> {
    let board = boards::get_by_slug(&state.pool, &board_slug).await?;
    let version = versions::get_latest(&state.pool, board.id).await?;
    Ok(Json(version))
}

pub async fn diff(
    State(state): State<Arc<AppState>>,
    Path(board_slug): Path<String>,
    Query(params): Query<HashMap<String, i32>>,
) -> ApiResult<impl IntoResponse> {
    let from = *params
        .get("from")
        .ok_or(riley_leaderboards_core::error::Error::Validation(
            "missing required query parameter 'from'".to_string(),
        ))?;
    let to = *params
        .get("to")
        .ok_or(riley_leaderboards_core::error::Error::Validation(
            "missing required query parameter 'to'".to_string(),
        ))?;

    if from < 1 || to < 1 {
        return Err(riley_leaderboards_core::error::Error::Validation(
            "version numbers must be >= 1".to_string(),
        ).into());
    }
    if from >= to {
        return Err(riley_leaderboards_core::error::Error::Validation(
            "'from' must be less than 'to'".to_string(),
        ).into());
    }

    let board = boards::get_by_slug(&state.pool, &board_slug).await?;
    let version_diff = versions::diff(&state.pool, board.id, from, to).await?;
    Ok(Json(version_diff))
}

pub async fn since(
    State(state): State<Arc<AppState>>,
    Path((board_slug, version_number)): Path<(String, i32)>,
) -> ApiResult<impl IntoResponse> {
    let board = boards::get_by_slug(&state.pool, &board_slug).await?;
    let versions = versions::since(&state.pool, board.id, version_number).await?;
    Ok(Json(versions))
}
