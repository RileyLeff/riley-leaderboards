//! WebSocket transport for live board updates.
//!
//! Alternative to SSE — same events from the same EventBus,
//! delivered as JSON text frames over a WebSocket connection.

use std::sync::Arc;
use std::time::Duration;

use axum::extract::ws::{Message, WebSocket, WebSocketUpgrade};
use axum::extract::{Path, State};
use axum::response::Response;
use tokio::sync::broadcast;
use tokio_stream::wrappers::BroadcastStream;
use tokio_stream::StreamExt;

use crate::error::ApiResult;
use crate::sse::{BoardEvent, ConnectionGuard};
use crate::AppState;
use riley_leaderboards_core::error::Error as CoreError;

/// WebSocket stream handler: `GET /boards/:slug/ws`
pub async fn stream(
    ws: WebSocketUpgrade,
    State(state): State<Arc<AppState>>,
    Path(board_slug): Path<String>,
) -> ApiResult<Response> {
    // Check WS is enabled
    let ws_enabled = state
        .config
        .server
        .as_ref()
        .is_some_and(|s| s.ws_enabled);
    if !ws_enabled {
        return Err(CoreError::ServiceUnavailable(
            "WebSocket streaming is not enabled".to_string(),
        )
        .into());
    }

    // Verify board exists
    riley_leaderboards_core::repo::boards::get_by_slug(&state.pool, &board_slug).await?;

    let event_bus = state.event_bus.as_ref().ok_or(CoreError::ServiceUnavailable(
        "WebSocket streaming is not enabled".to_string(),
    ))?;

    let (rx, guard) = event_bus.subscribe(&board_slug)?;

    let timeout_secs = state
        .config
        .server
        .as_ref()
        .map(|s| s.ws_timeout_secs)
        .unwrap_or(1800);

    Ok(ws.on_upgrade(move |socket| handle_socket(socket, rx, guard, timeout_secs)))
}

async fn handle_socket(
    mut socket: WebSocket,
    rx: broadcast::Receiver<BoardEvent>,
    _guard: ConnectionGuard,
    timeout_secs: u64,
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
                    // Client sent close or connection dropped
                    Some(Ok(Message::Close(_))) | None | Some(Err(_)) => break,
                    // Ignore unsolicited text/binary; protocol pings handled by axum
                    _ => {}
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
