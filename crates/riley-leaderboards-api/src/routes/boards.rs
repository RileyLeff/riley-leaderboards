use std::sync::Arc;

use axum::extract::{Path, Query, State};
use axum::http::StatusCode;
use axum::response::IntoResponse;
use axum::Json;

use riley_leaderboards_core::config::WebhookEvent;
use riley_leaderboards_core::models::{
    Board, BoardSummary, CreateBoard, Nullable, PaginatedResponse, PaginationParams, UpdateBoard,
};
use riley_leaderboards_core::repo::{boards, realtime};

use crate::AppState;
use crate::error::ApiResult;
use crate::outbound_webhooks;

use super::check_metadata_size;

#[utoipa::path(
    post,
    path = "/boards",
    request_body = CreateBoard,
    responses(
        (status = 201, description = "Board created", body = Board),
        (status = 400, description = "Validation error"),
        (status = 409, description = "Slug already exists"),
    ),
    security(("bearer_auth" = [])),
    tag = "boards"
)]
pub async fn create(
    State(state): State<Arc<AppState>>,
    Json(input): Json<CreateBoard>,
) -> ApiResult<impl IntoResponse> {
    let limits = state.config.effective_limits();
    check_metadata_size(input.metadata.as_ref(), limits.max_metadata_size_bytes)?;

    let board = boards::create(&state.pool, &input).await?;
    let _ = outbound_webhooks::fire(
        &state.config.webhooks,
        WebhookEvent::BoardCreated,
        &board.slug,
        &board.name,
        None,
        Some(board.created_at),
        Some(&state.task_tracker),
    );
    Ok((StatusCode::CREATED, Json(board)))
}

#[utoipa::path(
    get,
    path = "/boards",
    params(PaginationParams),
    responses(
        (status = 200, description = "Paginated list of boards", body = inline(PaginatedResponse<Board>)),
    ),
    security((), ("bearer_auth" = [])),
    tag = "boards"
)]
pub async fn list(
    State(state): State<Arc<AppState>>,
    Query(params): Query<PaginationParams>,
) -> ApiResult<impl IntoResponse> {
    let page = boards::list_paginated(&state.pool, &params).await?;
    Ok(Json(page))
}

#[utoipa::path(
    get,
    path = "/boards/{slug}",
    params(("slug" = String, Path, description = "Board slug")),
    responses(
        (status = 200, description = "Board details with summary", body = BoardSummary),
        (status = 404, description = "Board not found"),
    ),
    security((), ("bearer_auth" = [])),
    tag = "boards"
)]
pub async fn get(
    State(state): State<Arc<AppState>>,
    Path(slug): Path<String>,
) -> ApiResult<impl IntoResponse> {
    let summary = boards::get_summary(&state.pool, &slug).await?;
    Ok(Json(summary))
}

#[utoipa::path(
    patch,
    path = "/boards/{slug}",
    params(("slug" = String, Path, description = "Board slug")),
    request_body = UpdateBoard,
    responses(
        (status = 200, description = "Board updated", body = Board),
        (status = 404, description = "Board not found"),
    ),
    security(("bearer_auth" = [])),
    tag = "boards"
)]
pub async fn update(
    State(state): State<Arc<AppState>>,
    Path(slug): Path<String>,
    Json(input): Json<UpdateBoard>,
) -> ApiResult<impl IntoResponse> {
    // No-op PATCH: if all fields are absent, return current board without firing webhook
    if input.name.is_none()
        && input.sort_direction.is_none()
        && matches!(input.tier_config, Nullable::Absent)
        && matches!(input.metadata, Nullable::Absent)
        && input.realtime.is_none()
        && input.clear_on_snapshot.is_none()
    {
        let board = boards::get_by_slug(&state.pool, &slug).await?;
        return Ok(Json(board));
    }

    let limits = state.config.effective_limits();
    if let Nullable::Value(ref meta) = input.metadata {
        check_metadata_size(Some(meta), limits.max_metadata_size_bytes)?;
    }

    let board = boards::update(&state.pool, &slug, &input).await?;
    let _ = outbound_webhooks::fire(
        &state.config.webhooks,
        WebhookEvent::BoardUpdated,
        &board.slug,
        &board.name,
        None,
        Some(board.updated_at),
        Some(&state.task_tracker),
    );
    Ok(Json(board))
}

#[utoipa::path(
    delete,
    path = "/boards/{slug}",
    params(("slug" = String, Path, description = "Board slug")),
    responses(
        (status = 204, description = "Board deleted"),
        (status = 404, description = "Board not found"),
    ),
    security(("bearer_auth" = [])),
    tag = "boards"
)]
pub async fn delete(
    State(state): State<Arc<AppState>>,
    Path(slug): Path<String>,
) -> ApiResult<impl IntoResponse> {
    // Fetch board info before deleting (for webhook payload + Redis cleanup)
    let board = boards::get_by_slug(&state.pool, &slug).await?;

    // Clean up Redis keys for realtime boards before Postgres delete
    if board.realtime
        && let Some(ref redis) = state.redis
    {
        let prefix = state.config.redis_key_prefix();
        let _ = realtime::clear(&mut redis.clone(), &board.slug, prefix).await;
    }

    boards::delete(&state.pool, &slug).await?;
    let _ = outbound_webhooks::fire(
        &state.config.webhooks,
        WebhookEvent::BoardDeleted,
        &board.slug,
        &board.name,
        None,
        None, // board is deleted, no DB timestamp to use
        Some(&state.task_tracker),
    );
    Ok(StatusCode::NO_CONTENT)
}
