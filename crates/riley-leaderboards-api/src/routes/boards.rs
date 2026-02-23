use std::sync::Arc;

use axum::extract::{Path, Query, State};
use axum::http::StatusCode;
use axum::response::IntoResponse;
use axum::Json;

use riley_leaderboards_core::config::WebhookEvent;
use riley_leaderboards_core::models::{CreateBoard, PaginationParams, UpdateBoard};
use riley_leaderboards_core::repo::boards;

use crate::AppState;
use crate::error::ApiResult;
use crate::outbound_webhooks;

pub async fn create(
    State(state): State<Arc<AppState>>,
    Json(input): Json<CreateBoard>,
) -> ApiResult<impl IntoResponse> {
    let board = boards::create(&state.pool, &input).await?;
    outbound_webhooks::fire(
        &state.config.webhooks,
        WebhookEvent::BoardCreated,
        &board.slug,
        &board.name,
        None,
    );
    Ok((StatusCode::CREATED, Json(board)))
}

pub async fn list(
    State(state): State<Arc<AppState>>,
    Query(params): Query<PaginationParams>,
) -> ApiResult<impl IntoResponse> {
    let page = boards::list_paginated(&state.pool, &params).await?;
    Ok(Json(page))
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
    outbound_webhooks::fire(
        &state.config.webhooks,
        WebhookEvent::BoardUpdated,
        &board.slug,
        &board.name,
        None,
    );
    Ok(Json(board))
}

pub async fn delete(
    State(state): State<Arc<AppState>>,
    Path(slug): Path<String>,
) -> ApiResult<impl IntoResponse> {
    // Fetch board info before deleting (for webhook payload)
    let board = boards::get_by_slug(&state.pool, &slug).await?;
    boards::delete(&state.pool, &slug).await?;
    outbound_webhooks::fire(
        &state.config.webhooks,
        WebhookEvent::BoardDeleted,
        &board.slug,
        &board.name,
        None,
    );
    Ok(StatusCode::NO_CONTENT)
}
