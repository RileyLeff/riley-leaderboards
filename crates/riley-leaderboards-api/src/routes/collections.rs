use std::sync::Arc;

use axum::extract::{Path, Query, State};
use axum::http::StatusCode;
use axum::response::IntoResponse;
use axum::Json;

use riley_leaderboards_core::models::{
    AddBoardToCollection, Collection, CollectionWithBoards, CreateCollection, Nullable,
    PaginatedResponse, PaginationParams, UpdateCollection,
};
use riley_leaderboards_core::repo::collections;

use crate::AppState;
use crate::error::ApiResult;

use super::check_metadata_size;

#[utoipa::path(
    post,
    path = "/collections",
    request_body = CreateCollection,
    responses(
        (status = 201, description = "Collection created", body = Collection),
        (status = 400, description = "Validation error"),
        (status = 409, description = "Slug already exists"),
    ),
    security(("bearer_auth" = [])),
    tag = "collections"
)]
pub async fn create(
    State(state): State<Arc<AppState>>,
    Json(input): Json<CreateCollection>,
) -> ApiResult<impl IntoResponse> {
    let limits = state.config.effective_limits();
    check_metadata_size(input.metadata.as_ref(), limits.max_metadata_size_bytes)?;
    let collection = collections::create(&state.pool, &input).await?;
    Ok((StatusCode::CREATED, Json(collection)))
}

#[utoipa::path(
    get,
    path = "/collections",
    params(PaginationParams),
    responses(
        (status = 200, description = "Paginated list of collections", body = inline(PaginatedResponse<Collection>)),
    ),
    security(("bearer_auth" = [])),
    tag = "collections"
)]
pub async fn list(
    State(state): State<Arc<AppState>>,
    Query(params): Query<PaginationParams>,
) -> ApiResult<impl IntoResponse> {
    let page = collections::list_paginated(&state.pool, &params).await?;
    Ok(Json(page))
}

#[utoipa::path(
    get,
    path = "/collections/{slug}",
    params(("slug" = String, Path, description = "Collection slug")),
    responses(
        (status = 200, description = "Collection with boards", body = CollectionWithBoards),
        (status = 404, description = "Collection not found"),
    ),
    security(("bearer_auth" = [])),
    tag = "collections"
)]
pub async fn get(
    State(state): State<Arc<AppState>>,
    Path(slug): Path<String>,
) -> ApiResult<impl IntoResponse> {
    let collection = collections::get_with_boards(&state.pool, &slug).await?;
    Ok(Json(collection))
}

#[utoipa::path(
    patch,
    path = "/collections/{slug}",
    params(("slug" = String, Path, description = "Collection slug")),
    request_body = UpdateCollection,
    responses(
        (status = 200, description = "Collection updated", body = Collection),
        (status = 404, description = "Collection not found"),
    ),
    security(("bearer_auth" = [])),
    tag = "collections"
)]
pub async fn update(
    State(state): State<Arc<AppState>>,
    Path(slug): Path<String>,
    Json(input): Json<UpdateCollection>,
) -> ApiResult<impl IntoResponse> {
    // No-op PATCH: if all fields are absent, return current collection without touching DB
    if input.name.is_none() && matches!(input.metadata, Nullable::Absent) {
        let collection = collections::get_by_slug(&state.pool, &slug).await?;
        return Ok(Json(collection));
    }

    let limits = state.config.effective_limits();
    if let Nullable::Value(ref meta) = input.metadata {
        check_metadata_size(Some(meta), limits.max_metadata_size_bytes)?;
    }
    let collection = collections::update(&state.pool, &slug, &input).await?;
    Ok(Json(collection))
}

#[utoipa::path(
    delete,
    path = "/collections/{slug}",
    params(("slug" = String, Path, description = "Collection slug")),
    responses(
        (status = 204, description = "Collection deleted"),
        (status = 404, description = "Collection not found"),
    ),
    security(("bearer_auth" = [])),
    tag = "collections"
)]
pub async fn delete(
    State(state): State<Arc<AppState>>,
    Path(slug): Path<String>,
) -> ApiResult<impl IntoResponse> {
    collections::delete(&state.pool, &slug).await?;
    Ok(StatusCode::NO_CONTENT)
}

#[utoipa::path(
    post,
    path = "/collections/{slug}/boards",
    params(("slug" = String, Path, description = "Collection slug")),
    request_body = AddBoardToCollection,
    responses(
        (status = 201, description = "Board added to collection"),
        (status = 404, description = "Collection or board not found"),
    ),
    security(("bearer_auth" = [])),
    tag = "collections"
)]
pub async fn add_board(
    State(state): State<Arc<AppState>>,
    Path(slug): Path<String>,
    Json(input): Json<AddBoardToCollection>,
) -> ApiResult<impl IntoResponse> {
    collections::add_board(&state.pool, &slug, &input).await?;
    Ok(StatusCode::CREATED)
}

#[utoipa::path(
    delete,
    path = "/collections/{slug}/boards/{board_slug}",
    params(
        ("slug" = String, Path, description = "Collection slug"),
        ("board_slug" = String, Path, description = "Board slug to remove"),
    ),
    responses(
        (status = 204, description = "Board removed from collection"),
        (status = 404, description = "Collection or board not found"),
    ),
    security(("bearer_auth" = [])),
    tag = "collections"
)]
pub async fn remove_board(
    State(state): State<Arc<AppState>>,
    Path((slug, board_slug)): Path<(String, String)>,
) -> ApiResult<impl IntoResponse> {
    collections::remove_board(&state.pool, &slug, &board_slug).await?;
    Ok(StatusCode::NO_CONTENT)
}
