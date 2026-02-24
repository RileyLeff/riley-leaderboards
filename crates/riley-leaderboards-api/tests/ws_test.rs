//! Integration tests for WebSocket live updates.
//!
//! Requires Postgres and Redis: docker compose up -d

use std::net::SocketAddr;
use std::sync::Arc;
use std::time::Duration;

use futures_util::StreamExt;
use riley_leaderboards_api::sse::EventBus;
use riley_leaderboards_api::{AppState, build_router};
use riley_leaderboards_core::config::{
    ConfigValue, DatabaseConfig, RedisConfig, RileyLeaderboardsConfig, ServerConfig,
};
use riley_leaderboards_core::db;
use tokio_tungstenite::tungstenite;

fn test_db_url() -> String {
    std::env::var("TEST_DATABASE_URL").unwrap_or_else(|_| {
        "postgresql://riley_leaderboards:riley_leaderboards_test@localhost:15433/riley_leaderboards_test".to_string()
    })
}

fn test_redis_url() -> String {
    std::env::var("TEST_REDIS_URL").unwrap_or_else(|_| "redis://localhost:16380".to_string())
}

/// Drop leftover schema from a previous failed run.
async fn pre_cleanup(schema: &str) {
    let db_url = test_db_url();
    let pool = sqlx::PgPool::connect(&db_url).await.expect("pre-cleanup connect");
    sqlx::query(&format!("DROP SCHEMA IF EXISTS \"{}\" CASCADE", schema))
        .execute(&pool)
        .await
        .ok();
    pool.close().await;
}

/// Starts a real HTTP server and returns the bound address along with state.
async fn serve_with_ws(
    schema: &str,
    max_connections: usize,
    debounce_ms: u64,
) -> (Arc<AppState>, SocketAddr) {
    pre_cleanup(schema).await;

    let config = RileyLeaderboardsConfig {
        server: Some(ServerConfig {
            sse_enabled: true,
            ws_enabled: true,
            ws_timeout_secs: 30,
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
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
        .await
        .expect("bind failed");
    let addr = listener.local_addr().expect("local_addr failed");
    tokio::spawn(async move {
        axum::serve(listener, router).await.ok();
    });

    (state, addr)
}

/// Starts a server with streaming disabled (no EventBus).
async fn serve_no_streaming(schema: &str) -> (Arc<AppState>, SocketAddr) {
    pre_cleanup(schema).await;

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
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
        .await
        .expect("bind failed");
    let addr = listener.local_addr().expect("local_addr failed");
    tokio::spawn(async move {
        axum::serve(listener, router).await.ok();
    });

    (state, addr)
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

async fn create_board_via_http(addr: SocketAddr, slug: &str, name: &str) {
    let client = reqwest::Client::new();
    let resp = client
        .post(format!("http://{addr}/boards"))
        .json(&serde_json::json!({
            "slug": slug,
            "name": name,
            "board_type": "scored",
            "sort_direction": "desc"
        }))
        .send()
        .await
        .expect("create board request failed");
    assert_eq!(resp.status(), 201, "create board failed");
}

async fn create_realtime_board_via_http(addr: SocketAddr, slug: &str, name: &str) {
    let client = reqwest::Client::new();
    let resp = client
        .post(format!("http://{addr}/boards"))
        .json(&serde_json::json!({
            "slug": slug,
            "name": name,
            "board_type": "scored",
            "accumulative": true,
            "realtime": true,
            "sort_direction": "desc"
        }))
        .send()
        .await
        .expect("create realtime board request failed");
    assert_eq!(resp.status(), 201, "create realtime board failed");
}

// ── Test: WS connects for an existing board ──────────────────────────────────

#[tokio::test]
async fn ws_connects_for_existing_board() {
    let schema = "test_ws_connect";
    let (state, addr) = serve_with_ws(schema, 100, 1000).await;
    flush_redis(&state).await;

    create_board_via_http(addr, "ws-board", "WS Board").await;

    let url = format!("ws://{addr}/boards/ws-board/ws");
    let (ws_stream, resp) =
        tokio_tungstenite::connect_async(&url).await.expect("WS connect failed");
    assert_eq!(resp.status(), 101);
    drop(ws_stream);

    cleanup(&state, schema).await;
}

// ── Test: WS returns error for nonexistent board ─────────────────────────────

#[tokio::test]
async fn ws_nonexistent_board_returns_error() {
    let schema = "test_ws_404";
    let (state, addr) = serve_with_ws(schema, 100, 1000).await;

    let url = format!("ws://{addr}/boards/no-such-board/ws");
    let result = tokio_tungstenite::connect_async(&url).await;

    // The server should reject the upgrade; tungstenite returns an HTTP error
    assert!(result.is_err(), "should fail for nonexistent board");

    cleanup(&state, schema).await;
}

// ── Test: WS returns error when streaming is disabled ────────────────────────

#[tokio::test]
async fn ws_disabled_returns_error() {
    let schema = "test_ws_disabled";
    let (state, addr) = serve_no_streaming(schema).await;
    flush_redis(&state).await;

    create_realtime_board_via_http(addr, "ws-disabled", "WS Disabled").await;

    let url = format!("ws://{addr}/boards/ws-disabled/ws");
    let result = tokio_tungstenite::connect_async(&url).await;
    assert!(result.is_err(), "should fail when streaming is disabled");

    cleanup(&state, schema).await;
}

// ── Test: WS receives version.created event ──────────────────────────────────

#[tokio::test]
async fn ws_receives_version_created_event() {
    let schema = "test_ws_version_event";
    let (state, addr) = serve_with_ws(schema, 100, 0).await;
    flush_redis(&state).await;

    create_board_via_http(addr, "ws-ver", "WS Version Board").await;

    // Create entry
    let client = reqwest::Client::new();
    let resp = client
        .post(format!("http://{addr}/boards/ws-ver/entries"))
        .json(&serde_json::json!({"slug": "e1", "name": "Entry 1"}))
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), 201);

    // Connect WebSocket
    let url = format!("ws://{addr}/boards/ws-ver/ws");
    let (ws_stream, _) = tokio_tungstenite::connect_async(&url).await.expect("WS connect failed");
    let (_, mut read) = ws_stream.split();

    // Create a version (triggers version.created event)
    let resp = client
        .post(format!("http://{addr}/boards/ws-ver/versions"))
        .json(&serde_json::json!({
            "placements": [{"entry_slug": "e1", "score": 100.0}],
            "note": "WS test version"
        }))
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), 201);

    // Should receive version.created as a JSON text frame
    let msg = tokio::time::timeout(Duration::from_secs(5), read.next())
        .await
        .expect("timed out waiting for WS message")
        .expect("stream ended")
        .expect("WS read error");

    let text = match msg {
        tungstenite::Message::Text(t) => t.to_string(),
        other => panic!("expected text message, got {other:?}"),
    };
    let json: serde_json::Value = serde_json::from_str(&text).expect("invalid JSON");
    assert_eq!(json["type"], "version.created");
    assert_eq!(json["version_number"], 1);
    assert_eq!(json["note"], "WS test version");

    cleanup(&state, schema).await;
}

// ── Test: WS receives score.updated event ────────────────────────────────────

#[tokio::test]
async fn ws_receives_score_updated_event() {
    let schema = "test_ws_score_event";
    let (state, addr) = serve_with_ws(schema, 100, 0).await;
    flush_redis(&state).await;

    create_realtime_board_via_http(addr, "ws-scores", "WS Scores Board").await;

    // Connect WebSocket
    let url = format!("ws://{addr}/boards/ws-scores/ws");
    let (ws_stream, _) = tokio_tungstenite::connect_async(&url).await.expect("WS connect failed");
    let (_, mut read) = ws_stream.split();

    // Submit a score
    let client = reqwest::Client::new();
    let resp = client
        .post(format!("http://{addr}/boards/ws-scores/scores"))
        .json(&serde_json::json!({
            "entry_slug": "player-1",
            "entry_name": "Player One",
            "score": 1500.0
        }))
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), 200);

    // Should receive score.updated as a JSON text frame
    let msg = tokio::time::timeout(Duration::from_secs(5), read.next())
        .await
        .expect("timed out waiting for WS message")
        .expect("stream ended")
        .expect("WS read error");

    let text = match msg {
        tungstenite::Message::Text(t) => t.to_string(),
        other => panic!("expected text message, got {other:?}"),
    };
    let json: serde_json::Value = serde_json::from_str(&text).expect("invalid JSON");
    assert_eq!(json["type"], "score.updated");
    assert_eq!(json["entry_slug"], "player-1");
    assert_eq!(json["entry_name"], "Player One");
    assert_eq!(json["score"], 1500.0);

    cleanup(&state, schema).await;
}

// ── Test: SSE and WS share the EventBus connection count ─────────────────────

#[tokio::test]
async fn ws_and_sse_share_connection_count() {
    let schema = "test_ws_sse_shared";
    let (state, addr) = serve_with_ws(schema, 3, 1000).await;
    flush_redis(&state).await;

    create_board_via_http(addr, "shared-board", "Shared Board").await;

    let event_bus = state.event_bus.as_ref().unwrap();
    assert_eq!(event_bus.active_connections(), 0);

    // Open an SSE connection via EventBus subscribe (simulates SSE)
    let (_sse_rx, _sse_guard) = event_bus.subscribe("shared-board").unwrap();
    assert_eq!(event_bus.active_connections(), 1);

    // Open a WS connection (goes through the real server which calls subscribe)
    let url = format!("ws://{addr}/boards/shared-board/ws");
    let (ws1, _) = tokio_tungstenite::connect_async(&url).await.expect("WS1 connect failed");

    // Give the server a moment to process the upgrade
    tokio::time::sleep(Duration::from_millis(100)).await;
    assert_eq!(event_bus.active_connections(), 2);

    // Open a second WS connection
    let (ws2, _) = tokio_tungstenite::connect_async(&url).await.expect("WS2 connect failed");
    tokio::time::sleep(Duration::from_millis(100)).await;
    assert_eq!(event_bus.active_connections(), 3);

    // Third WS should be rejected (limit is 3, all slots taken)
    let result = tokio_tungstenite::connect_async(&url).await;
    assert!(result.is_err(), "should fail at connection limit");

    // Drop a WS connection, should free a slot
    drop(ws1);
    tokio::time::sleep(Duration::from_millis(500)).await;
    assert_eq!(event_bus.active_connections(), 2);

    // Now a new connection should succeed
    let (_ws3, _) = tokio_tungstenite::connect_async(&url).await.expect("WS3 connect failed");
    tokio::time::sleep(Duration::from_millis(100)).await;
    assert_eq!(event_bus.active_connections(), 3);

    drop(_ws3);
    drop(ws2);
    drop(_sse_guard);

    cleanup(&state, schema).await;
}
