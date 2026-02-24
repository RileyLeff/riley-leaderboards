use std::sync::Arc;

use axum::extract::{Path, Query, State};
use axum::http::StatusCode;
use axum::response::IntoResponse;
use axum::Json;

use riley_leaderboards_core::models::{
    CreateEntry, Entry, EntryHistoryItem, LimitParam, Nullable, PaginatedResponse,
    PaginationParams, UpdateEntry,
};
use riley_leaderboards_core::repo::{boards, entries};

use crate::AppState;
use crate::error::ApiResult;

use super::check_metadata_size;

#[utoipa::path(
    post,
    path = "/boards/{slug}/entries",
    params(("slug" = String, Path, description = "Board slug")),
    request_body = CreateEntry,
    responses(
        (status = 201, description = "Entry created", body = Entry),
        (status = 404, description = "Board not found"),
    ),
    security(("bearer_auth" = [])),
    tag = "entries"
)]
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

#[utoipa::path(
    get,
    path = "/boards/{slug}/entries",
    params(
        ("slug" = String, Path, description = "Board slug"),
        PaginationParams,
    ),
    responses(
        (status = 200, description = "Paginated list of entries", body = inline(PaginatedResponse<Entry>)),
        (status = 404, description = "Board not found"),
    ),
    security((), ("bearer_auth" = [])),
    tag = "entries"
)]
pub async fn list(
    State(state): State<Arc<AppState>>,
    Path(board_slug): Path<String>,
    Query(params): Query<PaginationParams>,
) -> ApiResult<impl IntoResponse> {
    let board = boards::get_by_slug(&state.pool, &board_slug).await?;
    let page = entries::list_paginated(&state.pool, board.id, &params).await?;
    Ok(Json(page))
}

#[utoipa::path(
    get,
    path = "/boards/{slug}/entries/{entry_slug}",
    params(
        ("slug" = String, Path, description = "Board slug"),
        ("entry_slug" = String, Path, description = "Entry slug"),
    ),
    responses(
        (status = 200, description = "Entry details", body = Entry),
        (status = 404, description = "Entry not found"),
    ),
    security((), ("bearer_auth" = [])),
    tag = "entries"
)]
pub async fn get(
    State(state): State<Arc<AppState>>,
    Path((board_slug, entry_slug)): Path<(String, String)>,
) -> ApiResult<impl IntoResponse> {
    let board = boards::get_by_slug(&state.pool, &board_slug).await?;
    let entry = entries::get_by_slug(&state.pool, board.id, &entry_slug).await?;
    Ok(Json(entry))
}

#[utoipa::path(
    patch,
    path = "/boards/{slug}/entries/{entry_slug}",
    params(
        ("slug" = String, Path, description = "Board slug"),
        ("entry_slug" = String, Path, description = "Entry slug"),
    ),
    request_body = UpdateEntry,
    responses(
        (status = 200, description = "Entry updated", body = Entry),
        (status = 404, description = "Entry not found"),
    ),
    security(("bearer_auth" = [])),
    tag = "entries"
)]
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

#[utoipa::path(
    delete,
    path = "/boards/{slug}/entries/{entry_slug}",
    params(
        ("slug" = String, Path, description = "Board slug"),
        ("entry_slug" = String, Path, description = "Entry slug"),
    ),
    responses(
        (status = 204, description = "Entry deleted"),
        (status = 404, description = "Entry not found"),
        (status = 409, description = "Entry has placements"),
    ),
    security(("bearer_auth" = [])),
    tag = "entries"
)]
pub async fn delete(
    State(state): State<Arc<AppState>>,
    Path((board_slug, entry_slug)): Path<(String, String)>,
) -> ApiResult<impl IntoResponse> {
    let board = boards::get_by_slug(&state.pool, &board_slug).await?;
    entries::delete(&state.pool, board.id, &entry_slug).await?;
    Ok(StatusCode::NO_CONTENT)
}

#[utoipa::path(
    get,
    path = "/boards/{slug}/entries/{entry_slug}/history",
    params(
        ("slug" = String, Path, description = "Board slug"),
        ("entry_slug" = String, Path, description = "Entry slug"),
        LimitParam,
    ),
    responses(
        (status = 200, description = "Entry history across versions", body = Vec<EntryHistoryItem>),
        (status = 404, description = "Entry not found"),
    ),
    security((), ("bearer_auth" = [])),
    tag = "entries"
)]
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
