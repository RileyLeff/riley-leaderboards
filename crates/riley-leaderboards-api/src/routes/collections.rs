use std::sync::Arc;

use axum::extract::{Path, Query, State};
use axum::http::StatusCode;
use axum::response::IntoResponse;
use axum::Json;

use riley_leaderboards_core::models::{
    AddBoardToCollection, CreateCollection, PaginationParams, UpdateCollection,
};
use riley_leaderboards_core::repo::collections;

use crate::AppState;
use crate::error::ApiResult;

pub async fn create(
    State(state): State<Arc<AppState>>,
    Json(input): Json<CreateCollection>,
) -> ApiResult<impl IntoResponse> {
    let collection = collections::create(&state.pool, &input).await?;
    Ok((StatusCode::CREATED, Json(collection)))
}

pub async fn list(
    State(state): State<Arc<AppState>>,
    Query(params): Query<PaginationParams>,
) -> ApiResult<impl IntoResponse> {
    let page = collections::list_paginated(&state.pool, &params).await?;
    Ok(Json(page))
}

pub async fn get(
    State(state): State<Arc<AppState>>,
    Path(slug): Path<String>,
) -> ApiResult<impl IntoResponse> {
    let collection = collections::get_with_boards(&state.pool, &slug).await?;
    Ok(Json(collection))
}

pub async fn update(
    State(state): State<Arc<AppState>>,
    Path(slug): Path<String>,
    Json(input): Json<UpdateCollection>,
) -> ApiResult<impl IntoResponse> {
    let collection = collections::update(&state.pool, &slug, &input).await?;
    Ok(Json(collection))
}

pub async fn delete(
    State(state): State<Arc<AppState>>,
    Path(slug): Path<String>,
) -> ApiResult<impl IntoResponse> {
    collections::delete(&state.pool, &slug).await?;
    Ok(StatusCode::NO_CONTENT)
}

pub async fn add_board(
    State(state): State<Arc<AppState>>,
    Path(slug): Path<String>,
    Json(input): Json<AddBoardToCollection>,
) -> ApiResult<impl IntoResponse> {
    collections::add_board(&state.pool, &slug, &input).await?;
    Ok(StatusCode::CREATED)
}

pub async fn remove_board(
    State(state): State<Arc<AppState>>,
    Path((slug, board_slug)): Path<(String, String)>,
) -> ApiResult<impl IntoResponse> {
    collections::remove_board(&state.pool, &slug, &board_slug).await?;
    Ok(StatusCode::NO_CONTENT)
}
