use std::sync::Arc;

use axum::Json;
use axum::extract::{Path, Query, State};
use axum::http::StatusCode;
use axum::response::IntoResponse;
use uuid::Uuid;

use riley_leaderboards_core::models::{
    BoardReference, CreateReference, PaginatedResponse, PaginationParams,
};
use riley_leaderboards_core::repo::{boards, references};

use crate::AppState;
use crate::error::ApiResult;

#[utoipa::path(
    post,
    path = "/boards/{slug}/references",
    params(("slug" = String, Path, description = "Board slug")),
    request_body = CreateReference,
    responses(
        (status = 201, description = "Reference created", body = BoardReference),
        (status = 404, description = "Board not found"),
    ),
    security(("bearer_auth" = [])),
    tag = "references"
)]
pub async fn create(
    State(state): State<Arc<AppState>>,
    Path(board_slug): Path<String>,
    Json(input): Json<CreateReference>,
) -> ApiResult<impl IntoResponse> {
    let board = boards::get_by_slug(&state.pool, &board_slug).await?;
    let reference = references::create(&state.pool, board.id, &input).await?;
    Ok((StatusCode::CREATED, Json(reference)))
}

#[utoipa::path(
    get,
    path = "/boards/{slug}/references",
    params(
        ("slug" = String, Path, description = "Board slug"),
        PaginationParams,
    ),
    responses(
        (status = 200, description = "Paginated list of references", body = inline(PaginatedResponse<BoardReference>)),
        (status = 404, description = "Board not found"),
    ),
    security((), ("bearer_auth" = [])),
    tag = "references"
)]
pub async fn list(
    State(state): State<Arc<AppState>>,
    Path(board_slug): Path<String>,
    Query(params): Query<PaginationParams>,
) -> ApiResult<impl IntoResponse> {
    let board = boards::get_by_slug(&state.pool, &board_slug).await?;
    let page = references::list_paginated(&state.pool, board.id, &params).await?;
    Ok(Json(page))
}

#[utoipa::path(
    delete,
    path = "/boards/{slug}/references/{reference_id}",
    params(
        ("slug" = String, Path, description = "Board slug"),
        ("reference_id" = Uuid, Path, description = "Reference UUID"),
    ),
    responses(
        (status = 204, description = "Reference deleted"),
        (status = 404, description = "Board or reference not found"),
    ),
    security(("bearer_auth" = [])),
    tag = "references"
)]
pub async fn delete(
    State(state): State<Arc<AppState>>,
    Path((board_slug, reference_id)): Path<(String, Uuid)>,
) -> ApiResult<impl IntoResponse> {
    let board = boards::get_by_slug(&state.pool, &board_slug).await?;
    references::delete(&state.pool, board.id, reference_id).await?;
    Ok(StatusCode::NO_CONTENT)
}
