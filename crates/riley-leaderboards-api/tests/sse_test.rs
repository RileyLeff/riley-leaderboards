//! Integration tests for SSE live updates.
//!
//! Requires Postgres and Redis: docker compose up -d

use std::sync::Arc;
use std::time::Duration;

use axum::body::Body;
use http::Request;
use riley_leaderboards_api::sse::EventBus;
use riley_leaderboards_api::{AppState, build_router};
use riley_leaderboards_core::config::{
    ConfigValue, DatabaseConfig, RedisConfig, RileyLeaderboardsConfig, ServerConfig,
};
use riley_leaderboards_core::db;
use tower::ServiceExt;

fn test_db_url() -> String {
    std::env::var("TEST_DATABASE_URL").unwrap_or_else(|_| {
        "postgresql://riley_leaderboards:riley_leaderboards_test@localhost:15433/riley_leaderboards_test".to_string()
    })
}

fn test_redis_url() -> String {
    std::env::var("TEST_REDIS_URL")
        .unwrap_or_else(|_| "redis://localhost:16380".to_string())
}

async fn setup_with_sse(schema: &str, max_connections: usize, debounce_ms: u64) -> (Arc<AppState>, axum::Router) {
    let config = RileyLeaderboardsConfig {
        server: Some(ServerConfig {
            sse_enabled: true,
            sse_max_connections: max_connections,
            sse_score_debounce_ms: debounce_ms,
            ..Default::default()
        }),
        database: DatabaseConfig {
            url: ConfigValue::new(test_db_url()),
            max_connections: 2,
            schema: Some(schema.to_string()),
        },
        redis: Some(RedisConfig {
            url: ConfigValue::new(test_redis_url()),
            key_prefix: "rl".to_string(),
        }),
        auth: None,
        sync: None,
        limits: None,
        webhooks: vec![],
    };

    let pool = db::connect(&config.database).await.expect("connect failed");
    db::migrate(&pool).await.expect("migrate failed");

    let url = test_redis_url();
    let client = redis::Client::open(url.as_str()).expect("redis client open failed");
    let redis_conn = redis::aio::ConnectionManager::new(client)
        .await
        .expect("redis connect failed");

    let event_bus = EventBus::new(max_connections, debounce_ms, 256);

    let state = Arc::new(AppState {
        pool,
        redis: Some(redis_conn),
        config,
        auth_mode: riley_leaderboards_api::auth::AuthMode::NoAuth,
        sync_mutex: tokio::sync::Mutex::new(()),
        event_bus: Some(event_bus),
        task_tracker: tokio_util::task::TaskTracker::new(),
    });
    let router = build_router(state.clone());
    (state, router)
}

async fn setup_no_sse(schema: &str) -> (Arc<AppState>, axum::Router) {
    let config = RileyLeaderboardsConfig {
        server: None,
        database: DatabaseConfig {
            url: ConfigValue::new(test_db_url()),
            max_connections: 2,
            schema: Some(schema.to_string()),
        },
        redis: Some(RedisConfig {
            url: ConfigValue::new(test_redis_url()),
            key_prefix: "rl".to_string(),
        }),
        auth: None,
        sync: None,
        limits: None,
        webhooks: vec![],
    };

    let pool = db::connect(&config.database).await.expect("connect failed");
    db::migrate(&pool).await.expect("migrate failed");

    let url = test_redis_url();
    let client = redis::Client::open(url.as_str()).expect("redis client open failed");
    let redis_conn = redis::aio::ConnectionManager::new(client)
        .await
        .expect("redis connect failed");

    let state = Arc::new(AppState {
        pool,
        redis: Some(redis_conn),
        config,
        auth_mode: riley_leaderboards_api::auth::AuthMode::NoAuth,
        sync_mutex: tokio::sync::Mutex::new(()),
        event_bus: None,
        task_tracker: tokio_util::task::TaskTracker::new(),
    });
    let router = build_router(state.clone());
    (state, router)
}

async fn cleanup(state: &AppState, schema: &str) {
    sqlx::query(&format!("DROP SCHEMA IF EXISTS \"{}\" CASCADE", schema))
        .execute(&state.pool)
        .await
        .expect("test cleanup: failed to drop schema");
    state.pool.close().await;
}

async fn flush_redis(state: &AppState) {
    if let Some(ref redis) = state.redis {
        let mut conn = redis.clone();
        let _: () = redis::cmd("FLUSHDB")
            .query_async(&mut conn)
            .await
            .expect("flush redis");
    }
}

fn json_request(method: &str, path: &str, body: Option<serde_json::Value>) -> Request<Body> {
    let builder = Request::builder()
        .method(method)
        .uri(path)
        .header("content-type", "application/json");

    match body {
        Some(json) => builder
            .body(Body::from(serde_json::to_string(&json).unwrap()))
            .unwrap(),
        None => builder.body(Body::empty()).unwrap(),
    }
}

async fn create_board(app: &axum::Router, slug: &str, name: &str) {
    let resp = app
        .clone()
        .oneshot(json_request(
            "POST",
            "/boards",
            Some(serde_json::json!({
                "slug": slug,
                "name": name,
                "board_type": "scored",
                "sort_direction": "desc"
            })),
        ))
        .await
        .unwrap();
    assert_eq!(resp.status(), 201, "create board failed");
}

async fn create_realtime_board(app: &axum::Router, slug: &str, name: &str) {
    let resp = app
        .clone()
        .oneshot(json_request(
            "POST",
            "/boards",
            Some(serde_json::json!({
                "slug": slug,
                "name": name,
                "board_type": "scored",
                "accumulative": true,
                "realtime": true,
                "sort_direction": "desc"
            })),
        ))
        .await
        .unwrap();
    assert_eq!(resp.status(), 201, "create realtime board failed");
}

// ── Test: SSE stream connects and returns 200 ────────────────────────────────

#[tokio::test]
async fn sse_stream_connects_for_existing_board() {
    let schema = "test_sse_connect";
    let (state, app) = setup_with_sse(schema, 100, 1000).await;
    flush_redis(&state).await;

    create_board(&app, "sse-board", "SSE Board").await;

    let resp = app
        .clone()
        .oneshot(
            Request::get("/boards/sse-board/stream")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(resp.status(), 200);

    cleanup(&state, schema).await;
}

// ── Test: SSE stream returns 404 for nonexistent board ───────────────────────

#[tokio::test]
async fn sse_stream_nonexistent_board_returns_404() {
    let schema = "test_sse_404";
    let (state, app) = setup_with_sse(schema, 100, 1000).await;

    let resp = app
        .clone()
        .oneshot(
            Request::get("/boards/no-such-board/stream")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(resp.status(), 404);

    cleanup(&state, schema).await;
}

// ── Test: SSE stream returns 503 when SSE is disabled ────────────────────────

#[tokio::test]
async fn sse_stream_disabled_returns_503() {
    let schema = "test_sse_disabled";
    let (state, app) = setup_no_sse(schema).await;
    flush_redis(&state).await;

    create_realtime_board(&app, "disabled-sse", "Disabled SSE").await;

    let resp = app
        .clone()
        .oneshot(
            Request::get("/boards/disabled-sse/stream")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(resp.status(), 503);

    cleanup(&state, schema).await;
}

// ── Test: EventBus publishes version.created events ──────────────────────────

#[tokio::test]
async fn event_bus_version_created() {
    let bus = EventBus::new(100, 0, 256);

    // Subscribe first
    let (mut rx, _guard) = bus.subscribe("test-board").unwrap();

    // Publish version event
    bus.publish_version("test-board", 1, Some("First version".to_string()));

    let event = rx.recv().await.unwrap();
    let json = serde_json::to_value(&event).unwrap();
    assert_eq!(json["type"], "version.created");
    assert_eq!(json["version_number"], 1);
    assert_eq!(json["note"], "First version");
}

// ── Test: EventBus publishes score.updated events ────────────────────────────

#[tokio::test]
async fn event_bus_score_updated() {
    let bus = EventBus::new(100, 0, 256); // No debounce

    let (mut rx, _guard) = bus.subscribe("test-board").unwrap();

    let sent = bus.publish_score(
        "test-board",
        "player-1".to_string(),
        "Player One".to_string(),
        42.5,
    );
    assert!(sent);

    let event = rx.recv().await.unwrap();
    let json = serde_json::to_value(&event).unwrap();
    assert_eq!(json["type"], "score.updated");
    assert_eq!(json["entry_slug"], "player-1");
    assert_eq!(json["entry_name"], "Player One");
    assert_eq!(json["score"], 42.5);
}

// ── Test: EventBus debounces score events ────────────────────────────────────

#[tokio::test]
async fn event_bus_debounces_score_events() {
    let bus = EventBus::new(100, 500, 256); // 500ms debounce

    let (mut rx, _guard) = bus.subscribe("debounce-board").unwrap();

    // First event should go through
    let sent1 = bus.publish_score(
        "debounce-board",
        "p1".to_string(),
        "P1".to_string(),
        100.0,
    );
    assert!(sent1);

    // Second event within window should be dropped
    let sent2 = bus.publish_score(
        "debounce-board",
        "p1".to_string(),
        "P1".to_string(),
        200.0,
    );
    assert!(!sent2);

    // First event should be in the channel
    let event = rx.recv().await.unwrap();
    let json = serde_json::to_value(&event).unwrap();
    assert_eq!(json["score"], 100.0);

    // After debounce window, should go through
    tokio::time::sleep(Duration::from_millis(600)).await;
    let sent3 = bus.publish_score(
        "debounce-board",
        "p1".to_string(),
        "P1".to_string(),
        300.0,
    );
    assert!(sent3);

    let event = rx.recv().await.unwrap();
    let json = serde_json::to_value(&event).unwrap();
    assert_eq!(json["score"], 300.0);
}

// ── Test: EventBus connection limit ──────────────────────────────────────────

#[tokio::test]
async fn event_bus_connection_limit() {
    let bus = EventBus::new(2, 0, 256); // Max 2 connections

    let (_rx1, _guard1) = bus.subscribe("board").unwrap();
    let (_rx2, _guard2) = bus.subscribe("board").unwrap();
    assert_eq!(bus.active_connections(), 2);

    // Third should fail
    let result = bus.subscribe("board");
    assert!(result.is_err());

    // Drop a guard, should allow a new subscription
    drop(_guard1);
    assert_eq!(bus.active_connections(), 1);
    let result = bus.subscribe("board");
    assert!(result.is_ok());
}

// ── Test: Events only go to the subscribed board ─────────────────────────────

#[tokio::test]
async fn event_bus_board_isolation() {
    let bus = EventBus::new(100, 0, 256);

    let (mut rx_a, _guard_a) = bus.subscribe("board-a").unwrap();
    let (mut rx_b, _guard_b) = bus.subscribe("board-b").unwrap();

    bus.publish_version("board-a", 1, None);

    // board-a should receive the event
    let event = tokio::time::timeout(Duration::from_millis(100), rx_a.recv())
        .await
        .expect("should receive event");
    assert!(event.is_ok());

    // board-b should not receive it
    let result = tokio::time::timeout(Duration::from_millis(100), rx_b.recv()).await;
    assert!(result.is_err(), "board-b should not receive board-a events");
}

// ── Test: Version creation publishes SSE event via API ───────────────────────

#[tokio::test]
async fn version_create_publishes_sse_event() {
    let schema = "test_sse_version_event";
    let (state, app) = setup_with_sse(schema, 100, 0).await;
    flush_redis(&state).await;

    create_board(&app, "sse-ver", "SSE Version Board").await;

    // Create entries
    app.clone()
        .oneshot(json_request(
            "POST",
            "/boards/sse-ver/entries",
            Some(serde_json::json!({"slug": "e1", "name": "Entry 1"})),
        ))
        .await
        .unwrap();

    // Subscribe to SSE events
    let event_bus = state.event_bus.as_ref().unwrap();
    let (mut rx, _guard) = event_bus.subscribe("sse-ver").unwrap();

    // Create a version
    let resp = app
        .clone()
        .oneshot(json_request(
            "POST",
            "/boards/sse-ver/versions",
            Some(serde_json::json!({
                "placements": [{"entry_slug": "e1", "score": 100.0}],
                "note": "Test version"
            })),
        ))
        .await
        .unwrap();
    assert_eq!(resp.status(), 201);

    // Should receive version.created event
    let event = tokio::time::timeout(Duration::from_millis(500), rx.recv())
        .await
        .expect("should receive event")
        .unwrap();
    let json = serde_json::to_value(&event).unwrap();
    assert_eq!(json["type"], "version.created");
    assert_eq!(json["version_number"], 1);
    assert_eq!(json["note"], "Test version");

    cleanup(&state, schema).await;
}

// ── Test: Score submission publishes SSE event via API ────────────────────────

#[tokio::test]
async fn score_submit_publishes_sse_event() {
    let schema = "test_sse_score_event";
    let (state, app) = setup_with_sse(schema, 100, 0).await;
    flush_redis(&state).await;

    create_realtime_board(&app, "sse-scores", "SSE Scores Board").await;

    // Subscribe to SSE events
    let event_bus = state.event_bus.as_ref().unwrap();
    let (mut rx, _guard) = event_bus.subscribe("sse-scores").unwrap();

    // Submit a score
    let resp = app
        .clone()
        .oneshot(json_request(
            "POST",
            "/boards/sse-scores/scores",
            Some(serde_json::json!({
                "entry_slug": "player-1",
                "entry_name": "Player One",
                "score": 1500.0
            })),
        ))
        .await
        .unwrap();
    assert_eq!(resp.status(), 200);

    // Should receive score.updated event
    let event = tokio::time::timeout(Duration::from_millis(500), rx.recv())
        .await
        .expect("should receive event")
        .unwrap();
    let json = serde_json::to_value(&event).unwrap();
    assert_eq!(json["type"], "score.updated");
    assert_eq!(json["entry_slug"], "player-1");
    assert_eq!(json["entry_name"], "Player One");
    assert_eq!(json["score"], 1500.0);

    cleanup(&state, schema).await;
}

// ── Test: Snapshot publishes SSE version.created event ───────────────────────

#[tokio::test]
async fn snapshot_publishes_sse_event() {
    let schema = "test_sse_snapshot";
    let (state, app) = setup_with_sse(schema, 100, 0).await;
    flush_redis(&state).await;

    create_realtime_board(&app, "sse-snap", "SSE Snapshot Board").await;

    // Submit a score first
    app.clone()
        .oneshot(json_request(
            "POST",
            "/boards/sse-snap/scores",
            Some(serde_json::json!({
                "entry_slug": "p1",
                "entry_name": "P1",
                "score": 100.0
            })),
        ))
        .await
        .unwrap();

    // Subscribe AFTER score submit (don't want that event)
    let event_bus = state.event_bus.as_ref().unwrap();
    let (mut rx, _guard) = event_bus.subscribe("sse-snap").unwrap();

    // Snapshot
    let resp = app
        .clone()
        .oneshot(json_request(
            "POST",
            "/boards/sse-snap/snapshot",
            Some(serde_json::json!({"note": "Snap!"})),
        ))
        .await
        .unwrap();
    assert_eq!(resp.status(), 201);

    // Should receive version.created
    let event = tokio::time::timeout(Duration::from_millis(500), rx.recv())
        .await
        .expect("should receive event")
        .unwrap();
    let json = serde_json::to_value(&event).unwrap();
    assert_eq!(json["type"], "version.created");
    assert_eq!(json["version_number"], 1);
    assert_eq!(json["note"], "Snap!");

    cleanup(&state, schema).await;
}

// ── Test: EventBus prunes channels with no receivers ─────────────────────────

#[tokio::test]
async fn event_bus_prunes_stale_channels() {
    let bus = EventBus::new(100, 100, 256); // 100ms debounce

    // Subscribe and then drop the receiver
    let (rx, guard) = bus.subscribe("prune-board").unwrap();
    assert_eq!(bus.channel_count(), 1);

    // Publish a score to create a debounce entry
    bus.publish_score(
        "prune-board",
        "p1".to_string(),
        "P1".to_string(),
        50.0,
    );

    // Drop the subscriber — receiver count goes to 0 on next publish
    drop(rx);
    drop(guard);

    // Next publish should trigger pruning (no receivers left)
    bus.publish_version("prune-board", 1, None);

    // Channel and debounce state should be pruned
    assert_eq!(bus.channel_count(), 0);
}
