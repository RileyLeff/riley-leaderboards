//! Integration tests for realtime board operations (Redis-backed).
//!
//! Requires both Postgres and Redis: docker compose up -d

use std::sync::Arc;

use axum::body::Body;
use http::Request;
use http_body_util::BodyExt;
use riley_leaderboards_api::{AppState, build_router};
use riley_leaderboards_core::config::{ConfigValue, DatabaseConfig, RileyLeaderboardsConfig};
use riley_leaderboards_core::db;
use sqlx::postgres::PgPoolOptions;
use tower::ServiceExt;

fn test_db_url() -> String {
    std::env::var("TEST_DATABASE_URL").unwrap_or_else(|_| {
        "postgresql://riley_leaderboards:riley_leaderboards_test@localhost:15433/riley_leaderboards_test".to_string()
    })
}

fn test_redis_url() -> String {
    std::env::var("TEST_REDIS_URL").unwrap_or_else(|_| "redis://localhost:16380".to_string())
}

/// Defensive cleanup: drop a test schema if it exists from a previous panicked run.
async fn drop_schema_if_exists(schema: &str) {
    let pool = PgPoolOptions::new()
        .max_connections(1)
        .connect(&test_db_url())
        .await
        .expect("bootstrap connect for cleanup failed");
    let _ = sqlx::query(&format!("DROP SCHEMA IF EXISTS \"{}\" CASCADE", schema))
        .execute(&pool)
        .await;
    pool.close().await;
}

async fn setup_with_redis(schema: &str) -> (Arc<AppState>, axum::Router) {
    drop_schema_if_exists(schema).await;
    let config = RileyLeaderboardsConfig {
        server: None,
        database: DatabaseConfig {
            url: ConfigValue::new(test_db_url()),
            max_connections: 2,
            schema: Some(schema.to_string()),
        },
        redis: None,
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

async fn setup_no_redis(schema: &str) -> (Arc<AppState>, axum::Router) {
    drop_schema_if_exists(schema).await;
    let config = RileyLeaderboardsConfig {
        server: None,
        database: DatabaseConfig {
            url: ConfigValue::new(test_db_url()),
            max_connections: 2,
            schema: Some(schema.to_string()),
        },
        redis: None,
        auth: None,
        sync: None,
        limits: None,
        webhooks: vec![],
    };

    let pool = db::connect(&config.database).await.expect("connect failed");
    db::migrate(&pool).await.expect("migrate failed");

    let state = Arc::new(AppState {
        pool,
        redis: None,
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

/// Flush the Redis test DB to avoid cross-test contamination.
async fn flush_redis(state: &AppState) {
    if let Some(ref redis) = state.redis {
        let mut conn = redis.clone();
        let _: () = redis::cmd("FLUSHDB")
            .query_async(&mut conn)
            .await
            .expect("flush redis");
    }
}

async fn json_body(response: http::Response<Body>) -> serde_json::Value {
    let body = response.into_body().collect().await.unwrap().to_bytes();
    serde_json::from_slice(&body).unwrap()
}

fn json_request(method: &str, uri: &str, body: Option<serde_json::Value>) -> Request<Body> {
    let builder = Request::builder()
        .method(method)
        .uri(uri)
        .header("content-type", "application/json");

    if let Some(b) = body {
        builder
            .body(Body::from(serde_json::to_vec(&b).unwrap()))
            .unwrap()
    } else {
        builder.body(Body::empty()).unwrap()
    }
}

/// Helper to create a realtime board via the API.
async fn create_realtime_board(app: &axum::Router, slug: &str, name: &str) -> serde_json::Value {
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
    assert_eq!(resp.status(), 201, "create board failed");
    json_body(resp).await
}

// ── Test: Create a realtime board ────────────────────────────────────────────

#[tokio::test]
async fn create_realtime_board_succeeds() {
    let schema = "test_rt_create";
    let (state, app) = setup_with_redis(schema).await;
    flush_redis(&state).await;

    let board = create_realtime_board(&app, "live-scores", "Live Scores").await;
    assert_eq!(board["realtime"], true);
    assert_eq!(board["clear_on_snapshot"], false);
    assert_eq!(board["accumulative"], true);
    assert_eq!(board["board_type"], "scored");

    cleanup(&state, schema).await;
}

// ── Test: Realtime requires accumulative + scored ────────────────────────────

#[tokio::test]
async fn create_realtime_non_accumulative_fails() {
    let schema = "test_rt_validate";
    let (state, app) = setup_with_redis(schema).await;
    flush_redis(&state).await;

    let resp = app
        .clone()
        .oneshot(json_request(
            "POST",
            "/boards",
            Some(serde_json::json!({
                "slug": "bad-rt",
                "name": "Bad Realtime",
                "board_type": "scored",
                "accumulative": false,
                "realtime": true
            })),
        ))
        .await
        .unwrap();
    assert_eq!(resp.status(), 400);

    cleanup(&state, schema).await;
}

// ── Test: clear_on_snapshot requires realtime ────────────────────────────────

#[tokio::test]
async fn clear_on_snapshot_requires_realtime() {
    let schema = "test_rt_clear_validate";
    let (state, app) = setup_with_redis(schema).await;
    flush_redis(&state).await;

    let resp = app
        .clone()
        .oneshot(json_request(
            "POST",
            "/boards",
            Some(serde_json::json!({
                "slug": "bad-clear",
                "name": "Bad Clear",
                "board_type": "scored",
                "accumulative": true,
                "realtime": false,
                "clear_on_snapshot": true
            })),
        ))
        .await
        .unwrap();
    assert_eq!(resp.status(), 400);

    cleanup(&state, schema).await;
}

// ── Test: Submit score to realtime board ──────────────────────────────────────

#[tokio::test]
async fn submit_score_and_read_latest() {
    let schema = "test_rt_submit_latest";
    let (state, app) = setup_with_redis(schema).await;
    flush_redis(&state).await;

    create_realtime_board(&app, "game-scores", "Game Scores").await;

    // Submit scores
    for (slug, name, score) in [
        ("alice", "Alice", 100.0),
        ("bob", "Bob", 200.0),
        ("carol", "Carol", 150.0),
    ] {
        let resp = app
            .clone()
            .oneshot(json_request(
                "POST",
                "/boards/game-scores/scores",
                Some(serde_json::json!({
                    "entry_slug": slug,
                    "entry_name": name,
                    "score": score
                })),
            ))
            .await
            .unwrap();
        assert_eq!(resp.status(), 200, "submit score for {slug} failed");
    }

    // Read latest — should return live standings from Redis
    let resp = app
        .clone()
        .oneshot(json_request("GET", "/boards/game-scores/latest", None))
        .await
        .unwrap();
    assert_eq!(resp.status(), 200);
    let body = json_body(resp).await;

    // version_number = 0 indicates live state
    assert_eq!(body["version_number"], 0);

    let placements = body["placements"].as_array().unwrap();
    assert_eq!(placements.len(), 3);

    // Sorted desc: Bob (200), Carol (150), Alice (100)
    assert_eq!(placements[0]["entry_slug"], "bob");
    assert_eq!(placements[0]["score"], 200.0);
    assert_eq!(placements[0]["position"], 1);

    assert_eq!(placements[1]["entry_slug"], "carol");
    assert_eq!(placements[1]["score"], 150.0);
    assert_eq!(placements[1]["position"], 2);

    assert_eq!(placements[2]["entry_slug"], "alice");
    assert_eq!(placements[2]["score"], 100.0);
    assert_eq!(placements[2]["position"], 3);

    cleanup(&state, schema).await;
}

// ── Test: Snapshot from Redis to Postgres ─────────────────────────────────────

#[tokio::test]
async fn snapshot_creates_postgres_version() {
    let schema = "test_rt_snapshot";
    let (state, app) = setup_with_redis(schema).await;
    flush_redis(&state).await;

    create_realtime_board(&app, "snap-board", "Snapshot Board").await;

    // Submit scores
    for (slug, name, score) in [("p1", "Player 1", 500.0), ("p2", "Player 2", 300.0)] {
        let resp = app
            .clone()
            .oneshot(json_request(
                "POST",
                "/boards/snap-board/scores",
                Some(serde_json::json!({
                    "entry_slug": slug,
                    "entry_name": name,
                    "score": score
                })),
            ))
            .await
            .unwrap();
        assert_eq!(resp.status(), 200);
    }

    // Snapshot
    let resp = app
        .clone()
        .oneshot(json_request(
            "POST",
            "/boards/snap-board/snapshot",
            Some(serde_json::json!({
                "note": "End of round 1"
            })),
        ))
        .await
        .unwrap();
    assert_eq!(resp.status(), 201);
    let body = json_body(resp).await;
    assert_eq!(body["version_number"], 1);
    assert_eq!(body["note"], "End of round 1");

    let placements = body["placements"].as_array().unwrap();
    assert_eq!(placements.len(), 2);
    assert_eq!(placements[0]["entry_slug"], "p1");
    assert_eq!(placements[0]["score"], 500.0);
    assert_eq!(placements[0]["position"], 1);

    // Verify version persisted in Postgres by fetching via versions endpoint
    let resp = app
        .clone()
        .oneshot(json_request("GET", "/boards/snap-board/versions/1", None))
        .await
        .unwrap();
    assert_eq!(resp.status(), 200);
    let pg_version = json_body(resp).await;
    assert_eq!(pg_version["version_number"], 1);
    assert_eq!(pg_version["placements"].as_array().unwrap().len(), 2);

    cleanup(&state, schema).await;
}

// ── Test: clear_on_snapshot clears Redis after snapshot ──────────────────────

#[tokio::test]
async fn clear_on_snapshot_empties_redis() {
    let schema = "test_rt_clear";
    let (state, app) = setup_with_redis(schema).await;
    flush_redis(&state).await;

    // Create board with clear_on_snapshot = true
    let resp = app
        .clone()
        .oneshot(json_request(
            "POST",
            "/boards",
            Some(serde_json::json!({
                "slug": "clearable",
                "name": "Clearable Board",
                "board_type": "scored",
                "accumulative": true,
                "realtime": true,
                "clear_on_snapshot": true,
                "sort_direction": "desc"
            })),
        ))
        .await
        .unwrap();
    assert_eq!(resp.status(), 201);

    // Submit a score
    let resp = app
        .clone()
        .oneshot(json_request(
            "POST",
            "/boards/clearable/scores",
            Some(serde_json::json!({
                "entry_slug": "player",
                "entry_name": "Player",
                "score": 42.0
            })),
        ))
        .await
        .unwrap();
    assert_eq!(resp.status(), 200);

    // Snapshot (should clear Redis after)
    let resp = app
        .clone()
        .oneshot(json_request(
            "POST",
            "/boards/clearable/snapshot",
            Some(serde_json::json!({})),
        ))
        .await
        .unwrap();
    assert_eq!(resp.status(), 201);

    // Latest should now show empty standings (0 placements)
    let resp = app
        .clone()
        .oneshot(json_request("GET", "/boards/clearable/latest", None))
        .await
        .unwrap();
    assert_eq!(resp.status(), 200);
    let body = json_body(resp).await;
    let placements = body["placements"].as_array().unwrap();
    assert_eq!(placements.len(), 0);

    cleanup(&state, schema).await;
}

// ── Test: Score update (ZADD replaces) ───────────────────────────────────────

#[tokio::test]
async fn score_update_replaces_previous() {
    let schema = "test_rt_update_score";
    let (state, app) = setup_with_redis(schema).await;
    flush_redis(&state).await;

    create_realtime_board(&app, "update-board", "Update Board").await;

    // Submit initial score
    let resp = app
        .clone()
        .oneshot(json_request(
            "POST",
            "/boards/update-board/scores",
            Some(serde_json::json!({
                "entry_slug": "player-1",
                "entry_name": "Player One",
                "score": 100.0
            })),
        ))
        .await
        .unwrap();
    assert_eq!(resp.status(), 200);

    // Update score
    let resp = app
        .clone()
        .oneshot(json_request(
            "POST",
            "/boards/update-board/scores",
            Some(serde_json::json!({
                "entry_slug": "player-1",
                "entry_name": "Player One Updated",
                "score": 999.0
            })),
        ))
        .await
        .unwrap();
    assert_eq!(resp.status(), 200);

    // Verify latest shows updated score
    let resp = app
        .clone()
        .oneshot(json_request("GET", "/boards/update-board/latest", None))
        .await
        .unwrap();
    assert_eq!(resp.status(), 200);
    let body = json_body(resp).await;
    let placements = body["placements"].as_array().unwrap();
    assert_eq!(placements.len(), 1);
    assert_eq!(placements[0]["score"], 999.0);
    assert_eq!(placements[0]["entry_name"], "Player One Updated");

    cleanup(&state, schema).await;
}

// ── Test: Ascending sort direction ───────────────────────────────────────────

#[tokio::test]
async fn realtime_ascending_sort() {
    let schema = "test_rt_asc";
    let (state, app) = setup_with_redis(schema).await;
    flush_redis(&state).await;

    // Create asc board
    let resp = app
        .clone()
        .oneshot(json_request(
            "POST",
            "/boards",
            Some(serde_json::json!({
                "slug": "asc-board",
                "name": "Ascending Board",
                "board_type": "scored",
                "accumulative": true,
                "realtime": true,
                "sort_direction": "asc"
            })),
        ))
        .await
        .unwrap();
    assert_eq!(resp.status(), 201);

    // Submit scores
    for (slug, name, score) in [("fast", "Fast", 10.0), ("slow", "Slow", 50.0)] {
        let resp = app
            .clone()
            .oneshot(json_request(
                "POST",
                "/boards/asc-board/scores",
                Some(serde_json::json!({
                    "entry_slug": slug,
                    "entry_name": name,
                    "score": score
                })),
            ))
            .await
            .unwrap();
        assert_eq!(resp.status(), 200);
    }

    // Latest should be ascending: Fast (10), Slow (50)
    let resp = app
        .clone()
        .oneshot(json_request("GET", "/boards/asc-board/latest", None))
        .await
        .unwrap();
    assert_eq!(resp.status(), 200);
    let body = json_body(resp).await;
    let placements = body["placements"].as_array().unwrap();
    assert_eq!(placements[0]["entry_slug"], "fast");
    assert_eq!(placements[0]["score"], 10.0);
    assert_eq!(placements[1]["entry_slug"], "slow");
    assert_eq!(placements[1]["score"], 50.0);

    cleanup(&state, schema).await;
}

// ── Test: Realtime board returns 503 when Redis not configured ───────────────

#[tokio::test]
async fn realtime_without_redis_returns_503() {
    let schema = "test_rt_no_redis";
    let (state, app) = setup_no_redis(schema).await;

    // Create a realtime board directly in Postgres (bypassing API validation
    // by using non-realtime first then updating)
    // Actually, we create it realtime — the create route doesn't need Redis.
    let resp = app
        .clone()
        .oneshot(json_request(
            "POST",
            "/boards",
            Some(serde_json::json!({
                "slug": "no-redis-board",
                "name": "No Redis Board",
                "board_type": "scored",
                "accumulative": true,
                "realtime": true,
                "sort_direction": "desc"
            })),
        ))
        .await
        .unwrap();
    assert_eq!(resp.status(), 201);

    // Score submission should fail with 503
    let resp = app
        .clone()
        .oneshot(json_request(
            "POST",
            "/boards/no-redis-board/scores",
            Some(serde_json::json!({
                "entry_slug": "test",
                "entry_name": "Test",
                "score": 1.0
            })),
        ))
        .await
        .unwrap();
    assert_eq!(resp.status(), 503);

    // Latest should fail with 503
    let resp = app
        .clone()
        .oneshot(json_request("GET", "/boards/no-redis-board/latest", None))
        .await
        .unwrap();
    assert_eq!(resp.status(), 503);

    // Snapshot should fail with 503
    let resp = app
        .clone()
        .oneshot(json_request(
            "POST",
            "/boards/no-redis-board/snapshot",
            Some(serde_json::json!({})),
        ))
        .await
        .unwrap();
    assert_eq!(resp.status(), 503);

    cleanup(&state, schema).await;
}

// ── Test: Multiple snapshots increment version numbers ───────────────────────

#[tokio::test]
async fn multiple_snapshots_increment_versions() {
    let schema = "test_rt_multi_snap";
    let (state, app) = setup_with_redis(schema).await;
    flush_redis(&state).await;

    create_realtime_board(&app, "multi-snap", "Multi Snap").await;

    // Submit and snapshot twice
    for round in 1..=2 {
        let resp = app
            .clone()
            .oneshot(json_request(
                "POST",
                "/boards/multi-snap/scores",
                Some(serde_json::json!({
                    "entry_slug": "player",
                    "entry_name": "Player",
                    "score": round as f64 * 100.0
                })),
            ))
            .await
            .unwrap();
        assert_eq!(resp.status(), 200);

        let resp = app
            .clone()
            .oneshot(json_request(
                "POST",
                "/boards/multi-snap/snapshot",
                Some(serde_json::json!({ "note": format!("Round {round}") })),
            ))
            .await
            .unwrap();
        assert_eq!(resp.status(), 201);
        let body = json_body(resp).await;
        assert_eq!(body["version_number"], round);
    }

    // Verify both versions exist via list
    let resp = app
        .clone()
        .oneshot(json_request("GET", "/boards/multi-snap/versions", None))
        .await
        .unwrap();
    assert_eq!(resp.status(), 200);
    let body = json_body(resp).await;
    assert_eq!(body["items"].as_array().unwrap().len(), 2);

    cleanup(&state, schema).await;
}

// ── Test: Snapshot with no scores returns 400 ────────────────────────────────

#[tokio::test]
async fn snapshot_empty_returns_400() {
    let schema = "test_rt_snap_empty";
    let (state, app) = setup_with_redis(schema).await;
    flush_redis(&state).await;

    create_realtime_board(&app, "empty-snap", "Empty Snap").await;

    let resp = app
        .clone()
        .oneshot(json_request(
            "POST",
            "/boards/empty-snap/snapshot",
            Some(serde_json::json!({})),
        ))
        .await
        .unwrap();
    assert_eq!(resp.status(), 400);

    cleanup(&state, schema).await;
}

// ── Test: Board deletion cleans up Redis keys ────────────────────────────────

#[tokio::test]
async fn delete_realtime_board_clears_redis() {
    let schema = "test_rt_delete";
    let (state, app) = setup_with_redis(schema).await;
    flush_redis(&state).await;

    create_realtime_board(&app, "del-board", "Delete Board").await;

    // Submit a score
    let resp = app
        .clone()
        .oneshot(json_request(
            "POST",
            "/boards/del-board/scores",
            Some(serde_json::json!({
                "entry_slug": "player",
                "entry_name": "Player",
                "score": 42.0
            })),
        ))
        .await
        .unwrap();
    assert_eq!(resp.status(), 200);

    // Delete the board
    let resp = app
        .clone()
        .oneshot(json_request("DELETE", "/boards/del-board", None))
        .await
        .unwrap();
    assert_eq!(resp.status(), 204);

    // Recreate same slug — should start fresh with no stale scores
    create_realtime_board(&app, "del-board", "Delete Board Reborn").await;

    // Latest should show empty (no stale scores from previous board)
    let resp = app
        .clone()
        .oneshot(json_request("GET", "/boards/del-board/latest", None))
        .await
        .unwrap();
    assert_eq!(resp.status(), 200);
    let body = json_body(resp).await;
    let placements = body["placements"].as_array().unwrap();
    assert_eq!(
        placements.len(),
        0,
        "expected no stale scores after delete+recreate"
    );

    cleanup(&state, schema).await;
}
