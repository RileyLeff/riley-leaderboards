use std::sync::Arc;

use axum::extract::{Path, Query, State};
use axum::http::StatusCode;
use axum::response::IntoResponse;
use axum::Json;

use serde::Deserialize;

use riley_leaderboards_core::config::WebhookEvent;
use riley_leaderboards_core::error::Error as CoreError;
use riley_leaderboards_core::models::{
    CreateVersion, LimitParam, PaginatedResponse, PaginationParams, Version, VersionDiff,
    VersionWithPlacements,
};
use riley_leaderboards_core::repo::{boards, realtime, versions};

use crate::AppState;
use crate::error::ApiResult;
use crate::outbound_webhooks;

use super::{check_metadata_size, check_note_size};

#[derive(Deserialize, utoipa::IntoParams)]
pub struct DiffParams {
    pub from: i32,
    pub to: i32,
}

#[utoipa::path(
    post,
    path = "/boards/{slug}/versions",
    params(("slug" = String, Path, description = "Board slug")),
    request_body = CreateVersion,
    responses(
        (status = 201, description = "Version created", body = VersionWithPlacements),
        (status = 400, description = "Validation error"),
        (status = 404, description = "Board not found"),
    ),
    security(("bearer_auth" = [])),
    tag = "versions"
)]
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

    // Safety limits: note size, metadata size (version-level and per-placement)
    check_note_size(input.note.as_deref(), limits.max_note_size_bytes)?;
    check_metadata_size(input.metadata.as_ref(), limits.max_metadata_size_bytes)?;
    for p in &input.placements {
        check_metadata_size(p.metadata.as_ref(), limits.max_metadata_size_bytes)?;
    }

    let board = boards::get_by_slug(&state.pool, &board_slug).await?;

    // max_versions_per_board is checked inside versions::create (after FOR UPDATE lock)
    let version = versions::create(
        &state.pool,
        &board,
        &input,
        Some(limits.max_versions_per_board),
    )
    .await?;
    metrics::counter!("versions_created_total", "board" => board.slug.clone()).increment(1);
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
        Some(&state.task_tracker),
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

#[utoipa::path(
    get,
    path = "/boards/{slug}/versions",
    params(
        ("slug" = String, Path, description = "Board slug"),
        PaginationParams,
    ),
    responses(
        (status = 200, description = "Paginated list of versions", body = inline(PaginatedResponse<Version>)),
        (status = 404, description = "Board not found"),
    ),
    security((), ("bearer_auth" = [])),
    tag = "versions"
)]
pub async fn list(
    State(state): State<Arc<AppState>>,
    Path(board_slug): Path<String>,
    Query(params): Query<PaginationParams>,
) -> ApiResult<impl IntoResponse> {
    let board = boards::get_by_slug(&state.pool, &board_slug).await?;
    let page = versions::list_paginated(&state.pool, board.id, &params).await?;
    Ok(Json(page))
}

#[utoipa::path(
    get,
    path = "/boards/{slug}/versions/{version_number}",
    params(
        ("slug" = String, Path, description = "Board slug"),
        ("version_number" = i32, Path, description = "Version number"),
    ),
    responses(
        (status = 200, description = "Version with placements", body = VersionWithPlacements),
        (status = 404, description = "Board or version not found"),
    ),
    security((), ("bearer_auth" = [])),
    tag = "versions"
)]
pub async fn get(
    State(state): State<Arc<AppState>>,
    Path((board_slug, version_number)): Path<(String, i32)>,
) -> ApiResult<impl IntoResponse> {
    let board = boards::get_by_slug(&state.pool, &board_slug).await?;
    let version = versions::get_by_number(&state.pool, board.id, version_number).await?;
    Ok(Json(version))
}

#[utoipa::path(
    get,
    path = "/boards/{slug}/latest",
    params(("slug" = String, Path, description = "Board slug")),
    responses(
        (status = 200, description = "Latest version with placements", body = VersionWithPlacements),
        (status = 404, description = "Board not found or no versions"),
    ),
    security((), ("bearer_auth" = [])),
    tag = "versions"
)]
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

#[utoipa::path(
    get,
    path = "/boards/{slug}/diff",
    params(
        ("slug" = String, Path, description = "Board slug"),
        DiffParams,
    ),
    responses(
        (status = 200, description = "Diff between two versions", body = VersionDiff),
        (status = 400, description = "Validation error"),
        (status = 404, description = "Board or version not found"),
    ),
    security((), ("bearer_auth" = [])),
    tag = "versions"
)]
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

#[utoipa::path(
    get,
    path = "/boards/{slug}/since/{version_number}",
    params(
        ("slug" = String, Path, description = "Board slug"),
        ("version_number" = i32, Path, description = "Version number to fetch since"),
        LimitParam,
    ),
    responses(
        (status = 200, description = "Versions since given number", body = Vec<Version>),
        (status = 404, description = "Board not found"),
    ),
    security((), ("bearer_auth" = [])),
    tag = "versions"
)]
pub async fn since(
    State(state): State<Arc<AppState>>,
    Path((board_slug, version_number)): Path<(String, i32)>,
    Query(params): Query<LimitParam>,
) -> ApiResult<impl IntoResponse> {
    let board = boards::get_by_slug(&state.pool, &board_slug).await?;
    let versions =
        versions::since(&state.pool, board.id, version_number, params.effective_limit()).await?;
    Ok(Json(versions))
}
