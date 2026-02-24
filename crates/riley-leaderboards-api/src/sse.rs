//! SSE (Server-Sent Events) infrastructure for live board updates.
//!
//! Provides a per-board broadcast channel registry and debouncing for
//! high-throughput score.updated events.

use std::collections::HashMap;
use std::convert::Infallible;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, RwLock};
use std::time::{Duration, Instant};

use axum::extract::{Path, State};
use axum::response::sse::{Event, KeepAlive, Sse};
use serde::Serialize;
use tokio::sync::broadcast;
use tokio_stream::StreamExt;
use tokio_stream::wrappers::BroadcastStream;

use crate::AppState;
use crate::error::ApiResult;
use riley_leaderboards_core::error::Error as CoreError;

/// Events published to streaming subscribers (SSE and WebSocket).
#[derive(Debug, Clone, Serialize)]
#[serde(tag = "type")]
pub enum BoardEvent {
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

impl BoardEvent {
    fn event_type(&self) -> &'static str {
        match self {
            Self::VersionCreated { .. } => "version.created",
            Self::ScoreUpdated { .. } => "score.updated",
        }
    }
}

/// Per-board broadcast channel registry with connection tracking and debouncing.
pub struct EventBus {
    channels: RwLock<HashMap<String, broadcast::Sender<BoardEvent>>>,
    active_connections: Arc<AtomicUsize>,
    max_connections: usize,
    broadcast_buffer: usize,
    debounce_ms: u64,
    last_score_event: RwLock<HashMap<String, Instant>>,
}

impl EventBus {
    pub fn new(max_connections: usize, debounce_ms: u64, broadcast_buffer: usize) -> Self {
        Self {
            channels: RwLock::new(HashMap::new()),
            active_connections: Arc::new(AtomicUsize::new(0)),
            max_connections,
            broadcast_buffer,
            debounce_ms,
            last_score_event: RwLock::new(HashMap::new()),
        }
    }

    /// Subscribe to events for a board. Returns a broadcast receiver and a guard
    /// that decrements the connection count on drop.
    pub fn subscribe(
        &self,
        board_slug: &str,
    ) -> Result<(broadcast::Receiver<BoardEvent>, ConnectionGuard), CoreError> {
        // Atomically reserve a connection slot (fetch_add first, undo if over limit)
        let prev = self.active_connections.fetch_add(1, Ordering::AcqRel);
        if prev >= self.max_connections {
            self.active_connections.fetch_sub(1, Ordering::AcqRel);
            return Err(CoreError::ServiceUnavailable(
                "streaming connection limit reached".to_string(),
            ));
        }

        let buffer = self.broadcast_buffer;
        let rx = {
            let mut channels = self.channels.write().unwrap();
            let tx = channels.entry(board_slug.to_string()).or_insert_with(|| {
                let (tx, _) = broadcast::channel(buffer);
                tx
            });
            tx.subscribe()
        };

        let guard = ConnectionGuard {
            counter: Arc::clone(&self.active_connections),
        };

        metrics::gauge!("streaming_active_connections").set((prev + 1) as f64);

        Ok((rx, guard))
    }

    /// Publish a version.created event (always sent, no debouncing).
    pub fn publish_version(&self, board_slug: &str, version_number: i32, note: Option<String>) {
        let should_prune = {
            let channels = self.channels.read().unwrap();
            if let Some(tx) = channels.get(board_slug) {
                let _ = tx.send(BoardEvent::VersionCreated {
                    version_number,
                    note,
                });
                tx.receiver_count() == 0
            } else {
                false
            }
        };
        if should_prune {
            self.prune_channel(board_slug);
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
        let should_prune = {
            let channels = self.channels.read().unwrap();
            let Some(tx) = channels.get(board_slug) else {
                return true; // No subscribers — nothing to debounce or send
            };

            // Check debounce (only when there are subscribers)
            if self.debounce_ms > 0 {
                let now = Instant::now();
                let mut last_events = self.last_score_event.write().unwrap();
                if let Some(last) = last_events.get(board_slug)
                    && now.duration_since(*last).as_millis() < self.debounce_ms as u128
                {
                    return false;
                }
                last_events.insert(board_slug.to_string(), now);
            }

            let _ = tx.send(BoardEvent::ScoreUpdated {
                entry_slug,
                entry_name,
                score,
            });
            tx.receiver_count() == 0
        };
        if should_prune {
            self.prune_channel(board_slug);
        }
        true
    }

    /// Remove a channel with no receivers (and its debounce state).
    fn prune_channel(&self, board_slug: &str) {
        let mut channels = self.channels.write().unwrap();
        // Re-check under write lock — a new subscriber may have appeared
        if let Some(tx) = channels.get(board_slug)
            && tx.receiver_count() == 0
        {
            channels.remove(board_slug);
            self.last_score_event.write().unwrap().remove(board_slug);
        }
    }

    pub fn active_connections(&self) -> usize {
        self.active_connections.load(Ordering::Acquire)
    }

    /// Number of board channels currently in the registry.
    pub fn channel_count(&self) -> usize {
        self.channels.read().unwrap().len()
    }
}

/// RAII guard that decrements the active connection count on drop.
pub struct ConnectionGuard {
    counter: Arc<AtomicUsize>,
}

impl Drop for ConnectionGuard {
    fn drop(&mut self) {
        let prev = self.counter.fetch_sub(1, Ordering::AcqRel);
        metrics::gauge!("streaming_active_connections").set((prev - 1) as f64);
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

    let timeout_secs = state
        .config
        .server
        .as_ref()
        .map(|s| s.sse_timeout_secs)
        .unwrap_or(1800);

    // Build stream that holds the guard (keeping connection count accurate)
    let stream = BroadcastStream::new(rx);
    let mapped = stream.filter_map(|result| match result {
        Ok(event) => {
            let event_type = event.event_type().to_string();
            match serde_json::to_string(&event) {
                Ok(data) => Some(Ok::<_, Infallible>(
                    Event::default().event(event_type).data(data),
                )),
                Err(_) => None,
            }
        }
        Err(_) => None, // Lagged receiver — drop missed events
    });

    // Wrap to keep the guard alive for the stream's duration, with optional timeout
    let deadline = if timeout_secs > 0 {
        Duration::from_secs(timeout_secs)
    } else {
        // Effectively no timeout (~136 years)
        Duration::from_secs(u32::MAX as u64)
    };

    let guarded = GuardedStream {
        inner: mapped,
        _guard: guard,
        deadline: tokio::time::sleep(deadline),
        has_deadline: timeout_secs > 0,
    };

    Ok(Sse::new(guarded).keep_alive(KeepAlive::default()))
}

// Stream wrapper that holds a ConnectionGuard and optional deadline.
pin_project_lite::pin_project! {
    struct GuardedStream<S> {
        #[pin]
        inner: S,
        _guard: ConnectionGuard,
        #[pin]
        deadline: tokio::time::Sleep,
        has_deadline: bool,
    }
}

impl<S: futures_util::stream::Stream> futures_util::stream::Stream for GuardedStream<S> {
    type Item = S::Item;

    fn poll_next(
        self: std::pin::Pin<&mut Self>,
        cx: &mut std::task::Context<'_>,
    ) -> std::task::Poll<Option<Self::Item>> {
        let this = self.project();
        if *this.has_deadline && this.deadline.poll(cx).is_ready() {
            return std::task::Poll::Ready(None);
        }
        this.inner.poll_next(cx)
    }
}
