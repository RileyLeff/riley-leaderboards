//! WebSocket transport for live board updates.
//!
//! Bidirectional: server pushes `BoardEvent`s (same as SSE), clients can
//! submit scores by sending JSON text frames with `SubmitScore` payloads.

use std::sync::Arc;
use std::time::Duration;

use axum::extract::ws::{Message, WebSocket, WebSocketUpgrade};
use axum::extract::{Path, State};
use axum::response::Response;
use tokio::sync::broadcast;
use tokio_stream::StreamExt;
use tokio_stream::wrappers::BroadcastStream;

use crate::AppState;
use crate::error::ApiResult;
use crate::sse::{BoardEvent, ConnectionGuard};
use riley_leaderboards_core::error::Error as CoreError;
use riley_leaderboards_core::models::SubmitScore;

/// WebSocket stream handler: `GET /boards/:slug/ws`
pub async fn stream(
    ws: WebSocketUpgrade,
    State(state): State<Arc<AppState>>,
    Path(board_slug): Path<String>,
    headers: axum::http::HeaderMap,
) -> ApiResult<Response> {
    // Check WS is enabled
    let ws_enabled = state.config.server.as_ref().is_some_and(|s| s.ws_enabled);
    if !ws_enabled {
        return Err(CoreError::ServiceUnavailable(
            "WebSocket streaming is not enabled".to_string(),
        )
        .into());
    }

    // Verify board exists
    riley_leaderboards_core::repo::boards::get_by_slug(&state.pool, &board_slug).await?;

    let event_bus = state
        .event_bus
        .as_ref()
        .ok_or(CoreError::ServiceUnavailable(
            "WebSocket streaming is not enabled".to_string(),
        ))?;

    let (rx, guard) = event_bus.subscribe(&board_slug)?;

    let timeout_secs = state
        .config
        .server
        .as_ref()
        .map(|s| s.ws_timeout_secs)
        .unwrap_or(1800);

    // Determine write authorization at upgrade time (WS upgrade is a GET,
    // so the auth middleware only checks read access). Score submissions
    // within the WS session require write-level auth.
    let write_authorized = crate::auth::has_write_auth(&state, &headers).await;

    Ok(ws.on_upgrade(move |socket| {
        handle_socket(
            socket,
            rx,
            guard,
            timeout_secs,
            state,
            board_slug,
            write_authorized,
        )
    }))
}

async fn handle_socket(
    mut socket: WebSocket,
    rx: broadcast::Receiver<BoardEvent>,
    _guard: ConnectionGuard,
    timeout_secs: u64,
    state: Arc<AppState>,
    board_slug: String,
    write_authorized: bool,
) {
    let events = BroadcastStream::new(rx);
    let mut events = std::pin::pin!(events);

    let deadline = if timeout_secs > 0 {
        Duration::from_secs(timeout_secs)
    } else {
        Duration::from_secs(u32::MAX as u64)
    };
    let sleep = tokio::time::sleep(deadline);
    tokio::pin!(sleep);

    loop {
        tokio::select! {
            _ = &mut sleep => {
                let _ = socket.send(Message::Close(None)).await;
                break;
            }
            msg = socket.recv() => {
                match msg {
                    Some(Ok(Message::Close(_))) | None | Some(Err(_)) => break,
                    Some(Ok(Message::Text(text))) => {
                        let resp = process_score(&state, &board_slug, &text, write_authorized).await;
                        if socket.send(Message::Text(resp.into())).await.is_err() {
                            break;
                        }
                    }
                    _ => {} // Binary frames ignored; pings handled by axum
                }
            }
            Some(result) = events.next() => {
                match result {
                    Ok(event) => {
                        if let Ok(json) = serde_json::to_string(&event)
                            && socket.send(Message::Text(json.into())).await.is_err()
                        {
                            break;
                        }
                    }
                    Err(_) => continue, // Lagged — drop missed events
                }
            }
            else => break,
        }
    }
    // _guard drops here, decrementing connection count
}

/// Process a client-submitted score over WebSocket.
///
/// Returns a JSON response string: `{"type":"ok"}` on success,
/// `{"type":"error","message":"..."}` on failure.
async fn process_score(
    state: &AppState,
    board_slug: &str,
    text: &str,
    write_authorized: bool,
) -> String {
    match try_process_score(state, board_slug, text, write_authorized).await {
        Ok(()) => r#"{"type":"ok"}"#.to_string(),
        Err(msg) => serde_json::json!({"type": "error", "message": msg}).to_string(),
    }
}

async fn try_process_score(
    state: &AppState,
    board_slug: &str,
    text: &str,
    write_authorized: bool,
) -> Result<(), String> {
    if !write_authorized {
        return Err("unauthorized: score submission requires write access".to_string());
    }

    let input: SubmitScore =
        serde_json::from_str(text).map_err(|e| format!("invalid score payload: {e}"))?;

    let board = riley_leaderboards_core::repo::boards::get_by_slug(&state.pool, board_slug)
        .await
        .map_err(|e| core_error_message(&e))?;

    if !board.realtime {
        return Err("score submission over WebSocket requires a realtime board".to_string());
    }

    let mut redis = state
        .redis
        .clone()
        .ok_or_else(|| "service unavailable".to_string())?;
    let prefix = state.config.redis_key_prefix();

    riley_leaderboards_core::repo::realtime::submit(
        &state.pool,
        &mut redis,
        &board,
        &input,
        prefix,
    )
    .await
    .map_err(|e| core_error_message(&e))?;

    metrics::counter!("scores_submitted_total", "board" => board.slug.clone()).increment(1);

    if let Some(ref event_bus) = state.event_bus {
        event_bus.publish_score(&board.slug, input.entry_slug, input.entry_name, input.score);
    }

    Ok(())
}

fn core_error_message(e: &CoreError) -> String {
    match e {
        CoreError::Validation(msg) | CoreError::NotFound(msg) => msg.clone(),
        CoreError::ServiceUnavailable(_) => "service unavailable".to_string(),
        _ => "internal error".to_string(),
    }
}
