use std::sync::Arc;

use axum::extract::{Path, State};
use axum::http::StatusCode;
use axum::response::IntoResponse;
use axum::Json;

use riley_leaderboards_core::config::WebhookEvent;
use riley_leaderboards_core::error::Error as CoreError;
use riley_leaderboards_core::models::{SnapshotInput, SubmitScore};
use riley_leaderboards_core::repo::{boards, realtime, scores};

use crate::AppState;
use crate::error::ApiResult;
use crate::outbound_webhooks;

pub async fn submit(
    State(state): State<Arc<AppState>>,
    Path(board_slug): Path<String>,
    Json(input): Json<SubmitScore>,
) -> ApiResult<impl IntoResponse> {
    let board = boards::get_by_slug(&state.pool, &board_slug).await?;

    if board.realtime {
        let mut redis = state.redis.clone().ok_or(CoreError::ServiceUnavailable(
            "Redis is required for realtime boards but not configured".to_string(),
        ))?;
        realtime::submit(&state.pool, &mut redis, &board, &input).await?;

        // Publish SSE score.updated event (debounced per-board)
        if let Some(ref event_bus) = state.event_bus {
            event_bus.publish_score(
                &board.slug,
                input.entry_slug.clone(),
                input.entry_name.clone(),
                input.score,
            );
        }

        Ok((StatusCode::OK, Json(serde_json::json!({ "ok": true }))))
    } else {
        let score = scores::submit(&state.pool, &board, &input).await?;
        Ok((StatusCode::OK, Json(serde_json::to_value(score).map_err(|e| CoreError::Internal(format!("serialization error: {e}")))?)))
    }
}

pub async fn snapshot(
    State(state): State<Arc<AppState>>,
    Path(board_slug): Path<String>,
    Json(input): Json<SnapshotInput>,
) -> ApiResult<impl IntoResponse> {
    let board = boards::get_by_slug(&state.pool, &board_slug).await?;

    let version = if board.realtime {
        let mut redis = state.redis.clone().ok_or(CoreError::ServiceUnavailable(
            "Redis is required for realtime boards but not configured".to_string(),
        ))?;
        realtime::snapshot(
            &state.pool,
            &mut redis,
            &board,
            input.note.as_deref(),
            input.metadata.as_ref(),
        )
        .await?
    } else {
        scores::snapshot(
            &state.pool,
            &board,
            input.note.as_deref(),
            input.metadata.as_ref(),
        )
        .await?
    };

    outbound_webhooks::fire(
        &state.config.webhooks,
        WebhookEvent::VersionCreated,
        &board.slug,
        &board.name,
        Some(outbound_webhooks::VersionInfo {
            version_number: version.version.version_number,
            note: version.version.note.clone(),
        }),
    );

    // Publish SSE version.created event
    if let Some(ref event_bus) = state.event_bus {
        event_bus.publish_version(
            &board.slug,
            version.version.version_number,
            version.version.note.clone(),
        );
    }

    Ok((StatusCode::CREATED, Json(serde_json::to_value(version).map_err(|e| CoreError::Internal(format!("serialization error: {e}")))?)))
}
