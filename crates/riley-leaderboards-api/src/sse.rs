//! SSE (Server-Sent Events) infrastructure for live board updates.
//!
//! Provides a per-board broadcast channel registry and debouncing for
//! high-throughput score.updated events.

use std::collections::HashMap;
use std::convert::Infallible;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, RwLock};
use std::time::Instant;

use axum::extract::{Path, State};
use axum::response::sse::{Event, KeepAlive, Sse};
use serde::Serialize;
use tokio::sync::broadcast;
use tokio_stream::wrappers::BroadcastStream;
use tokio_stream::StreamExt;

use crate::AppState;
use crate::error::ApiResult;
use riley_leaderboards_core::error::Error as CoreError;

/// Events published to SSE subscribers.
#[derive(Debug, Clone, Serialize)]
#[serde(tag = "type")]
pub enum SseEvent {
    #[serde(rename = "version.created")]
    VersionCreated {
        version_number: i32,
        note: Option<String>,
    },
    #[serde(rename = "score.updated")]
    ScoreUpdated {
        entry_slug: String,
        entry_name: String,
        score: f64,
    },
}

impl SseEvent {
    fn event_type(&self) -> &'static str {
        match self {
            Self::VersionCreated { .. } => "version.created",
            Self::ScoreUpdated { .. } => "score.updated",
        }
    }
}

/// Per-board broadcast channel registry with connection tracking and debouncing.
pub struct EventBus {
    channels: RwLock<HashMap<String, broadcast::Sender<SseEvent>>>,
    active_connections: Arc<AtomicUsize>,
    max_connections: usize,
    debounce_ms: u64,
    last_score_event: RwLock<HashMap<String, Instant>>,
}

impl EventBus {
    pub fn new(max_connections: usize, debounce_ms: u64) -> Self {
        Self {
            channels: RwLock::new(HashMap::new()),
            active_connections: Arc::new(AtomicUsize::new(0)),
            max_connections,
            debounce_ms,
            last_score_event: RwLock::new(HashMap::new()),
        }
    }

    /// Subscribe to events for a board. Returns a broadcast receiver and a guard
    /// that decrements the connection count on drop.
    pub fn subscribe(
        &self,
        board_slug: &str,
    ) -> Result<(broadcast::Receiver<SseEvent>, ConnectionGuard), CoreError> {
        let current = self.active_connections.load(Ordering::Relaxed);
        if current >= self.max_connections {
            return Err(CoreError::ServiceUnavailable(
                "SSE connection limit reached".to_string(),
            ));
        }

        let rx = {
            let mut channels = self.channels.write().unwrap();
            let tx = channels
                .entry(board_slug.to_string())
                .or_insert_with(|| {
                    let (tx, _) = broadcast::channel(256);
                    tx
                });
            tx.subscribe()
        };

        self.active_connections.fetch_add(1, Ordering::Relaxed);
        let guard = ConnectionGuard {
            counter: Arc::clone(&self.active_connections),
        };

        Ok((rx, guard))
    }

    /// Publish a version.created event (always sent, no debouncing).
    pub fn publish_version(&self, board_slug: &str, version_number: i32, note: Option<String>) {
        let channels = self.channels.read().unwrap();
        if let Some(tx) = channels.get(board_slug) {
            let _ = tx.send(SseEvent::VersionCreated {
                version_number,
                note,
            });
        }
    }

    /// Publish a score.updated event with per-board debouncing.
    /// Returns true if the event was sent, false if debounced.
    pub fn publish_score(
        &self,
        board_slug: &str,
        entry_slug: String,
        entry_name: String,
        score: f64,
    ) -> bool {
        // Check debounce
        if self.debounce_ms > 0 {
            let now = Instant::now();
            let mut last_events = self.last_score_event.write().unwrap();
            if let Some(last) = last_events.get(board_slug) {
                if now.duration_since(*last).as_millis() < self.debounce_ms as u128 {
                    return false;
                }
            }
            last_events.insert(board_slug.to_string(), now);
        }

        let channels = self.channels.read().unwrap();
        if let Some(tx) = channels.get(board_slug) {
            let _ = tx.send(SseEvent::ScoreUpdated {
                entry_slug,
                entry_name,
                score,
            });
        }
        true
    }

    pub fn active_connections(&self) -> usize {
        self.active_connections.load(Ordering::Relaxed)
    }
}

/// RAII guard that decrements the active connection count on drop.
pub struct ConnectionGuard {
    counter: Arc<AtomicUsize>,
}

impl Drop for ConnectionGuard {
    fn drop(&mut self) {
        self.counter.fetch_sub(1, Ordering::Relaxed);
    }
}

/// SSE stream handler: `GET /boards/:slug/stream`
pub async fn stream(
    State(state): State<Arc<AppState>>,
    Path(board_slug): Path<String>,
) -> ApiResult<Sse<impl futures_util::stream::Stream<Item = Result<Event, Infallible>>>> {
    // Verify board exists
    riley_leaderboards_core::repo::boards::get_by_slug(&state.pool, &board_slug).await?;

    let event_bus = state
        .event_bus
        .as_ref()
        .ok_or(CoreError::ServiceUnavailable(
            "SSE streaming is not enabled".to_string(),
        ))?;

    let (rx, guard) = event_bus.subscribe(&board_slug)?;

    // Build stream that holds the guard (keeping connection count accurate)
    let stream = BroadcastStream::new(rx);
    let mapped = stream.filter_map(|result| match result {
        Ok(event) => {
            let event_type = event.event_type().to_string();
            match serde_json::to_string(&event) {
                Ok(data) => Some(Ok::<_, Infallible>(Event::default().event(event_type).data(data))),
                Err(_) => None,
            }
        }
        Err(_) => None, // Lagged receiver — drop missed events
    });

    // Wrap to keep the guard alive for the stream's duration
    let guarded = GuardedStream {
        inner: mapped,
        _guard: guard,
    };

    Ok(Sse::new(guarded).keep_alive(KeepAlive::default()))
}

// Stream wrapper that holds a ConnectionGuard, decrementing the counter on drop.
pin_project_lite::pin_project! {
    struct GuardedStream<S> {
        #[pin]
        inner: S,
        _guard: ConnectionGuard,
    }
}

impl<S: futures_util::stream::Stream> futures_util::stream::Stream for GuardedStream<S> {
    type Item = S::Item;

    fn poll_next(
        self: std::pin::Pin<&mut Self>,
        cx: &mut std::task::Context<'_>,
    ) -> std::task::Poll<Option<Self::Item>> {
        self.project().inner.poll_next(cx)
    }
}
