use std::sync::Arc;

use axum::extract::{Path, Query, State};
use axum::http::StatusCode;
use axum::response::IntoResponse;
use axum::Json;

use serde::Deserialize;

use riley_leaderboards_core::config::WebhookEvent;
use riley_leaderboards_core::error::Error as CoreError;
use riley_leaderboards_core::models::{CreateVersion, PaginationParams};
use riley_leaderboards_core::repo::{boards, realtime, versions};

use crate::AppState;
use crate::error::ApiResult;
use crate::outbound_webhooks;

#[derive(Deserialize)]
pub struct DiffParams {
    pub from: i32,
    pub to: i32,
}

pub async fn create(
    State(state): State<Arc<AppState>>,
    Path(board_slug): Path<String>,
    Json(input): Json<CreateVersion>,
) -> ApiResult<impl IntoResponse> {
    let limits = state.config.effective_limits();

    // Safety limit: max entries per version
    if input.placements.len() > limits.max_entries_per_version {
        return Err(CoreError::Validation(format!(
            "too many placements ({}, max {})",
            input.placements.len(),
            limits.max_entries_per_version,
        ))
        .into());
    }

    // Safety limit: metadata size
    if let Some(ref meta) = input.metadata {
        let size = serde_json::to_string(meta)
            .map(|s| s.len())
            .unwrap_or(0);
        if size > limits.max_metadata_size_bytes {
            return Err(CoreError::Validation(format!(
                "metadata too large ({size} bytes, max {})",
                limits.max_metadata_size_bytes,
            ))
            .into());
        }
    }

    let board = boards::get_by_slug(&state.pool, &board_slug).await?;

    // Safety limit: max versions per board
    let version_count: (i64,) =
        sqlx::query_as("SELECT count(*) FROM versions WHERE board_id = $1")
            .bind(board.id)
            .fetch_one(&state.pool)
            .await
            .map_err(CoreError::Database)?;
    if version_count.0 >= limits.max_versions_per_board {
        return Err(CoreError::Validation(format!(
            "board has too many versions ({}, max {})",
            version_count.0, limits.max_versions_per_board,
        ))
        .into());
    }

    let version = versions::create(&state.pool, &board, &input).await?;
    let _ = outbound_webhooks::fire(
        &state.config.webhooks,
        WebhookEvent::VersionCreated,
        &board.slug,
        &board.name,
        Some(outbound_webhooks::VersionInfo {
            version_number: version.version.version_number,
            note: version.version.note.clone(),
        }),
        Some(version.version.created_at),
    );

    // Publish SSE version.created event
    if let Some(ref event_bus) = state.event_bus {
        event_bus.publish_version(
            &board.slug,
            version.version.version_number,
            version.version.note.clone(),
        );
    }

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

    if board.realtime {
        let mut redis = state.redis.clone().ok_or(CoreError::ServiceUnavailable(
            "Redis is required for realtime boards but not configured".to_string(),
        ))?;
        let prefix = state.config.redis_key_prefix();
        let standings = realtime::latest(&mut redis, &board, prefix).await?;
        return Ok(Json(standings));
    }

    let version = versions::get_latest(&state.pool, board.id).await?;
    Ok(Json(version))
}

pub async fn diff(
    State(state): State<Arc<AppState>>,
    Path(board_slug): Path<String>,
    Query(params): Query<DiffParams>,
) -> ApiResult<impl IntoResponse> {
    let from = params.from;
    let to = params.to;

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
